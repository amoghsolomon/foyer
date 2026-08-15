use std::{
    collections::HashMap,
    env,
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use zbus::{
    blocking::{Connection, Proxy},
    fdo::{RequestNameFlags, RequestNameReply},
    message::Header,
    object_server::SignalEmitter,
    zvariant::{DynamicTuple, OwnedObjectPath, OwnedStructure, OwnedValue, Structure},
};

use crate::{Availability, Event, TrayItem, TrayMenu, TrayMenuItem, TraySnapshot};

const WATCHER_NAME: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const WATCHER_INTERFACE: &str = "org.kde.StatusNotifierWatcher";
const ITEM_INTERFACE: &str = "org.kde.StatusNotifierItem";
const MENU_INTERFACE: &str = "com.canonical.dbusmenu";
const DEFAULT_ITEM_PATH: &str = "/StatusNotifierItem";
const REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const MAX_MENU_DEPTH: usize = 8;
const MAX_MENU_ITEMS: usize = 192;

#[derive(Clone, Debug)]
pub(crate) enum Command {
    Activate {
        service: String,
        path: String,
    },
    SecondaryActivate {
        service: String,
        path: String,
    },
    ContextMenu {
        service: String,
        path: String,
    },
    OpenMenu {
        item: TrayItem,
    },
    MenuEvent {
        service: String,
        menu_path: String,
        id: i32,
    },
    CloseMenu,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Registration {
    service: String,
    path: String,
}

#[derive(Clone, Default)]
struct Registry {
    items: Arc<Mutex<Vec<Registration>>>,
}

impl Registry {
    fn register(&self, registration: Registration) -> bool {
        let Ok(mut items) = self.items.lock() else {
            return false;
        };
        if items.contains(&registration) {
            return false;
        }
        items.push(registration);
        true
    }

    fn registrations(&self) -> Vec<Registration> {
        self.items
            .lock()
            .map(|items| items.clone())
            .unwrap_or_default()
    }

    fn registered_names(&self) -> Vec<String> {
        self.registrations()
            .into_iter()
            .map(|item| format!("{}{}", item.service, item.path))
            .collect()
    }

    fn retain(&self, keep: impl Fn(&Registration) -> bool) {
        if let Ok(mut items) = self.items.lock() {
            items.retain(keep)
        }
    }
}

struct Watcher {
    registry: Registry,
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    fn register_status_notifier_item(
        &self,
        service: String,
        #[zbus(header)] header: Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) {
        let sender = header.sender().map(ToString::to_string).unwrap_or_default();
        let Some(registration) = normalize_registration(&service, &sender) else {
            return;
        };
        let registered_name = format!("{}{}", registration.service, registration.path);
        if self.registry.register(registration) {
            let _ = zbus::block_on(Self::status_notifier_item_registered(
                &emitter,
                &registered_name,
            ));
        }
    }

    fn register_status_notifier_host(
        &self,
        _service: String,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) {
        let _ = zbus::block_on(Self::status_notifier_host_registered(&emitter));
    }

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.registry.registered_names()
    }
    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }

    #[zbus(signal)]
    async fn status_notifier_item_registered(
        emitter: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_item_unregistered(
        emitter: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_host_registered(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

pub(crate) fn start(events: async_channel::Sender<Event>) -> mpsc::Sender<Command> {
    let (commands, receiver) = mpsc::channel();
    thread::Builder::new()
        .name("foyer-shell-status-notifier".into())
        .spawn(move || run_worker(events, receiver))
        .expect("failed to start StatusNotifier worker");
    commands
}

fn run_worker(events: async_channel::Sender<Event>, commands: mpsc::Receiver<Command>) {
    let registry = Registry::default();
    let mut snapshot = TraySnapshot::default();
    loop {
        let connection = match connect(registry.clone()) {
            Ok(connection) => connection,
            Err(error) => {
                publish_if_changed(
                    TraySnapshot {
                        availability: Availability::Unavailable(error),
                        ..Default::default()
                    },
                    &mut snapshot,
                    &events,
                );
                match commands.recv_timeout(REFRESH_INTERVAL) {
                    Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
        };
        if run_connected(&connection, &registry, &events, &commands, &mut snapshot) {
            return;
        }
    }
}

fn run_connected(
    connection: &Connection,
    registry: &Registry,
    events: &async_channel::Sender<Event>,
    commands: &mpsc::Receiver<Command>,
    snapshot: &mut TraySnapshot,
) -> bool {
    let mut active_menu = None;
    loop {
        if connection.is_closed() {
            return false;
        }
        let mut next = query(connection, registry);
        if active_menu.as_ref().is_some_and(|menu: &TrayMenu| {
            next.items
                .iter()
                .any(|item| item.service == menu.service && item.path == menu.item_path)
        }) {
            next.active_menu = active_menu.clone();
        } else {
            active_menu = None;
        }
        publish_if_changed(next, snapshot, events);
        match commands.recv_timeout(REFRESH_INTERVAL) {
            Ok(command) => {
                let result = execute(connection, &command, &mut active_menu);
                let mut next = query(connection, registry);
                next.active_menu = active_menu.clone();
                next.last_error = result.err();
                publish_if_changed(next, snapshot, events);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return true,
        }
    }
}

fn publish_if_changed(
    next: TraySnapshot,
    snapshot: &mut TraySnapshot,
    events: &async_channel::Sender<Event>,
) {
    if &next != snapshot {
        *snapshot = next;
        let _ = events.send_blocking(Event::Tray(snapshot.clone()));
    }
}

fn connect(registry: Registry) -> Result<Connection, String> {
    let connection = Connection::session().map_err(|error| error.to_string())?;
    connection
        .object_server()
        .at(WATCHER_PATH, Watcher { registry })
        .map_err(|error| error.to_string())?;
    let reply = connection
        .request_name_with_flags(WATCHER_NAME, RequestNameFlags::DoNotQueue.into())
        .map_err(|error| error.to_string())?;
    {
        let proxy = watcher_proxy(&connection)?;
        let host = connection
            .unique_name()
            .map(ToString::to_string)
            .unwrap_or_else(|| "FoyerShell".into());
        // Registration is required both when Foyer Shell owns the watcher and when another host does.
        proxy
            .call_method("RegisterStatusNotifierHost", &(host))
            .map_err(|error| error.to_string())?;
    }
    if !matches!(
        reply,
        RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner
    ) {
        tracing::info!("using the existing StatusNotifier watcher");
    }
    Ok(connection)
}

fn watcher_proxy(connection: &Connection) -> Result<Proxy<'_>, String> {
    Proxy::new(connection, WATCHER_NAME, WATCHER_PATH, WATCHER_INTERFACE)
        .map_err(|error| error.to_string())
}

fn query(connection: &Connection, registry: &Registry) -> TraySnapshot {
    let registrations = watcher_proxy(connection)
        .and_then(|proxy| {
            proxy
                .get_property::<Vec<String>>("RegisteredStatusNotifierItems")
                .map_err(|error| error.to_string())
        })
        .map(|items| {
            items
                .into_iter()
                .filter_map(|item| normalize_registration(&item, ""))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|_| registry.registrations());
    let queried = registrations
        .iter()
        .filter_map(|registration| {
            query_item(connection, registration)
                .ok()
                .map(|item| (registration.clone(), item))
        })
        .collect::<Vec<_>>();
    registry.retain(|registration| {
        !registrations.contains(registration)
            || queried.iter().any(|(live, _)| live == registration)
    });
    let mut items = queried
        .into_iter()
        .map(|(_, item)| item)
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        status_rank(&left.status)
            .cmp(&status_rank(&right.status))
            .then_with(|| left.title.cmp(&right.title))
    });
    TraySnapshot {
        availability: Availability::Available,
        items,
        active_menu: None,
        last_error: None,
    }
}

fn query_item(connection: &Connection, registration: &Registration) -> Result<TrayItem, String> {
    let proxy = Proxy::new(
        connection,
        registration.service.as_str(),
        registration.path.as_str(),
        ITEM_INTERFACE,
    )
    .map_err(|error| error.to_string())?;
    let icon_name = proxy
        .get_property::<String>("IconName")
        .ok()
        .map(|name| bounded(name, 240))
        .filter(|name| !name.is_empty());
    let icon_theme_path = proxy.get_property::<String>("IconThemePath").ok();
    let menu_path = proxy
        .get_property::<OwnedObjectPath>("Menu")
        .ok()
        .map(|path| path.to_string())
        .filter(|path| path != "/");
    Ok(TrayItem {
        service: registration.service.clone(),
        path: registration.path.clone(),
        id: bounded(proxy.get_property("Id").unwrap_or_default(), 160),
        title: bounded(
            proxy
                .get_property("Title")
                .unwrap_or_else(|_| registration.service.clone()),
            240,
        ),
        status: proxy
            .get_property("Status")
            .unwrap_or_else(|_| "Active".into()),
        category: proxy
            .get_property("Category")
            .unwrap_or_else(|_| "ApplicationStatus".into()),
        icon_path: icon_name
            .as_deref()
            .and_then(|name| resolve_icon(name, icon_theme_path.as_deref()))
            .map(|path| path.to_string_lossy().into_owned()),
        icon_name,
        menu_path,
        item_is_menu: proxy.get_property("ItemIsMenu").unwrap_or(false),
    })
}

fn execute(
    connection: &Connection,
    command: &Command,
    active_menu: &mut Option<TrayMenu>,
) -> Result<(), String> {
    match command {
        Command::Activate { service, path } => call_item(connection, service, path, "Activate"),
        Command::SecondaryActivate { service, path } => {
            call_item(connection, service, path, "SecondaryActivate")
        }
        Command::ContextMenu { service, path } => {
            call_item(connection, service, path, "ContextMenu")
        }
        Command::OpenMenu { item } => {
            *active_menu = item
                .menu_path
                .as_deref()
                .map(|menu_path| load_menu(connection, item, menu_path))
                .transpose()?;
            Ok(())
        }
        Command::MenuEvent {
            service,
            menu_path,
            id,
        } => {
            let proxy = Proxy::new(
                connection,
                service.as_str(),
                menu_path.as_str(),
                MENU_INTERFACE,
            )
            .map_err(|error| error.to_string())?;
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u32;
            proxy
                .call_method(
                    "Event",
                    &(*id, "clicked", OwnedValue::from(0_i32), timestamp),
                )
                .map(|_| ())
                .map_err(|error| error.to_string())?;
            *active_menu = None;
            Ok(())
        }
        Command::CloseMenu => {
            *active_menu = None;
            Ok(())
        }
    }
}

fn call_item(
    connection: &Connection,
    service: &str,
    path: &str,
    method: &str,
) -> Result<(), String> {
    Proxy::new(connection, service, path, ITEM_INTERFACE)
        .map_err(|error| error.to_string())?
        .call_method(method, &(0_i32, 0_i32))
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn load_menu(
    connection: &Connection,
    item: &TrayItem,
    menu_path: &str,
) -> Result<TrayMenu, String> {
    let proxy = Proxy::new(connection, item.service.as_str(), menu_path, MENU_INTERFACE)
        .map_err(|error| error.to_string())?;
    let _ = proxy.call_method("AboutToShow", &(0_i32));
    let message = proxy
        .call_method("GetLayout", &(0_i32, -1_i32, Vec::<String>::new()))
        .map_err(|error| error.to_string())?;
    let DynamicTuple((revision, root)): DynamicTuple<(u32, OwnedStructure)> = message
        .body()
        .deserialize()
        .map_err(|error| error.to_string())?;
    let mut count = 0;
    let (_, _, children): (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>) =
        root.0
            .try_into()
            .map_err(|error: zbus::zvariant::Error| error.to_string())?;
    let items = children
        .into_iter()
        .filter_map(|child| Structure::try_from(child).ok())
        .filter_map(|child| parse_menu_item(child, 1, &mut count))
        .collect();
    Ok(TrayMenu {
        service: item.service.clone(),
        item_path: item.path.clone(),
        menu_path: menu_path.into(),
        revision,
        items,
    })
}

fn parse_menu_item(
    structure: Structure<'static>,
    depth: usize,
    count: &mut usize,
) -> Option<TrayMenuItem> {
    if depth > MAX_MENU_DEPTH || *count >= MAX_MENU_ITEMS {
        return None;
    }
    let (id, properties, children): (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>) =
        structure.try_into().ok()?;
    if property_bool(&properties, "visible").is_some_and(|visible| !visible) {
        return None;
    }
    *count += 1;
    let kind = property_string(&properties, "type").unwrap_or_default();
    let children = children
        .into_iter()
        .filter_map(|child| Structure::try_from(child).ok())
        .filter_map(|child| parse_menu_item(child, depth + 1, count))
        .collect();
    Some(TrayMenuItem {
        id,
        label: clean_menu_label(&property_string(&properties, "label").unwrap_or_default()),
        enabled: property_bool(&properties, "enabled").unwrap_or(true),
        separator: kind == "separator",
        toggle_type: property_string(&properties, "toggle-type").filter(|value| !value.is_empty()),
        toggle_state: property_i32(&properties, "toggle-state").unwrap_or(-1),
        icon_name: property_string(&properties, "icon-name").filter(|value| !value.is_empty()),
        children,
    })
}

fn property_string(properties: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    properties
        .get(key)
        .and_then(|value| <&str>::try_from(value).ok())
        .map(|value| bounded(value.into(), 240))
}
fn property_bool(properties: &HashMap<String, OwnedValue>, key: &str) -> Option<bool> {
    properties
        .get(key)
        .and_then(|value| bool::try_from(value).ok())
}
fn property_i32(properties: &HashMap<String, OwnedValue>, key: &str) -> Option<i32> {
    properties
        .get(key)
        .and_then(|value| i32::try_from(value).ok())
}

fn clean_menu_label(label: &str) -> String {
    let mut result = String::new();
    let mut chars = label.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '_' {
            if chars.peek() == Some(&'_') {
                chars.next();
                result.push('_');
            }
        } else {
            result.push(ch)
        }
    }
    bounded(result, 160)
}

fn resolve_icon(name: &str, extra_root: Option<&str>) -> Option<PathBuf> {
    let direct = PathBuf::from(name);
    if direct.is_absolute() && direct.is_file() {
        return Some(direct);
    }
    let mut roots = Vec::new();
    if let Some(root) = extra_root.filter(|root| !root.is_empty()) {
        roots.push(PathBuf::from(root));
    }
    if let Some(data_home) = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
    {
        roots.push(data_home.join("icons"));
    }
    roots.extend([
        PathBuf::from("/usr/share/icons/hicolor"),
        PathBuf::from("/usr/share/icons/Adwaita"),
        PathBuf::from("/usr/share/pixmaps"),
    ]);
    let extensions = ["svg", "png", "xpm"];
    let directories = [
        "",
        "scalable/status",
        "scalable/apps",
        "64x64/status",
        "64x64/apps",
        "48x48/status",
        "48x48/apps",
        "32x32/status",
        "32x32/apps",
        "24x24/status",
        "24x24/apps",
        "16x16/status",
        "16x16/apps",
    ];
    for root in roots {
        for directory in directories {
            for extension in extensions {
                let candidate = root.join(directory).join(format!("{name}.{extension}"));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

fn normalize_registration(service_or_path: &str, sender: &str) -> Option<Registration> {
    if service_or_path.starts_with('/') {
        return (!sender.is_empty()).then(|| Registration {
            service: sender.to_string(),
            path: service_or_path.to_string(),
        });
    }
    if let Some(index) = service_or_path.find('/') {
        return Some(Registration {
            service: service_or_path[..index].to_string(),
            path: service_or_path[index..].to_string(),
        });
    }
    (!service_or_path.is_empty()).then(|| Registration {
        service: service_or_path.to_string(),
        path: DEFAULT_ITEM_PATH.into(),
    })
}

fn status_rank(status: &str) -> u8 {
    match status {
        "NeedsAttention" => 0,
        "Active" => 1,
        _ => 2,
    }
}
fn bounded(value: String, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_service_and_path_registration_forms() {
        assert_eq!(
            normalize_registration("/CustomItem", ":1.42"),
            Some(Registration {
                service: ":1.42".into(),
                path: "/CustomItem".into()
            })
        );
        assert_eq!(
            normalize_registration("org.example.Item/StatusNotifierItem", ""),
            Some(Registration {
                service: "org.example.Item".into(),
                path: "/StatusNotifierItem".into()
            })
        );
    }

    #[test]
    fn removes_dbus_menu_mnemonics() {
        assert_eq!(clean_menu_label("_Open __ window"), "Open _ window");
    }
}
