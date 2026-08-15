//! Typed, independently retrying adapters for desktop system services.

mod audio;
mod bluetooth;
mod display;
mod events;
mod media;
mod network;
pub mod notifications;
mod power;
mod process;
mod tray;

use std::sync::mpsc;

use async_channel::Receiver;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Availability {
    Loading,
    Available,
    Unavailable(String),
}

impl Availability {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }

    pub fn detail(&self) -> &str {
        match self {
            Self::Loading => "Connecting…",
            Self::Available => "Available",
            Self::Unavailable(error) => error,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioDevice {
    pub id: String,
    pub description: String,
    pub volume: f32,
    pub muted: bool,
    pub is_default: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioStream {
    pub id: u32,
    pub name: String,
    pub volume: f32,
    pub muted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordingApp {
    pub id: u32,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioSnapshot {
    pub availability: Availability,
    // Compatibility fields retained for OSD and the toolbar.
    pub volume: f32,
    pub muted: bool,
    pub device: String,
    pub outputs: Vec<AudioDevice>,
    pub input_volume: f32,
    pub input_muted: bool,
    pub input_device: String,
    pub inputs: Vec<AudioDevice>,
    pub streams: Vec<AudioStream>,
    pub recording_apps: Vec<RecordingApp>,
    pub last_error: Option<String>,
}

impl Default for AudioSnapshot {
    fn default() -> Self {
        Self {
            availability: Availability::Loading,
            volume: 0.0,
            muted: false,
            device: "Default output".into(),
            outputs: Vec::new(),
            input_volume: 0.0,
            input_muted: false,
            input_device: "Default input".into(),
            inputs: Vec::new(),
            streams: Vec::new(),
            recording_apps: Vec::new(),
            last_error: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Connectivity {
    #[default]
    Unknown,
    None,
    Portal,
    Limited,
    Full,
}

impl Connectivity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::None => "Offline",
            Self::Portal => "Sign-in required",
            Self::Limited => "Limited",
            Self::Full => "Connected",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WifiNetwork {
    pub ssid: String,
    pub signal: u8,
    pub security: Option<String>,
    pub active: bool,
    pub saved_uuid: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkSnapshot {
    pub availability: Availability,
    pub connectivity: Connectivity,
    pub wifi_enabled: bool,
    pub device: Option<String>,
    pub connection: Option<String>,
    pub signal: Option<u8>,
    pub security: Option<String>,
    pub networks: Vec<WifiNetwork>,
    pub busy: Option<String>,
    pub last_error: Option<String>,
}

impl Default for NetworkSnapshot {
    fn default() -> Self {
        Self {
            availability: Availability::Loading,
            connectivity: Connectivity::Unknown,
            wifi_enabled: false,
            device: None,
            connection: None,
            signal: None,
            security: None,
            networks: Vec::new(),
            busy: None,
            last_error: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BluetoothDevice {
    pub address: String,
    pub name: String,
    pub icon: Option<String>,
    pub paired: bool,
    pub trusted: bool,
    pub connected: bool,
    pub battery_percent: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BluetoothPairingKind {
    PinCode,
    Passkey,
    ConfirmPasskey(u32),
    Authorize,
    AuthorizeService(String),
    DisplayPinCode(String),
    DisplayPasskey { passkey: u32, entered: u16 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BluetoothPairingRequest {
    pub id: u64,
    pub address: String,
    pub name: String,
    pub kind: BluetoothPairingKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BluetoothSnapshot {
    pub availability: Availability,
    pub powered: bool,
    pub discovering: bool,
    pub devices: Vec<BluetoothDevice>,
    pub busy: Option<String>,
    pub last_error: Option<String>,
    pub pairing: Option<BluetoothPairingRequest>,
}

impl Default for BluetoothSnapshot {
    fn default() -> Self {
        Self {
            availability: Availability::Loading,
            powered: false,
            discovering: false,
            devices: Vec::new(),
            busy: None,
            last_error: None,
            pairing: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaPlayer {
    pub bus_name: String,
    pub identity: String,
    pub playback_status: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art_url: Option<String>,
    pub can_go_previous: bool,
    pub can_play: bool,
    pub can_pause: bool,
    pub can_go_next: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaSnapshot {
    pub availability: Availability,
    pub players: Vec<MediaPlayer>,
    pub last_error: Option<String>,
}

impl Default for MediaSnapshot {
    fn default() -> Self {
        Self {
            availability: Availability::Loading,
            players: Vec::new(),
            last_error: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayItem {
    pub service: String,
    pub path: String,
    pub id: String,
    pub title: String,
    pub status: String,
    pub category: String,
    pub icon_name: Option<String>,
    pub icon_path: Option<String>,
    pub menu_path: Option<String>,
    pub item_is_menu: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayMenuItem {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub separator: bool,
    pub toggle_type: Option<String>,
    pub toggle_state: i32,
    pub icon_name: Option<String>,
    pub children: Vec<TrayMenuItem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayMenu {
    pub service: String,
    pub item_path: String,
    pub menu_path: String,
    pub revision: u32,
    pub items: Vec<TrayMenuItem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraySnapshot {
    pub availability: Availability,
    pub items: Vec<TrayItem>,
    pub active_menu: Option<TrayMenu>,
    pub last_error: Option<String>,
}

impl Default for TraySnapshot {
    fn default() -> Self {
        Self {
            availability: Availability::Loading,
            items: Vec::new(),
            active_menu: None,
            last_error: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrightnessSnapshot {
    pub availability: Availability,
    pub percent: u8,
    pub device: Option<String>,
    pub can_set: bool,
    pub last_error: Option<String>,
}

impl Default for BrightnessSnapshot {
    fn default() -> Self {
        Self {
            availability: Availability::Loading,
            percent: 0,
            device: None,
            can_set: false,
            last_error: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BatterySnapshot {
    pub availability: Availability,
    pub present: bool,
    pub percentage: u8,
    pub state: String,
    pub time_remaining: Option<String>,
    pub energy_rate_watts: Option<f32>,
    pub health_percent: Option<u8>,
    pub warning_level: Option<String>,
    pub power_profiles: Vec<String>,
    pub active_power_profile: Option<String>,
}

impl Default for BatterySnapshot {
    fn default() -> Self {
        Self {
            availability: Availability::Loading,
            present: false,
            percentage: 0,
            state: "unknown".into(),
            time_remaining: None,
            energy_rate_watts: None,
            health_percent: None,
            warning_level: None,
            power_profiles: Vec::new(),
            active_power_profile: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub lock_available: bool,
    pub suspend_available: bool,
    pub restart_available: bool,
    pub power_off_available: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub audio: AudioSnapshot,
    pub network: NetworkSnapshot,
    pub bluetooth: BluetoothSnapshot,
    pub brightness: BrightnessSnapshot,
    pub battery: BatterySnapshot,
    pub session: SessionSnapshot,
    pub media: MediaSnapshot,
    pub tray: TraySnapshot,
}

#[derive(Clone, Debug)]
enum Event {
    Audio(AudioSnapshot),
    Network(NetworkSnapshot),
    Bluetooth(BluetoothSnapshot),
    Brightness(BrightnessSnapshot),
    Battery(BatterySnapshot),
    Session(SessionSnapshot),
    Media(MediaSnapshot),
    Tray(TraySnapshot),
    BluetoothPairing(Option<BluetoothPairingRequest>),
}

#[derive(Clone, Debug)]
pub struct Controller {
    audio: mpsc::Sender<audio::Command>,
    network: mpsc::Sender<network::Command>,
    bluetooth: mpsc::Sender<bluetooth::Command>,
    display: mpsc::Sender<display::Command>,
    power: mpsc::Sender<power::Command>,
    media: mpsc::Sender<media::Command>,
    tray: mpsc::Sender<tray::Command>,
    bluetooth_pairing: bluetooth::PairingController,
}

impl Controller {
    pub fn set_volume(&self, volume: f32) {
        send(
            &self.audio,
            audio::Command::OutputVolume(volume.clamp(0.0, 1.5)),
        );
    }

    pub fn set_muted(&self, muted: bool) {
        send(&self.audio, audio::Command::OutputMuted(muted));
    }

    pub fn set_input_volume(&self, volume: f32) {
        send(
            &self.audio,
            audio::Command::InputVolume(volume.clamp(0.0, 1.0)),
        );
    }

    pub fn set_input_muted(&self, muted: bool) {
        send(&self.audio, audio::Command::InputMuted(muted));
    }

    pub fn set_default_output(&self, id: String) {
        send(&self.audio, audio::Command::DefaultOutput(id));
    }

    pub fn set_default_input(&self, id: String) {
        send(&self.audio, audio::Command::DefaultInput(id));
    }

    pub fn set_stream_volume(&self, id: u32, volume: f32) {
        send(
            &self.audio,
            audio::Command::StreamVolume(id, volume.clamp(0.0, 1.5)),
        );
    }

    pub fn set_stream_muted(&self, id: u32, muted: bool) {
        send(&self.audio, audio::Command::StreamMuted(id, muted));
    }

    pub fn raise_volume(&self) {
        send(&self.audio, audio::Command::RaiseVolume);
    }

    pub fn lower_volume(&self) {
        send(&self.audio, audio::Command::LowerVolume);
    }

    pub fn toggle_mute(&self) {
        send(&self.audio, audio::Command::ToggleMute);
    }

    pub fn raise_input_volume(&self) {
        send(&self.audio, audio::Command::RaiseInputVolume);
    }

    pub fn lower_input_volume(&self) {
        send(&self.audio, audio::Command::LowerInputVolume);
    }

    pub fn toggle_input_mute(&self) {
        send(&self.audio, audio::Command::ToggleInputMute);
    }

    pub fn set_wifi_enabled(&self, enabled: bool) {
        send(&self.network, network::Command::WifiEnabled(enabled));
    }

    pub fn refresh_wifi(&self) {
        send(&self.network, network::Command::Refresh);
    }

    pub fn connect_wifi(&self, ssid: String, password: Option<String>, saved_uuid: Option<String>) {
        send(
            &self.network,
            network::Command::Connect {
                ssid,
                password,
                saved_uuid,
            },
        );
    }

    pub fn disconnect_wifi(&self) {
        send(&self.network, network::Command::Disconnect);
    }

    pub fn forget_wifi(&self, uuid: String) {
        send(&self.network, network::Command::Forget(uuid));
    }

    pub fn set_bluetooth_powered(&self, powered: bool) {
        send(&self.bluetooth, bluetooth::Command::Powered(powered));
    }

    pub fn refresh_bluetooth(&self) {
        send(&self.bluetooth, bluetooth::Command::Refresh);
    }

    pub fn connect_bluetooth(&self, address: String) {
        send(&self.bluetooth, bluetooth::Command::Connect(address));
    }

    pub fn disconnect_bluetooth(&self, address: String) {
        send(&self.bluetooth, bluetooth::Command::Disconnect(address));
    }

    pub fn pair_bluetooth(&self, address: String) {
        send(&self.bluetooth, bluetooth::Command::Pair(address));
    }

    pub fn remove_bluetooth(&self, address: String) {
        send(&self.bluetooth, bluetooth::Command::Remove(address));
    }

    pub fn answer_bluetooth_pairing(&self, id: u64, value: String) {
        self.bluetooth_pairing.answer(id, value);
    }

    pub fn confirm_bluetooth_pairing(&self, id: u64, accepted: bool) {
        self.bluetooth_pairing.confirm(id, accepted);
    }

    pub fn cancel_bluetooth_pairing(&self, id: u64) {
        self.bluetooth_pairing.cancel(id);
    }

    pub fn media_play_pause(&self, bus_name: String) {
        send(&self.media, media::Command::PlayPause(bus_name));
    }

    pub fn media_previous(&self, bus_name: String) {
        send(&self.media, media::Command::Previous(bus_name));
    }

    pub fn media_next(&self, bus_name: String) {
        send(&self.media, media::Command::Next(bus_name));
    }

    pub fn media_raise(&self, bus_name: String) {
        send(&self.media, media::Command::Raise(bus_name));
    }

    pub fn tray_activate(&self, service: String, path: String) {
        send(&self.tray, tray::Command::Activate { service, path });
    }

    pub fn tray_secondary_activate(&self, service: String, path: String) {
        send(
            &self.tray,
            tray::Command::SecondaryActivate { service, path },
        );
    }

    pub fn tray_context_menu(&self, service: String, path: String) {
        send(&self.tray, tray::Command::ContextMenu { service, path });
    }

    pub fn open_tray_menu(&self, item: TrayItem) {
        send(&self.tray, tray::Command::OpenMenu { item });
    }

    pub fn activate_tray_menu_item(&self, service: String, menu_path: String, id: i32) {
        send(
            &self.tray,
            tray::Command::MenuEvent {
                service,
                menu_path,
                id,
            },
        );
    }

    pub fn close_tray_menu(&self) {
        send(&self.tray, tray::Command::CloseMenu);
    }

    pub fn set_brightness(&self, percent: u8) {
        send(
            &self.display,
            display::Command::Brightness(percent.clamp(1, 100)),
        );
    }

    pub fn raise_brightness(&self) {
        send(&self.display, display::Command::RaiseBrightness);
    }

    pub fn lower_brightness(&self) {
        send(&self.display, display::Command::LowerBrightness);
    }

    pub fn set_power_profile(&self, profile: String) {
        send(&self.power, power::Command::PowerProfile(profile));
    }

    pub fn lock(&self) {
        send(&self.power, power::Command::Lock);
    }

    pub fn suspend(&self) {
        send(&self.power, power::Command::Suspend);
    }

    pub fn restart(&self) {
        send(&self.power, power::Command::Restart);
    }

    pub fn power_off(&self) {
        send(&self.power, power::Command::PowerOff);
    }
}

fn send<T>(sender: &mpsc::Sender<T>, command: T) {
    if sender.send(command).is_err() {
        tracing::warn!("system-service worker is not running");
    }
}

pub struct Runtime {
    pub updates: Receiver<Snapshot>,
    pub controller: Controller,
}

pub fn start() -> Runtime {
    let (events, event_rx) = async_channel::unbounded();
    let (updates_tx, updates) = async_channel::unbounded();
    let bluetooth = bluetooth::start(events.clone());
    let controller = Controller {
        audio: audio::start(events.clone()),
        network: network::start(events.clone()),
        bluetooth: bluetooth.commands,
        bluetooth_pairing: bluetooth.pairing,
        display: display::start(events.clone()),
        power: power::start(events.clone()),
        media: media::start(events.clone()),
        tray: tray::start(events),
    };
    std::thread::Builder::new()
        .name("foyer-shell-service-snapshots".into())
        .spawn(move || aggregate(event_rx, updates_tx))
        .expect("failed to start service snapshot worker");
    Runtime {
        updates,
        controller,
    }
}

fn aggregate(events: Receiver<Event>, updates: async_channel::Sender<Snapshot>) {
    let mut snapshot = Snapshot::default();
    while let Ok(event) = events.recv_blocking() {
        match event {
            Event::Audio(value) => snapshot.audio = value,
            Event::Network(value) => snapshot.network = value,
            Event::Bluetooth(mut value) => {
                value.pairing = snapshot.bluetooth.pairing.clone();
                snapshot.bluetooth = value;
            }
            Event::BluetoothPairing(value) => snapshot.bluetooth.pairing = value,
            Event::Brightness(value) => snapshot.brightness = value,
            Event::Battery(value) => snapshot.battery = value,
            Event::Session(value) => snapshot.session = value,
            Event::Media(value) => snapshot.media = value,
            Event::Tray(value) => snapshot.tray = value,
        }
        if updates.send_blocking(snapshot.clone()).is_err() {
            return;
        }
    }
}
