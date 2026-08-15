use std::{env, ffi::OsString, path::PathBuf, sync::mpsc, thread, time::Duration};

use crate::{
    Availability, BatterySnapshot, Event, SessionSnapshot,
    process::{executable_in_path, run, spawn_detached},
};

const REFRESH_INTERVAL: Duration = Duration::from_secs(20);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LockBackend {
    Gtklock,
    Swaylock,
}

#[derive(Clone, Debug)]
pub(crate) enum Command {
    PowerProfile(String),
    Lock,
    Suspend,
    Restart,
    PowerOff,
}

pub(crate) fn start(events: async_channel::Sender<Event>) -> mpsc::Sender<Command> {
    let (commands, receiver) = mpsc::channel();
    thread::Builder::new()
        .name("foyer-shell-power-service".into())
        .spawn(move || run_worker(events, receiver))
        .expect("failed to start power service worker");
    commands
}

fn run_worker(events: async_channel::Sender<Event>, commands: mpsc::Receiver<Command>) {
    let mut battery = BatterySnapshot::default();
    let mut session = SessionSnapshot::default();
    loop {
        publish_battery(query_battery(), &mut battery, &events);
        publish_session(query_session(), &mut session, &events);
        match commands.recv_timeout(REFRESH_INTERVAL) {
            Ok(command) => {
                if let Err(error) = execute(command) {
                    tracing::warn!(%error, "power service command failed");
                }
                publish_battery(query_battery(), &mut battery, &events);
                publish_session(query_session(), &mut session, &events);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn publish_battery(
    next: BatterySnapshot,
    snapshot: &mut BatterySnapshot,
    events: &async_channel::Sender<Event>,
) {
    if &next != snapshot {
        *snapshot = next;
        let _ = events.send_blocking(Event::Battery(snapshot.clone()));
    }
}

fn publish_session(
    next: SessionSnapshot,
    snapshot: &mut SessionSnapshot,
    events: &async_channel::Sender<Event>,
) {
    if &next != snapshot {
        *snapshot = next;
        let _ = events.send_blocking(Event::Session(snapshot.clone()));
    }
}

fn query_session() -> SessionSnapshot {
    let has_systemctl = executable_in_path("systemctl");
    SessionSnapshot {
        lock_available: installed_lock_backend().is_some(),
        suspend_available: has_systemctl,
        restart_available: has_systemctl,
        power_off_available: has_systemctl,
    }
}

fn query_battery() -> BatterySnapshot {
    if !executable_in_path("upower") {
        return BatterySnapshot {
            availability: Availability::Unavailable("upower is not installed".into()),
            ..Default::default()
        };
    }
    let output = match run(
        "upower",
        &["-i", "/org/freedesktop/UPower/devices/DisplayDevice"],
    ) {
        Ok(output) => output,
        Err(error) => {
            return BatterySnapshot {
                availability: Availability::Unavailable(error),
                ..Default::default()
            };
        }
    };
    let present = property(&output, "present").is_none_or(|value| value == "yes");
    if !present || !output.contains("battery") {
        return BatterySnapshot {
            availability: Availability::Available,
            present: false,
            ..Default::default()
        };
    }
    let percentage = percent_property(&output, "percentage").unwrap_or(0);
    let state = property(&output, "state").unwrap_or_else(|| "unknown".into());
    let time_remaining = match state.as_str() {
        "charging" => property(&output, "time to full"),
        "discharging" => property(&output, "time to empty"),
        _ => None,
    };
    let energy_rate_watts = property(&output, "energy-rate")
        .and_then(|value| value.split_whitespace().next()?.parse::<f32>().ok());
    let health_percent = battery_health();
    let (power_profiles, active_power_profile) = power_profiles();
    BatterySnapshot {
        availability: Availability::Available,
        present: true,
        percentage,
        state,
        time_remaining,
        energy_rate_watts,
        health_percent,
        warning_level: property(&output, "warning-level"),
        power_profiles,
        active_power_profile,
    }
}

fn battery_health() -> Option<u8> {
    let devices = run("upower", &["-e"]).ok()?;
    let path = devices.lines().find(|path| path.contains("/battery_"))?;
    let output = run("upower", &["-i", path]).ok()?;
    percent_property(&output, "capacity")
}

fn power_profiles() -> (Vec<String>, Option<String>) {
    if !executable_in_path("powerprofilesctl") {
        return (Vec::new(), None);
    }
    let active = run("powerprofilesctl", &["get"]).ok();
    let output = run("powerprofilesctl", &["list"]).unwrap_or_default();
    let profiles = ["power-saver", "balanced", "performance"]
        .into_iter()
        .filter(|profile| output.contains(&format!("{profile}:")))
        .map(str::to_string)
        .collect();
    (profiles, active)
}

fn property(output: &str, name: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let (key, value) = line.trim().split_once(':')?;
        (key == name).then(|| value.trim().trim_matches('\'').to_string())
    })
}

fn percent_property(output: &str, name: &str) -> Option<u8> {
    property(output, name)?
        .strip_suffix('%')?
        .parse::<f32>()
        .ok()
        .map(|value| value.round().clamp(0.0, 100.0) as u8)
}

fn execute(command: Command) -> Result<(), String> {
    match command {
        Command::PowerProfile(profile) => run("powerprofilesctl", &["set", &profile]).map(|_| ()),
        Command::Lock => match installed_lock_backend() {
            Some(LockBackend::Gtklock) => spawn_detached("gtklock", gtklock_arguments()),
            Some(LockBackend::Swaylock) => run("swaylock", &["--daemonize"]).map(|_| ()),
            None => Err("no supported session locker is installed".into()),
        },
        Command::Suspend => run("systemctl", &["suspend"]).map(|_| ()),
        Command::Restart => run("systemctl", &["reboot"]).map(|_| ()),
        Command::PowerOff => run("systemctl", &["poweroff"]).map(|_| ()),
    }
}

fn installed_lock_backend() -> Option<LockBackend> {
    choose_lock_backend(
        executable_in_path("gtklock") && gtklock_layout_path().is_file(),
        executable_in_path("swaylock"),
    )
}

fn choose_lock_backend(has_gtklock: bool, has_swaylock: bool) -> Option<LockBackend> {
    if has_gtklock {
        Some(LockBackend::Gtklock)
    } else if has_swaylock {
        Some(LockBackend::Swaylock)
    } else {
        None
    }
}

fn gtklock_arguments() -> Vec<OsString> {
    let directory = gtklock_config_directory();
    vec![
        "--daemonize".into(),
        "--config".into(),
        directory.join("config.ini").into_os_string(),
        "--layout".into(),
        directory.join("layout.ui").into_os_string(),
        "--style".into(),
        directory.join("style.css").into_os_string(),
    ]
}

fn gtklock_layout_path() -> PathBuf {
    gtklock_config_directory().join("layout.ui")
}

fn gtklock_config_directory() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(env::temp_dir)
        .join("gtklock")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_upower_values() {
        let output = "  state: discharging\n  percentage: 73%\n  energy-rate: 8.2 W\n  time to empty: 3.1 hours";
        assert_eq!(property(output, "state").as_deref(), Some("discharging"));
        assert_eq!(percent_property(output, "percentage"), Some(73));
        assert_eq!(
            property(output, "time to empty").as_deref(),
            Some("3.1 hours")
        );
    }

    #[test]
    fn prefers_gtklock_and_keeps_swaylock_as_recovery() {
        assert_eq!(choose_lock_backend(true, true), Some(LockBackend::Gtklock));
        assert_eq!(
            choose_lock_backend(false, true),
            Some(LockBackend::Swaylock)
        );
        assert_eq!(choose_lock_backend(false, false), None);
    }
}
