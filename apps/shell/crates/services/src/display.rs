use std::{fs, path::Path, sync::mpsc, thread, time::Duration};

use crate::{
    Availability, BrightnessSnapshot, Event,
    process::{executable_in_path, run, short_error},
};

const REFRESH_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug)]
pub(crate) enum Command {
    Brightness(u8),
    RaiseBrightness,
    LowerBrightness,
}

pub(crate) fn start(events: async_channel::Sender<Event>) -> mpsc::Sender<Command> {
    let (commands, receiver) = mpsc::channel();
    thread::Builder::new()
        .name("foyer-shell-display-service".into())
        .spawn(move || run_worker(events, receiver))
        .expect("failed to start display service worker");
    commands
}

fn run_worker(events: async_channel::Sender<Event>, commands: mpsc::Receiver<Command>) {
    let mut snapshot = query();
    publish_if_changed(
        snapshot.clone(),
        &mut BrightnessSnapshot::default(),
        &events,
    );
    loop {
        match commands.recv_timeout(REFRESH_INTERVAL) {
            Ok(mut command) => {
                while let Ok(next) = commands.try_recv() {
                    command = next;
                }
                let error = execute(command, &snapshot).err();
                let mut next = query();
                next.last_error = error;
                publish_if_changed(next, &mut snapshot, &events);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                publish_if_changed(query(), &mut snapshot, &events)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn publish_if_changed(
    next: BrightnessSnapshot,
    snapshot: &mut BrightnessSnapshot,
    events: &async_channel::Sender<Event>,
) {
    if &next != snapshot {
        *snapshot = next;
        let _ = events.send_blocking(Event::Brightness(snapshot.clone()));
    }
}

fn query() -> BrightnessSnapshot {
    let mut devices = match fs::read_dir("/sys/class/backlight") {
        Ok(entries) => entries.filter_map(Result::ok).collect::<Vec<_>>(),
        Err(error) => {
            return BrightnessSnapshot {
                availability: Availability::Unavailable(short_error("backlight", &error)),
                ..Default::default()
            };
        }
    };
    devices.sort_by_key(|entry| entry.file_name());
    let Some(device) = devices.first() else {
        return BrightnessSnapshot {
            availability: Availability::Unavailable("No backlight device".into()),
            ..Default::default()
        };
    };
    let path = device.path();
    let current = read_number(&path.join("actual_brightness"))
        .or_else(|_| read_number(&path.join("brightness")));
    let maximum = read_number(&path.join("max_brightness"));
    let (Ok(current), Ok(maximum)) = (current, maximum) else {
        return BrightnessSnapshot {
            availability: Availability::Unavailable("Could not read backlight values".into()),
            ..Default::default()
        };
    };
    BrightnessSnapshot {
        availability: Availability::Available,
        percent: percentage(current, maximum),
        device: Some(device.file_name().to_string_lossy().into_owned()),
        can_set: executable_in_path("brightnessctl"),
        last_error: None,
    }
}

fn execute(command: Command, snapshot: &BrightnessSnapshot) -> Result<(), String> {
    let Some(device) = snapshot.device.as_deref() else {
        return Err("No backlight device".into());
    };
    let percent = match command {
        Command::Brightness(percent) => percent.clamp(1, 100),
        Command::RaiseBrightness => snapshot.percent.saturating_add(5).min(100),
        Command::LowerBrightness => snapshot.percent.saturating_sub(5).max(1),
    };
    let value = format!("{percent}%");
    run("brightnessctl", &["-q", "-d", device, "set", &value]).map(|_| ())
}

fn read_number(path: &Path) -> Result<u64, String> {
    fs::read_to_string(path)
        .map_err(|error| short_error(&path.display().to_string(), &error))?
        .trim()
        .parse::<u64>()
        .map_err(|_| format!("invalid number in {}", path.display()))
}

fn percentage(current: u64, maximum: u64) -> u8 {
    if maximum == 0 {
        return 0;
    }
    ((current.saturating_mul(100) + maximum / 2) / maximum).min(100) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculates_brightness_percentage_safely() {
        assert_eq!(percentage(12_000, 24_000), 50);
        assert_eq!(percentage(24_000, 24_000), 100);
        assert_eq!(percentage(1, 0), 0);
    }
}
