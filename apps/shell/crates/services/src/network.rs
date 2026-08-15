use std::{
    collections::{BTreeMap, HashMap},
    sync::mpsc,
    thread,
    time::Duration,
};

use zbus::{
    Message,
    blocking::{Connection, Proxy},
    zvariant::{OwnedObjectPath, OwnedValue},
};

use crate::{
    Availability, Connectivity, Event, NetworkSnapshot, WifiNetwork,
    events::{Bus, start_dbus_signal_monitor},
    process::{run, run_with_stdin},
};

const EVENT_SETTLE: Duration = Duration::from_millis(75);

#[derive(Clone)]
pub(crate) enum Command {
    WifiEnabled(bool),
    Refresh,
    Connect {
        ssid: String,
        password: Option<String>,
        saved_uuid: Option<String>,
    },
    Disconnect,
    Forget(String),
    EventRefresh,
}

pub(crate) fn start(events: async_channel::Sender<Event>) -> mpsc::Sender<Command> {
    let (commands, receiver) = mpsc::channel();
    start_dbus_signal_monitor(
        "foyer-shell-network-events",
        Bus::System,
        "org.freedesktop.NetworkManager",
        commands.clone(),
        Command::EventRefresh,
        network_event_requires_refresh,
    );
    thread::Builder::new()
        .name("foyer-shell-network-service".into())
        .spawn(move || run_worker(events, receiver))
        .expect("failed to start network service worker");
    commands
}

fn network_event_requires_refresh(message: &Message) -> bool {
    let header = message.header();
    let interface = header.interface().map(|interface| interface.as_str());
    let member = header.member().map(|member| member.as_str());
    if interface == Some("org.freedesktop.DBus.Properties") && member == Some("PropertiesChanged") {
        let Ok((changed_interface, changed, _invalidated)) =
            message
                .body()
                .deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>()
        else {
            return false;
        };
        return properties_require_refresh(&changed_interface, changed.keys().map(String::as_str));
    }
    signal_requires_refresh(interface, member)
}

fn signal_requires_refresh(interface: Option<&str>, member: Option<&str>) -> bool {
    matches!(
        (interface, member),
        (
            Some("org.freedesktop.NetworkManager.Device.Wireless"),
            Some("AccessPointAdded" | "AccessPointRemoved")
        ) | (
            Some("org.freedesktop.NetworkManager.Settings"),
            Some("NewConnection" | "ConnectionRemoved")
        ) | (
            Some("org.freedesktop.NetworkManager.Settings.Connection"),
            Some("Updated" | "Removed")
        )
    )
}

fn properties_require_refresh<'a>(interface: &str, changed: impl Iterator<Item = &'a str>) -> bool {
    changed.into_iter().any(|property| match interface {
        "org.freedesktop.NetworkManager" => matches!(
            property,
            "State"
                | "Connectivity"
                | "WirelessEnabled"
                | "WirelessHardwareEnabled"
                | "PrimaryConnection"
                | "ActivatingConnection"
        ),
        "org.freedesktop.NetworkManager.Device" => {
            matches!(property, "State" | "ActiveConnection" | "Managed")
        }
        "org.freedesktop.NetworkManager.Device.Wireless" => {
            matches!(property, "ActiveAccessPoint")
        }
        _ => false,
    })
}

fn run_worker(events: async_channel::Sender<Event>, commands: mpsc::Receiver<Command>) {
    let mut snapshot = NetworkSnapshot::default();
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
                        error = execute(command, &snapshot).err().or(error);
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
    next: NetworkSnapshot,
    snapshot: &mut NetworkSnapshot,
    events: &async_channel::Sender<Event>,
) {
    if &next != snapshot {
        *snapshot = next;
        let _ = events.send_blocking(Event::Network(snapshot.clone()));
    }
}

fn operation_label(command: &Command) -> &'static str {
    match command {
        Command::WifiEnabled(_) => "Changing Wi-Fi radio…",
        Command::Refresh => "Scanning for networks…",
        Command::Connect { .. } => "Connecting…",
        Command::Disconnect => "Disconnecting…",
        Command::Forget(_) => "Forgetting network…",
        Command::EventRefresh => "Refreshing network state…",
    }
}

fn query() -> NetworkSnapshot {
    let general = match run_nmcli(&["--terse", "--fields", "STATE,CONNECTIVITY", "general"]) {
        Ok(output) => output,
        Err(error) => return unavailable(error),
    };
    let general_fields = general
        .lines()
        .next()
        .map(parse_terse_fields)
        .unwrap_or_default();
    let connectivity = match general_fields.get(1).map(String::as_str) {
        Some("full") => Connectivity::Full,
        Some("limited") => Connectivity::Limited,
        Some("portal") => Connectivity::Portal,
        Some("none") => Connectivity::None,
        _ => Connectivity::Unknown,
    };

    let radio_fields = run_nmcli(&["--terse", "--fields", "WIFI-HW,WIFI", "radio"])
        .ok()
        .and_then(|output| output.lines().next().map(parse_terse_fields))
        .unwrap_or_default();
    let wifi_enabled = radio_fields.get(1).is_some_and(|state| state == "enabled");

    let active_device = run_nmcli(&[
        "--terse",
        "--fields",
        "DEVICE,TYPE,STATE,CONNECTION",
        "device",
        "status",
    ])
    .ok()
    .and_then(|output| {
        output.lines().find_map(|line| {
            let fields = parse_terse_fields(line);
            (fields.get(1).is_some_and(|kind| kind == "wifi")
                && fields.get(2).is_some_and(|state| state == "connected"))
            .then(|| {
                (
                    fields.first().cloned(),
                    fields.get(3).cloned().filter(|value| !value.is_empty()),
                )
            })
        })
    });

    let saved = saved_wifi_profiles();
    let networks = if wifi_enabled {
        query_access_points(&saved).unwrap_or_default()
    } else {
        Vec::new()
    };
    let active = networks.iter().find(|network| network.active);

    NetworkSnapshot {
        availability: Availability::Available,
        connectivity,
        wifi_enabled,
        device: active_device
            .as_ref()
            .and_then(|(device, _)| device.clone()),
        connection: active
            .map(|network| network.ssid.clone())
            .or_else(|| active_device.and_then(|(_, connection)| connection)),
        signal: active.map(|network| network.signal),
        security: active.and_then(|network| network.security.clone()),
        networks,
        busy: None,
        last_error: None,
    }
}

fn unavailable(error: String) -> NetworkSnapshot {
    NetworkSnapshot {
        availability: Availability::Unavailable(error),
        ..Default::default()
    }
}

#[derive(Clone)]
struct SavedWifi {
    uuid: String,
    ssid: String,
}

fn saved_wifi_profiles() -> Vec<SavedWifi> {
    let Ok(connection) = Connection::system() else {
        return Vec::new();
    };
    let Ok(settings) = Proxy::new(
        &connection,
        "org.freedesktop.NetworkManager",
        "/org/freedesktop/NetworkManager/Settings",
        "org.freedesktop.NetworkManager.Settings",
    ) else {
        return Vec::new();
    };
    let Ok(paths) = settings.call::<_, _, Vec<OwnedObjectPath>>("ListConnections", &()) else {
        return Vec::new();
    };
    paths
        .into_iter()
        .filter_map(|path| saved_wifi_profile(&connection, &path))
        .collect()
}

type ConnectionSettings = HashMap<String, HashMap<String, OwnedValue>>;

fn saved_wifi_profile(connection: &Connection, path: &OwnedObjectPath) -> Option<SavedWifi> {
    let proxy = Proxy::new(
        connection,
        "org.freedesktop.NetworkManager",
        path.as_str(),
        "org.freedesktop.NetworkManager.Settings.Connection",
    )
    .ok()?;
    let settings = proxy
        .call::<_, _, ConnectionSettings>("GetSettings", &())
        .ok()?;
    saved_wifi_from_settings(&settings)
}

fn saved_wifi_from_settings(settings: &ConnectionSettings) -> Option<SavedWifi> {
    let connection = settings.get("connection")?;
    let kind = setting_string(connection, "type")?;
    if kind != "802-11-wireless" && kind != "wifi" {
        return None;
    }
    let uuid = setting_string(connection, "uuid")?;
    let wireless = settings.get("802-11-wireless")?;
    let value = wireless.get("ssid")?.try_clone().ok()?;
    let ssid = String::from_utf8(Vec::<u8>::try_from(value).ok()?).ok()?;
    (!ssid.is_empty()).then_some(SavedWifi { uuid, ssid })
}

fn setting_string(settings: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    <&str>::try_from(settings.get(key)?).ok().map(str::to_owned)
}

fn query_access_points(saved: &[SavedWifi]) -> Result<Vec<WifiNetwork>, String> {
    let output = run_nmcli(&[
        "--terse",
        "--fields",
        "IN-USE,SSID,SIGNAL,SECURITY",
        "device",
        "wifi",
        "list",
        "--rescan",
        "no",
    ])?;
    let mut networks = BTreeMap::<String, WifiNetwork>::new();
    for line in output.lines() {
        let fields = parse_terse_fields(line);
        let Some(ssid) = fields.get(1).filter(|ssid| !ssid.is_empty()).cloned() else {
            continue;
        };
        let signal = fields
            .get(2)
            .and_then(|signal| signal.parse::<u8>().ok())
            .unwrap_or(0)
            .min(100);
        let active = fields.first().is_some_and(|value| value == "*");
        let security = fields
            .get(3)
            .cloned()
            .filter(|value| !value.is_empty() && value != "--");
        let saved_uuid = saved
            .iter()
            .find(|profile| profile.ssid == ssid)
            .map(|profile| profile.uuid.clone());
        let candidate = WifiNetwork {
            ssid: ssid.clone(),
            signal,
            security,
            active,
            saved_uuid,
        };
        networks
            .entry(ssid)
            .and_modify(|existing| {
                if candidate.active || (!existing.active && candidate.signal > existing.signal) {
                    *existing = candidate.clone();
                }
            })
            .or_insert(candidate);
    }
    let mut networks = networks.into_values().collect::<Vec<_>>();
    networks.sort_by(|left, right| {
        right
            .active
            .cmp(&left.active)
            .then_with(|| right.signal.cmp(&left.signal))
            .then_with(|| left.ssid.cmp(&right.ssid))
    });
    Ok(networks)
}

fn execute(command: Command, snapshot: &NetworkSnapshot) -> Result<(), String> {
    match command {
        Command::WifiEnabled(enabled) => {
            run_nmcli(&["radio", "wifi", if enabled { "on" } else { "off" }]).map(|_| ())
        }
        Command::Refresh => {
            let mut arguments = vec!["device", "wifi", "rescan"];
            if let Some(device) = snapshot.device.as_deref() {
                arguments.extend(["ifname", device]);
            }
            run_nmcli(&arguments).map(|_| ())
        }
        Command::Connect {
            ssid,
            password,
            saved_uuid,
        } => {
            if let Some(uuid) = saved_uuid {
                return run_nmcli(&["connection", "up", "uuid", &uuid]).map(|_| ());
            }
            let mut arguments = vec!["device", "wifi", "connect", ssid.as_str()];
            if let Some(device) = snapshot.device.as_deref() {
                arguments.extend(["ifname", device]);
            }
            match password {
                Some(password) => {
                    let mut prompt_arguments = vec!["--ask"];
                    prompt_arguments.extend(arguments);
                    let input = format!("{password}\n");
                    run_with_stdin("nmcli", &prompt_arguments, &input).map(|_| ())
                }
                None => run_nmcli(&arguments).map(|_| ()),
            }
        }
        Command::Disconnect => {
            let Some(device) = snapshot.device.as_deref() else {
                return Err("No connected Wi-Fi device".into());
            };
            run_nmcli(&["device", "disconnect", device]).map(|_| ())
        }
        Command::Forget(uuid) => run_nmcli(&["connection", "delete", "uuid", &uuid]).map(|_| ()),
        Command::EventRefresh => Ok(()),
    }
}

fn run_nmcli(arguments: &[&str]) -> Result<String, String> {
    run("nmcli", arguments)
}

pub(crate) fn parse_terse_fields(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut escaped = false;
    for character in line.chars() {
        if escaped {
            field.push(character);
            escaped = false;
        } else {
            match character {
                '\\' => escaped = true,
                ':' => fields.push(std::mem::take(&mut field)),
                _ => field.push(character),
            }
        }
    }
    if escaped {
        field.push('\\');
    }
    fields.push(field);
    fields
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::Value;

    fn owned(value: impl Into<Value<'static>>) -> OwnedValue {
        OwnedValue::try_from(value.into()).unwrap()
    }

    #[test]
    fn parses_nmcli_escaped_fields() {
        assert_eq!(
            parse_terse_fields(r"*:Cafe\: Upstairs:71:WPA2\\WPA3"),
            ["*", "Cafe: Upstairs", "71", "WPA2\\WPA3"]
        );
        assert_eq!(
            parse_terse_fields("wlo1:wifi:connected:"),
            ["wlo1", "wifi", "connected", ""]
        );
    }

    #[test]
    fn reads_saved_wifi_from_networkmanager_settings() {
        let settings = ConnectionSettings::from([
            (
                "connection".into(),
                HashMap::from([
                    ("uuid".into(), owned("profile-1")),
                    ("type".into(), owned("802-11-wireless")),
                ]),
            ),
            (
                "802-11-wireless".into(),
                HashMap::from([("ssid".into(), owned(b"Foyer Wi-Fi".to_vec()))]),
            ),
        ]);
        let profile = saved_wifi_from_settings(&settings).unwrap();
        assert_eq!(profile.uuid, "profile-1");
        assert_eq!(profile.ssid, "Foyer Wi-Fi");
    }

    #[test]
    fn ignores_periodic_access_point_strength_notifications() {
        assert!(!properties_require_refresh(
            "org.freedesktop.NetworkManager.AccessPoint",
            ["Strength"].into_iter(),
        ));
        assert!(!properties_require_refresh(
            "org.freedesktop.NetworkManager.Device.Statistics",
            ["TxBytes", "RxBytes"].into_iter(),
        ));
        assert!(properties_require_refresh(
            "org.freedesktop.NetworkManager.Device.Wireless",
            ["ActiveAccessPoint"].into_iter(),
        ));
        assert!(signal_requires_refresh(
            Some("org.freedesktop.NetworkManager.Device.Wireless"),
            Some("AccessPointAdded"),
        ));
        assert!(!signal_requires_refresh(
            Some("org.freedesktop.NetworkManager"),
            Some("DeviceAdded"),
        ));
    }

    #[test]
    #[ignore = "requires a running host NetworkManager"]
    fn live_network_snapshot_uses_host_dbus_and_nmcli() {
        assert!(query().availability.is_available());
    }
}
