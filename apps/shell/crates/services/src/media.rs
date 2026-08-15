use std::{collections::HashMap, sync::mpsc, thread, time::Duration};

use zbus::{
    blocking::{Connection, Proxy, fdo::DBusProxy},
    zvariant::OwnedValue,
};

use crate::{Availability, Event, MediaPlayer, MediaSnapshot};

const REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
const OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";

#[derive(Clone, Debug)]
pub(crate) enum Command {
    PlayPause(String),
    Previous(String),
    Next(String),
    Raise(String),
}

pub(crate) fn start(events: async_channel::Sender<Event>) -> mpsc::Sender<Command> {
    let (commands, receiver) = mpsc::channel();
    thread::Builder::new()
        .name("foyer-shell-media-service".into())
        .spawn(move || run_worker(events, receiver))
        .expect("failed to start MPRIS service worker");
    commands
}

fn run_worker(events: async_channel::Sender<Event>, commands: mpsc::Receiver<Command>) {
    let mut snapshot = MediaSnapshot::default();
    loop {
        publish_if_changed(query(), &mut snapshot, &events);
        match commands.recv_timeout(REFRESH_INTERVAL) {
            Ok(command) => {
                let error = execute(&command).err();
                let mut next = query();
                next.last_error = error;
                publish_if_changed(next, &mut snapshot, &events);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn publish_if_changed(
    next: MediaSnapshot,
    snapshot: &mut MediaSnapshot,
    events: &async_channel::Sender<Event>,
) {
    if &next != snapshot {
        *snapshot = next;
        let _ = events.send_blocking(Event::Media(snapshot.clone()));
    }
}

fn query() -> MediaSnapshot {
    let connection = match Connection::session() {
        Ok(connection) => connection,
        Err(error) => return unavailable(error.to_string()),
    };
    let bus = match DBusProxy::new(&connection) {
        Ok(bus) => bus,
        Err(error) => return unavailable(error.to_string()),
    };
    let names = match bus.list_names() {
        Ok(names) => names,
        Err(error) => return unavailable(error.to_string()),
    };
    let mut players = names
        .into_iter()
        .map(|name| name.to_string())
        .filter(|name| name.starts_with(MPRIS_PREFIX))
        .filter_map(|name| query_player(&connection, &name).ok())
        .collect::<Vec<_>>();
    players.sort_by(|left, right| {
        playback_rank(&left.playback_status)
            .cmp(&playback_rank(&right.playback_status))
            .then_with(|| left.identity.cmp(&right.identity))
    });
    MediaSnapshot {
        availability: Availability::Available,
        players,
        last_error: None,
    }
}

fn unavailable(error: String) -> MediaSnapshot {
    MediaSnapshot {
        availability: Availability::Unavailable(error),
        ..Default::default()
    }
}

fn query_player(connection: &Connection, bus_name: &str) -> Result<MediaPlayer, String> {
    let root = Proxy::new(connection, bus_name, OBJECT_PATH, "org.mpris.MediaPlayer2")
        .map_err(|error| error.to_string())?;
    let player = Proxy::new(
        connection,
        bus_name,
        OBJECT_PATH,
        "org.mpris.MediaPlayer2.Player",
    )
    .map_err(|error| error.to_string())?;
    let metadata = player
        .get_property::<HashMap<String, OwnedValue>>("Metadata")
        .unwrap_or_default();
    Ok(MediaPlayer {
        bus_name: bus_name.to_string(),
        identity: bounded(
            root.get_property("Identity")
                .unwrap_or_else(|_| player_name(bus_name)),
            120,
        ),
        playback_status: player
            .get_property("PlaybackStatus")
            .unwrap_or_else(|_| "Stopped".into()),
        title: metadata_string(&metadata, "xesam:title", 240)
            .unwrap_or_else(|| "Nothing playing".into()),
        artist: metadata_artists(&metadata)
            .map(|value| bounded(value, 240))
            .unwrap_or_default(),
        album: metadata_string(&metadata, "xesam:album", 240).unwrap_or_default(),
        art_url: metadata_string(&metadata, "mpris:artUrl", 2_048).filter(|url| {
            url.starts_with("file://") || url.starts_with("http://") || url.starts_with("https://")
        }),
        can_go_previous: player.get_property("CanGoPrevious").unwrap_or(false),
        can_play: player.get_property("CanPlay").unwrap_or(false),
        can_pause: player.get_property("CanPause").unwrap_or(false),
        can_go_next: player.get_property("CanGoNext").unwrap_or(false),
    })
}

fn execute(command: &Command) -> Result<(), String> {
    let connection = Connection::session().map_err(|error| error.to_string())?;
    let (bus_name, interface, method) = match command {
        Command::PlayPause(bus_name) => (bus_name, "org.mpris.MediaPlayer2.Player", "PlayPause"),
        Command::Previous(bus_name) => (bus_name, "org.mpris.MediaPlayer2.Player", "Previous"),
        Command::Next(bus_name) => (bus_name, "org.mpris.MediaPlayer2.Player", "Next"),
        Command::Raise(bus_name) => (bus_name, "org.mpris.MediaPlayer2", "Raise"),
    };
    Proxy::new(&connection, bus_name.as_str(), OBJECT_PATH, interface)
        .map_err(|error| error.to_string())?
        .call_method(method, &())
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn metadata_string(
    metadata: &HashMap<String, OwnedValue>,
    key: &str,
    max_chars: usize,
) -> Option<String> {
    metadata
        .get(key)
        .and_then(|value| value.try_clone().ok())
        .and_then(|value| String::try_from(value).ok())
        .map(|value| bounded(value, max_chars))
        .filter(|value| !value.is_empty())
}

fn metadata_artists(metadata: &HashMap<String, OwnedValue>) -> Option<String> {
    metadata
        .get("xesam:artist")
        .and_then(|value| value.try_clone().ok())
        .and_then(|value| Vec::<String>::try_from(value).ok())
        .map(|artists| artists.join(", "))
        .filter(|value| !value.is_empty())
}

fn player_name(bus_name: &str) -> String {
    bus_name
        .strip_prefix(MPRIS_PREFIX)
        .unwrap_or(bus_name)
        .split('.')
        .next()
        .unwrap_or("Media player")
        .to_string()
}

fn playback_rank(status: &str) -> u8 {
    match status {
        "Playing" => 0,
        "Paused" => 1,
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
    fn derives_stable_player_names_and_playback_order() {
        assert_eq!(player_name("org.mpris.MediaPlayer2.vlc.instance42"), "vlc");
        assert!(playback_rank("Playing") < playback_rank("Paused"));
        assert!(playback_rank("Paused") < playback_rank("Stopped"));
    }
}
