use std::{fs, path::Path, time::Duration};

use foyer_shell_pi_bridge::{PiConfig, PiHarness, SidecarMessage};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let state_dir = Path::new(".foyer-shell/pi-smoke");
    fs::create_dir_all(state_dir)?;

    let config = PiConfig::for_workspace(Path::new("."));
    let mut harness = PiHarness::spawn(&config, Path::new("."), state_dir)?;
    let prompt = std::env::var("FOYER_SHELL_SMOKE_PROMPT").unwrap_or_else(|_| {
        "Explain in three concise slides how a seed becomes a flowering plant.".into()
    });
    harness.prompt("smoke-1", &prompt)?;

    loop {
        match harness.events().recv_timeout(Duration::from_secs(90))?? {
            SidecarMessage::Events { events, .. } => {
                for event in events {
                    println!("{}", serde_json::to_string(&event)?);
                }
            }
            SidecarMessage::Settled { .. } => break,
            SidecarMessage::Error { message, fatal, .. } => {
                eprintln!("sidecar error: {message}");
                if fatal {
                    break;
                }
            }
            SidecarMessage::PresentationMaterials { .. } => {}
            SidecarMessage::Ready { .. } => {}
        }
    }
    Ok(())
}
