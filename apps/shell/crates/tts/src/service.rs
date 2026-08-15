use std::sync::Mutex;

use anyhow::{Context as _, Result};
use zbus::{
    blocking::{Connection, connection::Builder},
    fdo::{RequestNameFlags, RequestNameReply},
};

use crate::{
    BUS_NAME, Config, OBJECT_PATH, StatusWire, SynthesisWire, validate_request, worker::Worker,
};

struct TextToSpeechInterface {
    worker: Mutex<Worker>,
    voice: String,
    load_ms: u64,
}

#[zbus::interface(name = "org.amazity.FoyerShell.TextToSpeech1")]
impl TextToSpeechInterface {
    fn get_status(&self) -> StatusWire {
        (
            "ready".into(),
            self.voice.clone(),
            self.load_ms,
            String::new(),
        )
    }

    fn synthesize(
        &self,
        channel: String,
        text: String,
        style: String,
        style_degree: f64,
    ) -> zbus::fdo::Result<SynthesisWire> {
        validate_request(&channel, &text, &style, style_degree as f32)
            .map_err(|error| zbus::fdo::Error::InvalidArgs(error.to_string()))?;
        let started = std::time::Instant::now();
        let mut worker = self
            .worker
            .lock()
            .map_err(|_| zbus::fdo::Error::Failed("TTS worker state is poisoned".into()))?;
        let pcm = worker
            .synthesize(&text, &style, style_degree as f32)
            .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
        tracing::info!(
            %channel,
            text_chars = text.chars().count(),
            synthesis_ms = started.elapsed().as_millis() as u64,
            "completed shared TTS synthesis"
        );
        Ok((
            worker.sample_rate,
            pcm,
            started.elapsed().as_millis() as u64,
        ))
    }
}

pub fn run(config: Config) -> Result<()> {
    let worker = Worker::spawn(&config)?;
    let load_ms = worker.load_ms;
    let interface = TextToSpeechInterface {
        worker: Mutex::new(worker),
        voice: config.voice.clone(),
        load_ms,
    };
    let connection = Builder::session()
        .context("connect to the session bus")?
        .serve_at(OBJECT_PATH, interface)
        .context("publish shared TTS D-Bus object")?
        .build()
        .context("start shared TTS D-Bus connection")?;
    request_name(&connection)?;
    eprintln!(
        "Foyer Shell TTS service: Chatterbox Nano CUDA · {} · loaded in {load_ms} ms",
        config.voice
    );
    loop {
        std::thread::park();
    }
}

fn request_name(connection: &Connection) -> Result<()> {
    let reply = connection
        .request_name_with_flags(BUS_NAME, RequestNameFlags::DoNotQueue.into())
        .context("request shared TTS D-Bus name")?;
    match reply {
        RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner => Ok(()),
        RequestNameReply::Exists | RequestNameReply::InQueue => {
            anyhow::bail!("another TTS service owns {BUS_NAME}")
        }
    }
}
