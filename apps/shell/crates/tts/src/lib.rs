//! Shared local text-to-speech client and service primitives.

use std::{env, path::PathBuf, time::Duration};

use anyhow::{Context as _, Result, ensure};
use zbus::blocking::{Connection, Proxy};

pub mod service;
mod worker;

pub const BUS_NAME: &str = "org.amazity.FoyerShell.TextToSpeech1";
pub const OBJECT_PATH: &str = "/org/amazity/FoyerShell/TextToSpeech1";
pub const INTERFACE: &str = "org.amazity.FoyerShell.TextToSpeech1";
pub const MAX_TEXT_CHARS: usize = 4_096;
pub const MAX_PCM_BYTES: usize = 32 * 1024 * 1024;

pub type StatusWire = (String, String, u64, String);
pub type SynthesisWire = (u32, Vec<u8>, u64);

#[derive(Clone, Debug)]
pub struct Config {
    pub python: PathBuf,
    pub worker: PathBuf,
    pub reference: PathBuf,
    pub hf_home: PathBuf,
    pub device: String,
    pub threads: usize,
    pub voice: String,
}

impl Config {
    pub fn from_env() -> Self {
        let data_home = foyer_shell_paths::data_root();
        let tts_home = data_home.join("tts");
        Self {
            python: env::var_os("FOYER_SHELL_TTS_PYTHON")
                .map(PathBuf::from)
                .unwrap_or_else(|| tts_home.join("venv/bin/python")),
            worker: env::var_os("FOYER_SHELL_TTS_WORKER")
                .map(PathBuf::from)
                .unwrap_or_else(|| tts_home.join("worker.py")),
            reference: env::var_os("FOYER_SHELL_TTS_REFERENCE")
                .map(PathBuf::from)
                .unwrap_or_else(|| tts_home.join("voices/tifa.wav")),
            hf_home: env::var_os("FOYER_SHELL_TTS_HF_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| data_home.join("models/chatterbox/huggingface")),
            device: env::var("FOYER_SHELL_TTS_DEVICE").unwrap_or_else(|_| "cuda".into()),
            threads: env::var("FOYER_SHELL_TTS_THREADS")
                .ok()
                .and_then(|value| value.parse().ok())
                .filter(|value| (1..=12).contains(value))
                .unwrap_or(4),
            voice: env::var("FOYER_SHELL_TTS_VOICE").unwrap_or_else(|_| "tifa".into()),
        }
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.device == "cuda",
            "Chatterbox Nano requires FOYER_SHELL_TTS_DEVICE=cuda"
        );
        ensure!(
            self.python.is_file(),
            "TTS Python is missing: {}",
            self.python.display()
        );
        ensure!(
            self.worker.is_file(),
            "TTS worker is missing: {}",
            self.worker.display()
        );
        ensure!(
            self.reference.is_file(),
            "TTS voice reference is missing: {}",
            self.reference.display()
        );
        ensure!(
            valid_identifier(&self.voice),
            "invalid configured TTS voice name"
        );
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct Synthesis {
    pub sample_rate: u32,
    pub samples: Vec<f32>,
    pub synthesis_ms: u64,
}

pub struct Client {
    connection: Connection,
}

impl Client {
    pub fn connect() -> Result<Self> {
        Ok(Self {
            connection: Connection::session().context("connect to the session D-Bus")?,
        })
    }

    pub fn status(&self) -> Result<StatusWire> {
        self.proxy()?
            .call("GetStatus", &())
            .context("read shared TTS status")
    }

    pub fn wait_ready(timeout: Duration) -> Result<StatusWire> {
        let started = std::time::Instant::now();
        loop {
            match Self::connect().and_then(|client| client.status()) {
                Ok(status) if status.0 == "ready" => return Ok(status),
                Ok(status) if status.0 == "error" => anyhow::bail!(status.3),
                Ok(_) | Err(_) if started.elapsed() < timeout => {
                    std::thread::sleep(Duration::from_millis(100));
                }
                Ok(status) => anyhow::bail!("TTS service did not become ready: {}", status.0),
                Err(error) => return Err(error).context("TTS service did not become ready"),
            }
        }
    }

    pub fn synthesize(
        &self,
        channel: &str,
        text: &str,
        style: &str,
        style_degree: f32,
    ) -> Result<Synthesis> {
        validate_request(channel, text, style, style_degree)?;
        let (sample_rate, pcm, synthesis_ms): SynthesisWire = self
            .proxy()?
            .call(
                "Synthesize",
                &(
                    channel.to_string(),
                    text.to_string(),
                    style.to_string(),
                    f64::from(style_degree),
                ),
            )
            .context("request shared TTS synthesis")?;
        ensure!(
            (8_000..=96_000).contains(&sample_rate),
            "invalid TTS sample rate"
        );
        ensure!(!pcm.is_empty(), "TTS service returned empty PCM");
        ensure!(
            pcm.len() <= MAX_PCM_BYTES,
            "TTS service returned oversized PCM"
        );
        ensure!(
            pcm.len().is_multiple_of(2),
            "TTS service returned unaligned PCM16"
        );
        Ok(Synthesis {
            sample_rate,
            samples: pcm
                .chunks_exact(2)
                .map(|sample| i16::from_le_bytes([sample[0], sample[1]]) as f32 / 32_768.0)
                .collect(),
            synthesis_ms,
        })
    }

    fn proxy(&self) -> Result<Proxy<'_>> {
        Proxy::new(&self.connection, BUS_NAME, OBJECT_PATH, INTERFACE)
            .context("create shared TTS proxy")
    }
}

pub(crate) fn validate_request(
    channel: &str,
    text: &str,
    style: &str,
    style_degree: f32,
) -> Result<()> {
    ensure!(valid_identifier(channel), "invalid TTS channel");
    ensure!(!text.trim().is_empty(), "TTS text is empty");
    ensure!(
        text.chars().count() <= MAX_TEXT_CHARS,
        "TTS text is too long"
    );
    ensure!(valid_identifier(style), "invalid TTS style");
    ensure!(
        style_degree.is_finite() && (0.5..=1.5).contains(&style_degree),
        "invalid TTS style degree"
    );
    Ok(())
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_validation_bounds_shared_inputs() {
        assert!(validate_request("presentation", "hello", "neutral", 1.0).is_ok());
        assert!(validate_request("Presentation", "hello", "neutral", 1.0).is_err());
        assert!(validate_request("agent", "", "neutral", 1.0).is_err());
        assert!(
            validate_request("agent", &"x".repeat(MAX_TEXT_CHARS + 1), "neutral", 1.0).is_err()
        );
        assert!(validate_request("agent", "hello", "neutral", 2.0).is_err());
    }
}
