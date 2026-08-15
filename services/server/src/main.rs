use std::{
    env,
    error::Error,
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    time::Duration,
};

use foyer_server::{Config, app_state, router};
use tokio::{net::TcpListener, signal};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    if env::args().nth(1).as_deref() == Some("healthcheck") {
        return healthcheck();
    }

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .json()
        .init();

    let config = Config::from_env()?;
    if config.is_development() {
        tracing::warn!(
            user_id = config
                .dev_users
                .first()
                .map(|user| user.user_id.as_str())
                .unwrap_or("unset"),
            "starting with development-only authentication; this must not be used in production"
        );
    }
    let bind = config.bind;
    let state = app_state(config).await?;
    let listener = TcpListener::bind(bind).await?;
    info!(bind = %bind, "Foyer server listening");

    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn healthcheck() -> Result<(), Box<dyn Error>> {
    let address: SocketAddr = env::var("FOYER_SERVER_HEALTH_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:3583".into())
        .parse()?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream
        .write_all(b"GET /health/ready HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;

    let mut response = [0_u8; 64];
    let read = stream.read(&mut response)?;
    let status = String::from_utf8_lossy(&response[..read]);
    if status.starts_with("HTTP/1.1 200") {
        Ok(())
    } else {
        Err(format!("readiness endpoint returned an unexpected response: {status}").into())
    }
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate = signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler");
        tokio::select! {
            _ = signal::ctrl_c() => {},
            _ = terminate.recv() => {},
        }
    }

    #[cfg(not(unix))]
    signal::ctrl_c().await.expect("install Ctrl+C handler");

    info!("shutdown requested");
}
