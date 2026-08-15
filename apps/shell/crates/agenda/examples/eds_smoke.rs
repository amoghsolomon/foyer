use std::time::Duration;

fn main() {
    let runtime = foyer_shell_agenda::start();
    match runtime.updates.recv_blocking() {
        Ok(snapshot) => {
            let (status, detail_kind) = match snapshot.availability {
                foyer_shell_agenda::Availability::Loading => ("loading", "none"),
                foyer_shell_agenda::Availability::Available => ("available", "none"),
                foyer_shell_agenda::Availability::Unavailable(error) => {
                    let kind = if error.contains("timed out") {
                        "timeout"
                    } else if error.contains("invalid data") {
                        "invalid-data"
                    } else if error.contains("bridge failed") {
                        "bridge-failed"
                    } else if error.contains("bridge is unavailable") {
                        "missing-runtime"
                    } else {
                        "provider"
                    };
                    ("unavailable", kind)
                }
            };
            println!(
                "status={status} detail_kind={detail_kind} sources={} items={} partial_error={}",
                snapshot.sources.len(),
                snapshot.items.len(),
                snapshot.last_error.is_some()
            );
        }
        Err(_) => println!("status=worker-stopped sources=0 items=0 partial_error=true"),
    }
    std::thread::sleep(Duration::from_millis(10));
}
