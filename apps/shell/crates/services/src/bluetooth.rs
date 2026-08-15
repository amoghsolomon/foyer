use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use zbus::{
    DBusError,
    blocking::{Connection, Proxy},
    zvariant::{ObjectPath, OwnedObjectPath, OwnedValue},
};

use crate::{
    Availability, BluetoothDevice, BluetoothPairingKind, BluetoothPairingRequest,
    BluetoothSnapshot, Event,
    events::{Bus, start_dbus_signal_monitor},
    process::run,
};

const AGENT_PATH: &str = "/org/amazity/FoyerShell/BluetoothAgent";
const PAIRING_TIMEOUT: Duration = Duration::from_secs(90);
const EVENT_SETTLE: Duration = Duration::from_millis(75);

#[derive(Clone, Debug)]
pub(crate) enum Command {
    Powered(bool),
    Refresh,
    Connect(String),
    Disconnect(String),
    Pair(String),
    Remove(String),
    EventRefresh,
}

pub(crate) struct Runtime {
    pub commands: mpsc::Sender<Command>,
    pub pairing: PairingController,
}

#[derive(Clone, Debug)]
pub(crate) struct PairingController {
    state: Arc<PairingState>,
}

#[derive(Debug)]
struct PairingState {
    next_id: AtomicU64,
    pending: Mutex<HashMap<u64, mpsc::Sender<PairingResponse>>>,
    events: async_channel::Sender<Event>,
}

#[derive(Clone, Debug)]
enum PairingResponse {
    Value(String),
    Confirm(bool),
    Canceled,
}

impl PairingController {
    pub fn answer(&self, id: u64, value: String) {
        self.respond(id, PairingResponse::Value(value));
    }

    pub fn confirm(&self, id: u64, accepted: bool) {
        self.respond(id, PairingResponse::Confirm(accepted));
    }

    pub fn cancel(&self, id: u64) {
        self.respond(id, PairingResponse::Canceled);
        let _ = self
            .state
            .events
            .send_blocking(Event::BluetoothPairing(None));
    }

    fn respond(&self, id: u64, response: PairingResponse) {
        if let Some(sender) = self
            .state
            .pending
            .lock()
            .ok()
            .and_then(|mut pending| pending.remove(&id))
        {
            let _ = sender.send(response);
        }
    }

    fn request(
        &self,
        address: String,
        name: String,
        kind: BluetoothPairingKind,
    ) -> Result<PairingResponse, AgentError> {
        let id = self.state.next_id.fetch_add(1, Ordering::Relaxed).max(1);
        let (sender, receiver) = mpsc::channel();
        self.state
            .pending
            .lock()
            .map_err(|_| AgentError::Rejected("Pairing state is unavailable".into()))?
            .insert(id, sender);
        let _ = self
            .state
            .events
            .send_blocking(Event::BluetoothPairing(Some(BluetoothPairingRequest {
                id,
                address,
                name,
                kind,
            })));
        let response = receiver
            .recv_timeout(PAIRING_TIMEOUT)
            .map_err(|_| AgentError::Canceled("Pairing request timed out".into()));
        if let Ok(mut pending) = self.state.pending.lock() {
            pending.remove(&id);
        }
        let _ = self
            .state
            .events
            .send_blocking(Event::BluetoothPairing(None));
        response
    }

    fn display(&self, address: String, name: String, kind: BluetoothPairingKind) {
        let id = self.state.next_id.fetch_add(1, Ordering::Relaxed).max(1);
        let _ = self
            .state
            .events
            .send_blocking(Event::BluetoothPairing(Some(BluetoothPairingRequest {
                id,
                address,
                name,
                kind,
            })));
    }
}

pub(crate) fn start(events: async_channel::Sender<Event>) -> Runtime {
    let (commands, receiver) = mpsc::channel();
    let pairing = PairingController {
        state: Arc::new(PairingState {
            next_id: AtomicU64::new(1),
            pending: Mutex::new(HashMap::new()),
            events: events.clone(),
        }),
    };
    start_agent(pairing.clone());
    start_dbus_signal_monitor(
        "foyer-shell-bluetooth-events",
        Bus::System,
        "org.bluez",
        commands.clone(),
        Command::EventRefresh,
        |_| true,
    );
    thread::Builder::new()
        .name("foyer-shell-bluetooth-service".into())
        .spawn(move || run_worker(events, receiver))
        .expect("failed to start Bluetooth service worker");
    Runtime { commands, pairing }
}

fn run_worker(events: async_channel::Sender<Event>, commands: mpsc::Receiver<Command>) {
    let mut snapshot = BluetoothSnapshot::default();
    loop {
        publish_if_changed(query(), &mut snapshot, &events);
        match commands.recv() {
            Ok(command) => {
                let pending = coalesce_pending(command, &commands);
                let operation = pending
                    .iter()
                    .find(|command| !matches!(command, Command::EventRefresh));
                let mut busy = snapshot.clone();
                busy.busy = operation.map(|command| operation_label(command).into());
                busy.last_error = None;
                publish_if_changed(busy, &mut snapshot, &events);
                let mut error = None;
                for command in pending {
                    if !matches!(command, Command::EventRefresh) {
                        error = execute(command).err().or(error);
                    }
                }
                let mut next = query();
                next.last_error = error;
                publish_if_changed(next, &mut snapshot, &events);
            }
            Err(_) => return,
        }
    }
}

fn coalesce_pending(first: Command, receiver: &mpsc::Receiver<Command>) -> Vec<Command> {
    if matches!(first, Command::EventRefresh) {
        thread::sleep(EVENT_SETTLE);
    }
    let mut pending = vec![first];
    while let Ok(next) = receiver.try_recv() {
        if matches!(next, Command::EventRefresh)
            && pending
                .iter()
                .any(|command| matches!(command, Command::EventRefresh))
        {
            continue;
        }
        pending.push(next);
    }
    pending
}

fn publish_if_changed(
    next: BluetoothSnapshot,
    snapshot: &mut BluetoothSnapshot,
    events: &async_channel::Sender<Event>,
) {
    if &next != snapshot {
        *snapshot = next;
        let _ = events.send_blocking(Event::Bluetooth(snapshot.clone()));
    }
}

fn operation_label(command: &Command) -> &'static str {
    match command {
        Command::Powered(_) => "Changing Bluetooth radio…",
        Command::Refresh => "Scanning for devices…",
        Command::Connect(_) => "Connecting device…",
        Command::Disconnect(_) => "Disconnecting device…",
        Command::Pair(_) => "Pairing device…",
        Command::Remove(_) => "Removing device…",
        Command::EventRefresh => "Refreshing Bluetooth state…",
    }
}

fn query() -> BluetoothSnapshot {
    let connection = match Connection::system() {
        Ok(connection) => connection,
        Err(error) => return unavailable(error.to_string()),
    };
    let manager = match Proxy::new(
        &connection,
        "org.bluez",
        "/",
        "org.freedesktop.DBus.ObjectManager",
    ) {
        Ok(manager) => manager,
        Err(error) => return unavailable(error.to_string()),
    };
    let objects = match manager.call::<_, _, ManagedObjects>("GetManagedObjects", &()) {
        Ok(objects) => objects,
        Err(error) => return unavailable(error.to_string()),
    };
    let Some(adapter) = objects
        .values()
        .find_map(|interfaces| interfaces.get("org.bluez.Adapter1"))
    else {
        return unavailable("No Bluetooth controller".into());
    };
    let powered = bool_value(adapter, "Powered");
    let discovering = bool_value(adapter, "Discovering");
    let devices = if powered {
        bluez_devices(&objects)
    } else {
        Vec::new()
    };
    BluetoothSnapshot {
        availability: Availability::Available,
        powered,
        discovering,
        devices,
        busy: None,
        last_error: None,
        pairing: None,
    }
}

type BluezProperties = HashMap<String, OwnedValue>;
type BluezInterfaces = HashMap<String, BluezProperties>;
type ManagedObjects = HashMap<OwnedObjectPath, BluezInterfaces>;

fn unavailable(error: String) -> BluetoothSnapshot {
    BluetoothSnapshot {
        availability: Availability::Unavailable(error),
        ..Default::default()
    }
}

fn bluez_devices(objects: &ManagedObjects) -> Vec<BluetoothDevice> {
    let mut devices = objects
        .iter()
        .filter_map(|(path, interfaces)| {
            let properties = interfaces.get("org.bluez.Device1")?;
            let address = string_value(properties, "Address").unwrap_or_else(|| {
                path.as_str()
                    .rsplit('/')
                    .next()
                    .unwrap_or("Bluetooth device")
                    .trim_start_matches("dev_")
                    .replace('_', ":")
            });
            let battery_percent = interfaces
                .get("org.bluez.Battery1")
                .and_then(|battery| u8_value(battery, "Percentage"));
            Some(BluetoothDevice {
                name: string_value(properties, "Alias")
                    .or_else(|| string_value(properties, "Name"))
                    .unwrap_or_else(|| address.clone()),
                address,
                icon: string_value(properties, "Icon"),
                paired: bool_value(properties, "Paired"),
                trusted: bool_value(properties, "Trusted"),
                connected: bool_value(properties, "Connected"),
                battery_percent,
            })
        })
        .collect::<Vec<_>>();
    devices.sort_by(|left, right| {
        right
            .connected
            .cmp(&left.connected)
            .then_with(|| right.paired.cmp(&left.paired))
            .then_with(|| left.name.cmp(&right.name))
    });
    devices
}

fn string_value(properties: &BluezProperties, name: &str) -> Option<String> {
    <&str>::try_from(properties.get(name)?)
        .ok()
        .map(str::to_owned)
}

fn bool_value(properties: &BluezProperties, name: &str) -> bool {
    properties
        .get(name)
        .and_then(|value| bool::try_from(value).ok())
        .unwrap_or(false)
}

fn u8_value(properties: &BluezProperties, name: &str) -> Option<u8> {
    u8::try_from(properties.get(name)?).ok()
}

fn execute(command: Command) -> Result<(), String> {
    match command {
        Command::Powered(powered) => run(
            "bluetoothctl",
            &["power", if powered { "on" } else { "off" }],
        )
        .map(|_| ()),
        Command::Refresh => run("bluetoothctl", &["--timeout", "6", "scan", "on"]).map(|_| ()),
        Command::Connect(address) => run("bluetoothctl", &["connect", &address]).map(|_| ()),
        Command::Disconnect(address) => run("bluetoothctl", &["disconnect", &address]).map(|_| ()),
        Command::Pair(address) => {
            run("bluetoothctl", &["--timeout", "90", "pair", &address])?;
            run("bluetoothctl", &["trust", &address])?;
            run("bluetoothctl", &["connect", &address]).map(|_| ())
        }
        Command::Remove(address) => run("bluetoothctl", &["remove", &address]).map(|_| ()),
        Command::EventRefresh => Ok(()),
    }
}

#[derive(Debug, DBusError)]
#[zbus(prefix = "org.bluez.Error")]
enum AgentError {
    Rejected(String),
    Canceled(String),
    #[zbus(error)]
    ZBus(zbus::Error),
}

struct BluetoothAgent {
    pairing: PairingController,
}

#[zbus::interface(name = "org.bluez.Agent1")]
impl BluetoothAgent {
    fn release(&self) {
        let _ = self
            .pairing
            .state
            .events
            .send_blocking(Event::BluetoothPairing(None));
    }

    fn request_pin_code(&self, device: OwnedObjectPath) -> Result<String, AgentError> {
        let (address, name) = device_identity(&device);
        match self
            .pairing
            .request(address, name, BluetoothPairingKind::PinCode)?
        {
            PairingResponse::Value(value) if !value.is_empty() => {
                Ok(value.chars().take(16).collect())
            }
            PairingResponse::Canceled => Err(AgentError::Canceled("Pairing canceled".into())),
            _ => Err(AgentError::Rejected("A PIN code is required".into())),
        }
    }

    fn display_pin_code(&self, device: OwnedObjectPath, pin_code: String) {
        let (address, name) = device_identity(&device);
        self.pairing.display(
            address,
            name,
            BluetoothPairingKind::DisplayPinCode(pin_code),
        );
    }

    fn request_passkey(&self, device: OwnedObjectPath) -> Result<u32, AgentError> {
        let (address, name) = device_identity(&device);
        match self
            .pairing
            .request(address, name, BluetoothPairingKind::Passkey)?
        {
            PairingResponse::Value(value) => value
                .parse::<u32>()
                .ok()
                .filter(|value| *value <= 999_999)
                .ok_or_else(|| AgentError::Rejected("Enter a six-digit passkey".into())),
            PairingResponse::Canceled => Err(AgentError::Canceled("Pairing canceled".into())),
            _ => Err(AgentError::Rejected("A numeric passkey is required".into())),
        }
    }

    fn display_passkey(&self, device: OwnedObjectPath, passkey: u32, entered: u16) {
        let (address, name) = device_identity(&device);
        self.pairing.display(
            address,
            name,
            BluetoothPairingKind::DisplayPasskey { passkey, entered },
        );
    }

    fn request_confirmation(
        &self,
        device: OwnedObjectPath,
        passkey: u32,
    ) -> Result<(), AgentError> {
        let (address, name) = device_identity(&device);
        match self
            .pairing
            .request(address, name, BluetoothPairingKind::ConfirmPasskey(passkey))?
        {
            PairingResponse::Confirm(true) => Ok(()),
            PairingResponse::Canceled => Err(AgentError::Canceled("Pairing canceled".into())),
            _ => Err(AgentError::Rejected("Passkey was rejected".into())),
        }
    }

    fn request_authorization(&self, device: OwnedObjectPath) -> Result<(), AgentError> {
        let (address, name) = device_identity(&device);
        authorize(
            self.pairing
                .request(address, name, BluetoothPairingKind::Authorize)?,
        )
    }

    fn authorize_service(&self, device: OwnedObjectPath, uuid: String) -> Result<(), AgentError> {
        let (address, name) = device_identity(&device);
        authorize(self.pairing.request(
            address,
            name,
            BluetoothPairingKind::AuthorizeService(uuid),
        )?)
    }

    fn cancel(&self) {
        let pending = self
            .pairing
            .state
            .pending
            .lock()
            .map(|mut pending| {
                pending
                    .drain()
                    .map(|(_, sender)| sender)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for sender in pending {
            let _ = sender.send(PairingResponse::Canceled);
        }
        let _ = self
            .pairing
            .state
            .events
            .send_blocking(Event::BluetoothPairing(None));
    }
}

fn authorize(response: PairingResponse) -> Result<(), AgentError> {
    match response {
        PairingResponse::Confirm(true) => Ok(()),
        PairingResponse::Canceled => Err(AgentError::Canceled("Pairing canceled".into())),
        _ => Err(AgentError::Rejected("Authorization was rejected".into())),
    }
}

fn start_agent(pairing: PairingController) {
    thread::Builder::new()
        .name("foyer-shell-bluetooth-agent".into())
        .spawn(move || {
            let connection = match Connection::system() {
                Ok(connection) => connection,
                Err(error) => {
                    tracing::warn!(%error, "could not connect the Bluetooth pairing agent");
                    return;
                }
            };
            if let Err(error) = connection
                .object_server()
                .at(AGENT_PATH, BluetoothAgent { pairing })
            {
                tracing::warn!(%error, "could not export the Bluetooth pairing agent");
                return;
            }
            let manager = match Proxy::new(
                &connection,
                "org.bluez",
                "/org/bluez",
                "org.bluez.AgentManager1",
            ) {
                Ok(manager) => manager,
                Err(error) => {
                    tracing::warn!(%error, "could not access the BlueZ agent manager");
                    return;
                }
            };
            let path = ObjectPath::try_from(AGENT_PATH).expect("static Bluetooth agent path");
            let _ = manager.call_method("UnregisterAgent", &(path.clone()));
            if let Err(error) =
                manager.call_method("RegisterAgent", &(path.clone(), "KeyboardDisplay"))
            {
                tracing::warn!(%error, "could not register the Bluetooth pairing agent");
                return;
            }
            if let Err(error) = manager.call_method("RequestDefaultAgent", &(path)) {
                tracing::warn!(%error, "could not make Foyer Shell the default Bluetooth pairing agent");
                return;
            }
            tracing::info!("registered Foyer Shell as the BlueZ pairing agent");
            loop {
                thread::park_timeout(Duration::from_secs(60));
                if connection.is_closed() {
                    return;
                }
            }
        })
        .expect("failed to start Bluetooth agent worker");
}

fn device_identity(device: &OwnedObjectPath) -> (String, String) {
    let fallback = device
        .as_str()
        .rsplit('/')
        .next()
        .unwrap_or("Bluetooth device")
        .strip_prefix("dev_")
        .unwrap_or("Bluetooth device")
        .replace('_', ":");
    let name = Connection::system()
        .ok()
        .and_then(|connection| {
            Proxy::new(
                &connection,
                "org.bluez",
                device.as_str(),
                "org.bluez.Device1",
            )
            .ok()?
            .get_property::<String>("Alias")
            .ok()
        })
        .unwrap_or_else(|| fallback.clone());
    (fallback, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::Value;

    fn owned(value: impl Into<Value<'static>>) -> OwnedValue {
        OwnedValue::try_from(value.into()).unwrap()
    }

    #[test]
    fn reads_bluez_objects_in_one_snapshot() {
        let path = OwnedObjectPath::try_from("/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF").unwrap();
        let objects = ManagedObjects::from([(
            path,
            BluezInterfaces::from([
                (
                    "org.bluez.Device1".into(),
                    BluezProperties::from([
                        ("Address".into(), owned("AA:BB:CC:DD:EE:FF")),
                        ("Alias".into(), owned("Headphones")),
                        ("Connected".into(), OwnedValue::from(true)),
                    ]),
                ),
                (
                    "org.bluez.Battery1".into(),
                    BluezProperties::from([("Percentage".into(), OwnedValue::from(75_u8))]),
                ),
            ]),
        )]);
        let devices = bluez_devices(&objects);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].name, "Headphones");
        assert!(devices[0].connected);
        assert_eq!(devices[0].battery_percent, Some(75));
    }

    #[test]
    #[ignore = "requires a running host BlueZ"]
    fn live_bluetooth_snapshot_uses_bluez_object_manager() {
        assert!(query().availability.is_available());
    }

    #[test]
    fn pairing_response_is_bound_to_one_request_id() {
        let (events, updates) = async_channel::unbounded();
        let controller = PairingController {
            state: Arc::new(PairingState {
                next_id: AtomicU64::new(1),
                pending: Mutex::new(HashMap::new()),
                events,
            }),
        };
        let requester = controller.clone();
        let task = thread::spawn(move || {
            requester.request(
                "AA:BB:CC:DD:EE:FF".into(),
                "Keyboard".into(),
                BluetoothPairingKind::ConfirmPasskey(123456),
            )
        });
        let request = match updates.recv_blocking().unwrap() {
            Event::BluetoothPairing(Some(request)) => request,
            event => panic!("unexpected event: {event:?}"),
        };
        controller.confirm(request.id + 1, true);
        assert!(!task.is_finished());
        controller.confirm(request.id, true);
        assert!(matches!(
            task.join().unwrap(),
            Ok(PairingResponse::Confirm(true))
        ));
        assert!(matches!(
            updates.recv_blocking().unwrap(),
            Event::BluetoothPairing(None)
        ));
    }
}
