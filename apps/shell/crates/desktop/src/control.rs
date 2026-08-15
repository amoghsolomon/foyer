use std::{
    env, fs,
    io::{self, Write as _},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
    thread,
};

use anyhow::{Context as _, Result, anyhow};
use async_channel::Receiver;

#[derive(Clone, Copy, Debug)]
pub enum Command {
    Search,
    Agenda,
    Tasks,
    Notes,
    Contacts,
    Bookmarks,
    Notifications,
    Transcription,
    Audio,
    Network,
    Bluetooth,
    Display,
    Tray,
    Power,
    Lock,
    RaiseVolume,
    LowerVolume,
    ToggleMute,
    RaiseInputVolume,
    LowerInputVolume,
    ToggleInputMute,
    RaiseBrightness,
    LowerBrightness,
}

pub fn handle_client_invocation() -> Result<bool> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments.is_empty() {
        return Ok(false);
    }
    let command = invocation_line(&arguments)?;

    let mut stream = UnixStream::connect(socket_path())
        .context("connect to the running foyer-shell instance")?;
    stream
        .write_all(command.as_bytes())
        .context("send foyer-shell command")?;
    Ok(true)
}

fn invocation_line(arguments: &[String]) -> Result<&'static str> {
    match arguments {
        [surface, operation] if surface == "search" && operation == "toggle" => {
            Ok("search toggle\n")
        }
        [surface, operation] if surface == "notifications" && operation == "toggle" => {
            Ok("notifications toggle\n")
        }
        [surface, operation] if surface == "transcription" && operation == "toggle" => {
            Ok("transcription toggle\n")
        }
        [surface, operation] if surface == "agenda" && operation == "toggle" => {
            Ok("agenda toggle\n")
        }
        [surface, operation] if surface == "tasks" && operation == "toggle" => Ok("tasks toggle\n"),
        [surface, operation] if surface == "notes" && operation == "toggle" => Ok("notes toggle\n"),
        [surface, operation] if surface == "contacts" && operation == "toggle" => {
            Ok("contacts toggle\n")
        }
        [surface, operation] if surface == "bookmarks" && operation == "toggle" => {
            Ok("bookmarks toggle\n")
        }
        [surface, operation] if surface == "audio" && operation == "toggle" => Ok("audio toggle\n"),
        [surface, operation] if surface == "network" && operation == "toggle" => {
            Ok("network toggle\n")
        }
        [surface, operation] if surface == "bluetooth" && operation == "toggle" => {
            Ok("bluetooth toggle\n")
        }
        [surface, operation] if surface == "display" && operation == "toggle" => {
            Ok("display toggle\n")
        }
        [surface, operation] if surface == "tray" && operation == "toggle" => Ok("tray toggle\n"),
        [surface, operation] if surface == "power" && operation == "toggle" => Ok("power toggle\n"),
        [surface, operation] if surface == "session" && operation == "lock" => Ok("session lock\n"),
        [service, operation] if service == "volume" && operation == "raise" => Ok("volume raise\n"),
        [service, operation] if service == "volume" && operation == "lower" => Ok("volume lower\n"),
        [service, operation] if service == "volume" && operation == "toggle-mute" => {
            Ok("volume toggle-mute\n")
        }
        [service, operation] if service == "microphone" && operation == "raise" => {
            Ok("microphone raise\n")
        }
        [service, operation] if service == "microphone" && operation == "lower" => {
            Ok("microphone lower\n")
        }
        [service, operation] if service == "microphone" && operation == "toggle-mute" => {
            Ok("microphone toggle-mute\n")
        }
        [service, operation] if service == "brightness" && operation == "raise" => {
            Ok("brightness raise\n")
        }
        [service, operation] if service == "brightness" && operation == "lower" => {
            Ok("brightness lower\n")
        }
        _ => Err(anyhow!(
            "unknown command; expected a surface or transcription toggle, session lock, volume or microphone <raise|lower|toggle-mute>, or brightness <raise|lower>"
        )),
    }
}

pub fn listen() -> Result<Receiver<Command>> {
    let path = socket_path();
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
            if UnixStream::connect(&path).is_ok() {
                return Err(anyhow!("foyer-shell is already running"));
            }
            fs::remove_file(&path).with_context(|| {
                format!(
                    "remove stale Foyer Shell control socket at {}",
                    path.display()
                )
            })?;
            UnixListener::bind(&path)
                .with_context(|| format!("bind Foyer Shell control socket at {}", path.display()))?
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("bind Foyer Shell control socket at {}", path.display()));
        }
    };

    let (sender, receiver) = async_channel::unbounded();
    thread::Builder::new()
        .name("foyer-shell-control".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    continue;
                };
                let mut line = String::new();
                if io::BufRead::read_line(&mut io::BufReader::new(stream), &mut line).is_err() {
                    continue;
                }
                if line.trim().is_empty() {
                    continue;
                }
                let command = match parse_line(line.trim()) {
                    Some(command) => Some(command),
                    None => {
                        let unknown = line.trim();
                        tracing::warn!(%unknown, "ignored unknown Foyer Shell control command");
                        None
                    }
                };
                if let Some(command) = command
                    && sender.send_blocking(command).is_err()
                {
                    break;
                }
            }
            let _ = fs::remove_file(path);
        })
        .context("start Foyer Shell control listener")?;
    Ok(receiver)
}

fn parse_line(line: &str) -> Option<Command> {
    match line {
        "search toggle" => Some(Command::Search),
        "agenda toggle" => Some(Command::Agenda),
        "tasks toggle" => Some(Command::Tasks),
        "notes toggle" => Some(Command::Notes),
        "contacts toggle" => Some(Command::Contacts),
        "bookmarks toggle" => Some(Command::Bookmarks),
        "notifications toggle" => Some(Command::Notifications),
        "transcription toggle" => Some(Command::Transcription),
        "audio toggle" => Some(Command::Audio),
        "network toggle" => Some(Command::Network),
        "bluetooth toggle" => Some(Command::Bluetooth),
        "display toggle" => Some(Command::Display),
        "tray toggle" => Some(Command::Tray),
        "power toggle" => Some(Command::Power),
        "session lock" => Some(Command::Lock),
        "volume raise" => Some(Command::RaiseVolume),
        "volume lower" => Some(Command::LowerVolume),
        "volume toggle-mute" => Some(Command::ToggleMute),
        "microphone raise" => Some(Command::RaiseInputVolume),
        "microphone lower" => Some(Command::LowerInputVolume),
        "microphone toggle-mute" => Some(Command::ToggleInputMute),
        "brightness raise" => Some(Command::RaiseBrightness),
        "brightness lower" => Some(Command::LowerBrightness),
        _ => None,
    }
}

fn socket_path() -> PathBuf {
    env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
        .join("foyer-shell.sock")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn accepts_only_fixed_semantic_client_commands() {
        assert_eq!(
            invocation_line(&arguments(&["volume", "raise"])).unwrap(),
            "volume raise\n"
        );
        assert_eq!(
            invocation_line(&arguments(&["power", "toggle"])).unwrap(),
            "power toggle\n"
        );
        assert_eq!(
            invocation_line(&arguments(&["notifications", "toggle"])).unwrap(),
            "notifications toggle\n"
        );
        assert_eq!(
            invocation_line(&arguments(&["transcription", "toggle"])).unwrap(),
            "transcription toggle\n"
        );
        assert_eq!(
            invocation_line(&arguments(&["agenda", "toggle"])).unwrap(),
            "agenda toggle\n"
        );
        assert_eq!(
            invocation_line(&arguments(&["tasks", "toggle"])).unwrap(),
            "tasks toggle\n"
        );
        assert_eq!(
            invocation_line(&arguments(&["notes", "toggle"])).unwrap(),
            "notes toggle\n"
        );
        assert_eq!(
            invocation_line(&arguments(&["contacts", "toggle"])).unwrap(),
            "contacts toggle\n"
        );
        assert_eq!(
            invocation_line(&arguments(&["bookmarks", "toggle"])).unwrap(),
            "bookmarks toggle\n"
        );
        assert_eq!(
            invocation_line(&arguments(&["display", "toggle"])).unwrap(),
            "display toggle\n"
        );
        assert_eq!(
            invocation_line(&arguments(&["tray", "toggle"])).unwrap(),
            "tray toggle\n"
        );
        assert_eq!(
            invocation_line(&arguments(&["session", "lock"])).unwrap(),
            "session lock\n"
        );
        assert_eq!(
            invocation_line(&arguments(&["microphone", "toggle-mute"])).unwrap(),
            "microphone toggle-mute\n"
        );
        assert!(invocation_line(&arguments(&["spawn", "sh"])).is_err());
        assert!(invocation_line(&arguments(&["volume", "raise", "anything"])).is_err());
    }

    #[test]
    fn rejects_unknown_wire_messages() {
        assert!(matches!(
            parse_line("brightness lower"),
            Some(Command::LowerBrightness)
        ));
        assert!(matches!(parse_line("session lock"), Some(Command::Lock)));
        assert!(matches!(parse_line("notes toggle"), Some(Command::Notes)));
        assert!(matches!(
            parse_line("contacts toggle"),
            Some(Command::Contacts)
        ));
        assert!(matches!(
            parse_line("bookmarks toggle"),
            Some(Command::Bookmarks)
        ));
        assert!(parse_line("brightness lower; reboot").is_none());
        assert!(parse_line("").is_none());
    }
}
