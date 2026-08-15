use std::env;
use std::fs;
use std::io::{self, Read};
use std::process::ExitCode;

use foyer_server::auth::{
    DeviceAddStatus, DeviceRevokeStatus, add_device, list_devices, public_jwk_from_bytes,
    revoke_device,
};
use foyer_server::db;

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("devices") => devices(args.collect()).await,
        Some("help" | "--help" | "-h") | None => {
            print_help();
            Ok(())
        }
        Some(other) => Err(format!("unknown command {other:?}\n{}", usage())),
    }
}

async fn devices(args: Vec<String>) -> Result<(), String> {
    let mut args = args.into_iter();
    match args.next().as_deref() {
        Some("add") => add(parse_flags(args)?).await,
        Some("list") => list(parse_flags(args)?).await,
        Some("revoke") => revoke(parse_flags(args)?).await,
        Some(other) => Err(format!("unknown devices command {other:?}\n{}", usage())),
        None => Err(usage()),
    }
}

async fn add(flags: Flags) -> Result<(), String> {
    let user_id = flags.required("user-id")?;
    let label = flags.required("label")?;
    let bytes = match flags.optional("jwk") {
        Some("-") | None => read_stdin()?,
        Some(path) => fs::read(path).map_err(|error| format!("failed to read {path}: {error}"))?,
    };
    let jwk = public_jwk_from_bytes(&bytes)?;
    let thumbprint = jwk.device_key_id();
    let pool = connect().await?;
    let added = add_device(&pool, user_id, label, &jwk).await?;
    let status = match added.status {
        DeviceAddStatus::Created => "created",
        DeviceAddStatus::Updated => "updated",
        DeviceAddStatus::Unchanged => "unchanged",
    };
    println!("deviceKeyId={thumbprint}");
    println!("userId={}", added.user_id);
    println!("label={}", added.label);
    println!("status={status}");
    Ok(())
}

async fn list(flags: Flags) -> Result<(), String> {
    let pool = connect().await?;
    let devices = list_devices(&pool, flags.optional("user-id")).await?;
    println!("deviceKeyId\tuserId\tlabel\tcreatedAt\tlastSeenAt\trevokedAt");
    for device in devices {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            device.device_key_id,
            device.user_id,
            device.label,
            device.created_at.to_rfc3339(),
            device
                .last_seen_at
                .map(|value| value.to_rfc3339())
                .unwrap_or_default(),
            device
                .revoked_at
                .map(|value| value.to_rfc3339())
                .unwrap_or_default(),
        );
    }
    Ok(())
}

async fn revoke(flags: Flags) -> Result<(), String> {
    let device_key_id = flags
        .optional("device-key-id")
        .or(flags.optional("deviceKeyId"))
        .ok_or_else(|| "missing --device-key-id".to_string())?;
    let pool = connect().await?;
    let revoked = revoke_device(&pool, device_key_id).await?;
    let status = match revoked.status {
        DeviceRevokeStatus::Revoked => "revoked",
        DeviceRevokeStatus::AlreadyRevoked => "already-revoked",
    };
    println!("deviceKeyId={}", revoked.device_key_id);
    println!("userId={}", revoked.user_id);
    println!("label={}", revoked.label);
    println!("status={status}");
    Ok(())
}

async fn connect() -> Result<sqlx::PgPool, String> {
    let database_url =
        env::var("FOYER_DATABASE_URL").map_err(|_| "FOYER_DATABASE_URL is required".to_string())?;
    db::connect(&database_url).await
}

fn read_stdin() -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    io::stdin()
        .read_to_end(&mut bytes)
        .map_err(|error| format!("failed to read JWK from stdin: {error}"))?;
    if bytes.is_empty() {
        return Err("public JWK is required on stdin or --jwk <file>".into());
    }
    Ok(bytes)
}

struct Flags {
    values: Vec<(String, String)>,
}

impl Flags {
    fn optional(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    fn required(&self, name: &str) -> Result<&str, String> {
        self.optional(name)
            .ok_or_else(|| format!("missing --{name}"))
    }
}

fn parse_flags(args: impl IntoIterator<Item = String>) -> Result<Flags, String> {
    let mut values = Vec::new();
    let mut args = args.into_iter().peekable();
    while let Some(arg) = args.next() {
        let Some(name) = arg.strip_prefix("--") else {
            return Err(format!("unexpected argument {arg:?}"));
        };
        let value = if let Some((name, value)) = name.split_once('=') {
            values.push((name.to_string(), value.to_string()));
            continue;
        } else {
            args.next()
                .ok_or_else(|| format!("missing value for --{name}"))?
        };
        values.push((name.to_string(), value));
    }
    Ok(Flags { values })
}

fn usage() -> String {
    "Usage:
  foyer-admin devices add --user-id <id> --label <label> [--jwk <file>|-]
  foyer-admin devices list [--user-id <id>]
  foyer-admin devices revoke --device-key-id <thumbprint>"
        .into()
}

fn print_help() {
    println!(
        "foyer-admin manages enrolled device keys through direct database authority.
It does not expose or call a remote admin endpoint.

{}",
        usage()
    );
}
