use std::{
    io::{BufRead, BufReader, BufWriter, Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    thread,
    time::Instant,
};

use anyhow::{Context as _, Result, bail, ensure};
use serde::{Deserialize, Serialize};

use crate::{Config, MAX_PCM_BYTES};

const READY_PREFIX: &str = "FOYER_SHELL_TTS_READY ";
const RESPONSE_PREFIX: &str = "FOYER_SHELL_TTS_RESPONSE ";

#[derive(Serialize)]
struct Request<'a> {
    text: &'a str,
    style: &'a str,
    style_degree: f32,
}

#[derive(Deserialize)]
struct Ready {
    sample_rate: u32,
}

#[derive(Deserialize)]
struct Response {
    ok: bool,
    sample_rate: Option<u32>,
    byte_len: Option<usize>,
    error: Option<String>,
}

pub struct Worker {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    pub sample_rate: u32,
    pub load_ms: u64,
}

impl Worker {
    pub fn spawn(config: &Config) -> Result<Self> {
        config.validate()?;
        std::fs::create_dir_all(&config.hf_home)
            .with_context(|| format!("create {}", config.hf_home.display()))?;
        let started = Instant::now();
        let mut child = Command::new(&config.python)
            .arg(&config.worker)
            .arg("--reference")
            .arg(&config.reference)
            .arg("--device")
            .arg(&config.device)
            .arg("--threads")
            .arg(config.threads.to_string())
            .env("HF_HOME", &config.hf_home)
            .env("TORCH_HOME", config.hf_home.join("torch"))
            .env("OMP_NUM_THREADS", config.threads.to_string())
            .env("MKL_NUM_THREADS", config.threads.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("start TTS worker {}", config.worker.display()))?;
        let stdin = BufWriter::new(child.stdin.take().context("TTS worker stdin is missing")?);
        let mut stdout = BufReader::new(
            child
                .stdout
                .take()
                .context("TTS worker stdout is missing")?,
        );
        if let Some(stderr) = child.stderr.take() {
            thread::Builder::new()
                .name("foyer-shell-tts-python-log".into())
                .spawn(move || {
                    for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                        eprintln!("Chatterbox: {line}");
                    }
                })
                .context("start TTS worker log reader")?;
        }
        let ready: Ready = read_prefixed_json(&mut stdout, READY_PREFIX, &mut child)?;
        ensure!(
            (8_000..=96_000).contains(&ready.sample_rate),
            "TTS worker reported invalid sample rate"
        );
        Ok(Self {
            child,
            stdin,
            stdout,
            sample_rate: ready.sample_rate,
            load_ms: started.elapsed().as_millis() as u64,
        })
    }

    pub fn synthesize(&mut self, text: &str, style: &str, style_degree: f32) -> Result<Vec<u8>> {
        serde_json::to_writer(
            &mut self.stdin,
            &Request {
                text,
                style,
                style_degree,
            },
        )?;
        self.stdin.write_all(b"\n")?;
        self.stdin.flush().context("send TTS worker request")?;
        let response: Response =
            read_prefixed_json(&mut self.stdout, RESPONSE_PREFIX, &mut self.child)?;
        if !response.ok {
            bail!(
                "Chatterbox synthesis failed: {}",
                response.error.as_deref().unwrap_or("unknown worker error")
            );
        }
        let sample_rate = response
            .sample_rate
            .context("TTS response omitted sample rate")?;
        ensure!(
            sample_rate == self.sample_rate,
            "TTS worker changed sample rate"
        );
        let byte_len = response
            .byte_len
            .context("TTS response omitted PCM length")?;
        ensure!(
            byte_len > 0 && byte_len <= MAX_PCM_BYTES,
            "invalid TTS PCM length"
        );
        ensure!(
            byte_len.is_multiple_of(2),
            "TTS PCM is not aligned to 16-bit samples"
        );
        let mut pcm = vec![0; byte_len];
        self.stdout
            .read_exact(&mut pcm)
            .context("read framed TTS PCM")?;
        Ok(pcm)
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn read_prefixed_json<T: for<'de> Deserialize<'de>>(
    stdout: &mut BufReader<ChildStdout>,
    prefix: &str,
    child: &mut Child,
) -> Result<T> {
    loop {
        let mut line = String::new();
        let read = stdout
            .read_line(&mut line)
            .context("read TTS worker frame")?;
        if read == 0 {
            let status = child
                .try_wait()?
                .map(|status| status.to_string())
                .unwrap_or_else(|| "unknown".into());
            bail!("TTS worker closed its protocol stream ({status})");
        }
        if let Some(payload) = line.trim_end().strip_prefix(prefix) {
            return serde_json::from_str(payload).context("decode TTS worker frame");
        }
        tracing::debug!(line = line.trim_end(), "ignored unframed TTS worker output");
    }
}
