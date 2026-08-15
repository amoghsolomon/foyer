use std::{sync::mpsc, thread, time::Duration};

use serde_json::Value;

use crate::{
    AudioDevice, AudioSnapshot, AudioStream, Availability, Event, RecordingApp,
    events::start_process_line_monitor, process::run,
};

const EVENT_SETTLE: Duration = Duration::from_millis(75);

#[derive(Clone, Debug)]
pub(crate) enum Command {
    OutputVolume(f32),
    OutputMuted(bool),
    InputVolume(f32),
    InputMuted(bool),
    DefaultOutput(String),
    DefaultInput(String),
    StreamVolume(u32, f32),
    StreamMuted(u32, bool),
    RaiseVolume,
    LowerVolume,
    ToggleMute,
    RaiseInputVolume,
    LowerInputVolume,
    ToggleInputMute,
    Refresh,
}

pub(crate) fn start(events: async_channel::Sender<Event>) -> mpsc::Sender<Command> {
    let (commands, receiver) = mpsc::channel();
    start_process_line_monitor(
        "foyer-shell-audio-events",
        "pactl",
        &["subscribe"],
        commands.clone(),
        Command::Refresh,
        audio_event_requires_refresh,
    );
    thread::Builder::new()
        .name("foyer-shell-audio-service".into())
        .spawn(move || run_worker(events, receiver))
        .expect("failed to start audio service worker");
    commands
}

fn run_worker(events: async_channel::Sender<Event>, commands: mpsc::Receiver<Command>) {
    let mut snapshot = query();
    publish_if_changed(snapshot.clone(), &mut AudioSnapshot::default(), &events);
    loop {
        match commands.recv() {
            Ok(command) => {
                let commands = coalesce_pending(command, &commands);
                let mut error = None;
                for command in &commands {
                    if let Err(command_error) = execute(command, &snapshot) {
                        error = Some(command_error);
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
    if matches!(first, Command::Refresh) {
        thread::sleep(EVENT_SETTLE);
    }
    let mut pending = vec![first];
    while let Ok(next) = receiver.try_recv() {
        if pending
            .last()
            .is_some_and(|previous| same_replaceable_target(previous, &next))
        {
            *pending.last_mut().expect("pending is not empty") = next;
        } else {
            pending.push(next);
        }
    }
    pending
}

fn same_replaceable_target(previous: &Command, next: &Command) -> bool {
    matches!(
        (previous, next),
        (Command::OutputVolume(_), Command::OutputVolume(_))
            | (Command::OutputMuted(_), Command::OutputMuted(_))
            | (Command::InputVolume(_), Command::InputVolume(_))
            | (Command::InputMuted(_), Command::InputMuted(_))
            | (Command::DefaultOutput(_), Command::DefaultOutput(_))
            | (Command::DefaultInput(_), Command::DefaultInput(_))
            | (Command::Refresh, Command::Refresh)
    ) || matches!(
        (previous, next),
        (Command::StreamVolume(previous_id, _), Command::StreamVolume(next_id, _))
            | (Command::StreamMuted(previous_id, _), Command::StreamMuted(next_id, _))
            if previous_id == next_id
    )
}

fn publish_if_changed(
    next: AudioSnapshot,
    snapshot: &mut AudioSnapshot,
    events: &async_channel::Sender<Event>,
) {
    if &next != snapshot {
        *snapshot = next;
        let _ = events.send_blocking(Event::Audio(snapshot.clone()));
    }
}

fn query() -> AudioSnapshot {
    let default_output = match run("pactl", &["get-default-sink"]) {
        Ok(value) => value,
        Err(error) => return unavailable(error),
    };
    let default_input = match run("pactl", &["get-default-source"]) {
        Ok(value) => value,
        Err(error) => return unavailable(error),
    };
    let outputs = match query_devices("sinks", &default_output, false) {
        Ok(devices) => devices,
        Err(error) => return unavailable(error),
    };
    let inputs = match query_devices("sources", &default_input, true) {
        Ok(devices) => devices,
        Err(error) => return unavailable(error),
    };
    let streams = query_streams().unwrap_or_default();
    let recording_apps = query_recording_apps().unwrap_or_default();
    let output = outputs.iter().find(|device| device.is_default);
    let input = inputs.iter().find(|device| device.is_default);

    AudioSnapshot {
        availability: Availability::Available,
        volume: output.map_or(0.0, |device| device.volume),
        muted: output.is_some_and(|device| device.muted),
        device: output
            .map(|device| device.description.clone())
            .unwrap_or_else(|| "No output device".into()),
        outputs,
        input_volume: input.map_or(0.0, |device| device.volume),
        input_muted: input.is_some_and(|device| device.muted),
        input_device: input
            .map(|device| device.description.clone())
            .unwrap_or_else(|| "No input device".into()),
        inputs,
        streams,
        recording_apps,
        last_error: None,
    }
}

fn unavailable(error: String) -> AudioSnapshot {
    AudioSnapshot {
        availability: Availability::Unavailable(error),
        ..Default::default()
    }
}

fn query_devices(
    kind: &str,
    default_name: &str,
    omit_monitors: bool,
) -> Result<Vec<AudioDevice>, String> {
    let output = run("pactl", &["-f", "json", "list", kind])?;
    let values = serde_json::from_str::<Vec<Value>>(&output)
        .map_err(|error| format!("pactl {kind}: invalid JSON: {error}"))?;
    let mut devices = values
        .into_iter()
        .filter_map(|value| {
            let id = value.get("name")?.as_str()?.to_string();
            if omit_monitors && id.ends_with(".monitor") {
                return None;
            }
            let description = value
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or(&id)
                .to_string();
            Some(AudioDevice {
                id: id.clone(),
                description,
                volume: volume(&value).unwrap_or(0.0),
                muted: value.get("mute").and_then(Value::as_bool).unwrap_or(false),
                is_default: id == default_name,
            })
        })
        .collect::<Vec<_>>();
    devices.sort_by(|left, right| {
        right
            .is_default
            .cmp(&left.is_default)
            .then_with(|| left.description.cmp(&right.description))
    });
    Ok(devices)
}

fn query_streams() -> Result<Vec<AudioStream>, String> {
    let output = run("pactl", &["-f", "json", "list", "sink-inputs"])?;
    let values = serde_json::from_str::<Vec<Value>>(&output)
        .map_err(|error| format!("pactl sink-inputs: invalid JSON: {error}"))?;
    let mut streams = values
        .into_iter()
        .filter_map(|value| {
            let id = value.get("index")?.as_u64()?.try_into().ok()?;
            Some(AudioStream {
                id,
                name: application_label(&value),
                volume: volume(&value).unwrap_or(0.0),
                muted: value.get("mute").and_then(Value::as_bool).unwrap_or(false),
            })
        })
        .collect::<Vec<_>>();
    streams.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(streams)
}

fn query_recording_apps() -> Result<Vec<RecordingApp>, String> {
    let output = run("pactl", &["-f", "json", "list", "source-outputs"])?;
    let values = serde_json::from_str::<Vec<Value>>(&output)
        .map_err(|error| format!("pactl source-outputs: invalid JSON: {error}"))?;
    let mut apps = values
        .into_iter()
        .filter_map(|value| {
            let id = value.get("index")?.as_u64()?.try_into().ok()?;
            Some(RecordingApp {
                id,
                name: application_label(&value),
            })
        })
        .collect::<Vec<_>>();
    apps.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(apps)
}

fn application_label(value: &Value) -> String {
    let properties = value.get("properties");
    [
        "application.name",
        "media.name",
        "application.process.binary",
    ]
    .into_iter()
    .find_map(|key| properties?.get(key)?.as_str())
    .unwrap_or("Application audio")
    .to_string()
}

fn volume(value: &Value) -> Option<f32> {
    let channel = value.get("volume")?.as_object()?.values().next()?;
    let percent = channel.get("value_percent")?.as_str()?.trim();
    percent
        .strip_suffix('%')?
        .parse::<f32>()
        .ok()
        .map(|value| (value / 100.0).clamp(0.0, 1.5))
}

fn execute(command: &Command, snapshot: &AudioSnapshot) -> Result<(), String> {
    match command {
        Command::OutputVolume(volume) => set_percent("set-sink-volume", "@DEFAULT_SINK@", *volume),
        Command::OutputMuted(muted) => set_mute("set-sink-mute", "@DEFAULT_SINK@", *muted),
        Command::InputVolume(volume) => set_percent(
            "set-source-volume",
            "@DEFAULT_SOURCE@",
            volume.clamp(0.0, 1.0),
        ),
        Command::InputMuted(muted) => set_mute("set-source-mute", "@DEFAULT_SOURCE@", *muted),
        Command::DefaultOutput(id) => {
            run("pactl", &["set-default-sink", id])?;
            for stream in &snapshot.streams {
                let stream_id = stream.id.to_string();
                if let Err(error) = run("pactl", &["move-sink-input", &stream_id, id]) {
                    tracing::warn!(%error, stream_id = stream.id, "could not move active audio stream");
                }
            }
            Ok(())
        }
        Command::DefaultInput(id) => {
            run("pactl", &["set-default-source", id])?;
            for app in &snapshot.recording_apps {
                let stream_id = app.id.to_string();
                if let Err(error) = run("pactl", &["move-source-output", &stream_id, id]) {
                    tracing::warn!(%error, stream_id = app.id, "could not move active capture stream");
                }
            }
            Ok(())
        }
        Command::StreamVolume(id, volume) => {
            set_percent("set-sink-input-volume", &id.to_string(), *volume)
        }
        Command::StreamMuted(id, muted) => set_mute("set-sink-input-mute", &id.to_string(), *muted),
        Command::RaiseVolume => {
            run("pactl", &["set-sink-volume", "@DEFAULT_SINK@", "+5%"]).map(|_| ())
        }
        Command::LowerVolume => {
            run("pactl", &["set-sink-volume", "@DEFAULT_SINK@", "-5%"]).map(|_| ())
        }
        Command::ToggleMute => {
            run("pactl", &["set-sink-mute", "@DEFAULT_SINK@", "toggle"]).map(|_| ())
        }
        Command::RaiseInputVolume => {
            run("pactl", &["set-source-volume", "@DEFAULT_SOURCE@", "+5%"]).map(|_| ())
        }
        Command::LowerInputVolume => {
            run("pactl", &["set-source-volume", "@DEFAULT_SOURCE@", "-5%"]).map(|_| ())
        }
        Command::ToggleInputMute => {
            run("pactl", &["set-source-mute", "@DEFAULT_SOURCE@", "toggle"]).map(|_| ())
        }
        Command::Refresh => Ok(()),
    }
}

fn audio_event_requires_refresh(line: &str) -> bool {
    [
        " on sink ",
        " on source ",
        " on sink-input ",
        " on source-output ",
        " on server ",
        " on card ",
    ]
    .iter()
    .any(|kind| line.contains(kind))
}

fn set_percent(command: &str, id: &str, volume: f32) -> Result<(), String> {
    let percent = format!("{}%", (volume.clamp(0.0, 1.5) * 100.0).round() as u16);
    run("pactl", &[command, id, &percent]).map(|_| ())
}

fn set_mute(command: &str, id: &str, muted: bool) -> Result<(), String> {
    run("pactl", &[command, id, if muted { "1" } else { "0" }]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pactl_volume_and_application_name() {
        let value: Value = serde_json::from_str(
            r#"{
                "index": 12,
                "mute": false,
                "volume": {"front-left": {"value_percent": "73%"}},
                "properties": {"application.name": "Music"}
            }"#,
        )
        .unwrap();
        assert_eq!(volume(&value), Some(0.73));
        assert_eq!(application_label(&value), "Music");
    }

    #[test]
    fn repeated_absolute_volume_updates_replace_one_another() {
        assert!(same_replaceable_target(
            &Command::OutputVolume(0.4),
            &Command::OutputVolume(0.7),
        ));
        assert!(!same_replaceable_target(
            &Command::OutputVolume(0.4),
            &Command::InputVolume(0.7),
        ));
    }

    #[test]
    fn ignores_pactl_client_churn_from_snapshot_queries() {
        assert!(!audio_event_requires_refresh("Event 'new' on client #42"));
        assert!(audio_event_requires_refresh("Event 'change' on sink #1"));
        assert!(audio_event_requires_refresh(
            "Event 'new' on source-output #9"
        ));
    }
}
