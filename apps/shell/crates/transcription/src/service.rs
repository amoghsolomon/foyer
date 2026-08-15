use std::{
    sync::{Arc, Mutex, mpsc},
    thread,
};

use anyhow::{Context as _, Result, ensure};
use zbus::{
    blocking::{Connection, connection::Builder},
    fdo::{RequestNameFlags, RequestNameReply},
};

use crate::{BUS_NAME, Config, OBJECT_PATH, Snapshot, SnapshotWire, State, WAVEFORM_SAMPLES};
use crate::{recognizer::Recognizer, recorder::Recorder};

pub enum EngineEvent {
    Waveform {
        session_id: u64,
        rms: f32,
        samples: [f32; WAVEFORM_SAMPLES],
    },
    CaptureFailed {
        session_id: u64,
        error: String,
    },
    ModelLoaded {
        session_id: u64,
    },
    Finished {
        session_id: u64,
        result: Result<String, String>,
    },
}

struct Inner {
    snapshot: Snapshot,
    next_session_id: u64,
    recorder: Option<Recorder>,
}

impl Inner {
    fn update(&mut self, update: impl FnOnce(&mut Snapshot)) {
        self.snapshot.generation = self.snapshot.generation.wrapping_add(1).max(1);
        update(&mut self.snapshot);
    }
}

struct TranscriptionInterface {
    inner: Arc<Mutex<Inner>>,
    recognizer: Arc<Mutex<Option<Recognizer>>>,
    config: Config,
    events: mpsc::Sender<EngineEvent>,
}

#[zbus::interface(name = "org.amazity.FoyerShell.Transcription1")]
impl TranscriptionInterface {
    fn start(&self, channel: String) -> zbus::fdo::Result<u64> {
        if channel.is_empty()
            || channel.len() > 64
            || !channel
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(zbus::fdo::Error::InvalidArgs(
                "invalid transcription channel".into(),
            ));
        }
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| zbus::fdo::Error::Failed("transcription state is poisoned".into()))?;
        if inner.snapshot.state.is_active() {
            return Err(zbus::fdo::Error::Failed(
                "a transcription session is already active".into(),
            ));
        }
        inner.next_session_id = inner.next_session_id.wrapping_add(1).max(1);
        let session_id = inner.next_session_id;
        let recorder = Recorder::start(session_id, self.events.clone())
            .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
        inner.recorder = Some(recorder);
        inner.update(|snapshot| {
            snapshot.state = State::Recording;
            snapshot.session_id = session_id;
            snapshot.channel = Arc::from(channel);
            snapshot.rms = 0.0;
            snapshot.waveform = Arc::new([0.0; WAVEFORM_SAMPLES]);
            snapshot.transcript = Arc::from("");
            snapshot.error = Arc::from("");
        });
        Ok(session_id)
    }

    fn stop(&self, session_id: u64) -> zbus::fdo::Result<bool> {
        let audio = {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| zbus::fdo::Error::Failed("transcription state is poisoned".into()))?;
            if inner.snapshot.state != State::Recording || inner.snapshot.session_id != session_id {
                return Ok(false);
            }
            let recorder = inner
                .recorder
                .take()
                .ok_or_else(|| zbus::fdo::Error::Failed("microphone recorder is missing".into()))?;
            let audio = recorder
                .stop()
                .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
            let loading = self
                .recognizer
                .lock()
                .map(|recognizer| recognizer.is_none())
                .unwrap_or(true);
            inner.update(|snapshot| {
                snapshot.state = if loading {
                    State::LoadingModel
                } else {
                    State::Transcribing
                };
                snapshot.rms = 0.0;
            });
            audio
        };

        let recognizer = self.recognizer.clone();
        let events = self.events.clone();
        let config = self.config.clone();
        thread::Builder::new()
            .name(format!("foyer-shell-transcription-inference-{session_id}"))
            .spawn(move || {
                let result = (|| -> Result<String> {
                    let mut recognizer = recognizer
                        .lock()
                        .map_err(|_| anyhow::anyhow!("recognizer is poisoned"))?;
                    if recognizer.is_none() {
                        *recognizer = Some(Recognizer::load(&config)?);
                        let _ = events.send(EngineEvent::ModelLoaded { session_id });
                    }
                    recognizer
                        .as_mut()
                        .context("recognizer was not initialized")?
                        .transcribe(audio)
                })();
                let _ = events.send(EngineEvent::Finished {
                    session_id,
                    result: result.map_err(|error| error.to_string()),
                });
            })
            .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
        Ok(true)
    }

    fn cancel(&self, session_id: u64) -> zbus::fdo::Result<bool> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| zbus::fdo::Error::Failed("transcription state is poisoned".into()))?;
        if inner.snapshot.session_id != session_id || !inner.snapshot.state.is_active() {
            return Ok(false);
        }
        if let Some(recorder) = inner.recorder.take() {
            recorder.abort();
        }
        inner.update(|snapshot| {
            snapshot.state = State::Idle;
            snapshot.session_id = 0;
            snapshot.channel = Arc::from("");
            snapshot.rms = 0.0;
            snapshot.waveform = Arc::new([0.0; WAVEFORM_SAMPLES]);
            snapshot.transcript = Arc::from("");
            snapshot.error = Arc::from("");
        });
        Ok(true)
    }

    fn get_snapshot(&self) -> zbus::fdo::Result<SnapshotWire> {
        self.inner
            .lock()
            .map(|inner| inner.snapshot.to_wire())
            .map_err(|_| zbus::fdo::Error::Failed("transcription state is poisoned".into()))
    }
}

pub fn run(config: Config) -> Result<()> {
    ensure!(config.ngram_alpha >= 0.0 && config.ngram_alpha <= 2.0);
    let (events, event_rx) = mpsc::channel();
    let inner = Arc::new(Mutex::new(Inner {
        snapshot: Snapshot {
            state: State::Idle,
            error: Arc::from(""),
            ..Snapshot::default()
        },
        next_session_id: 0,
        recorder: None,
    }));
    let interface = TranscriptionInterface {
        inner: inner.clone(),
        recognizer: Arc::new(Mutex::new(None)),
        config,
        events,
    };
    let connection = Builder::session()
        .context("connect to the session bus")?
        .serve_at(OBJECT_PATH, interface)
        .context("publish transcription D-Bus object")?
        .build()
        .context("start transcription D-Bus connection")?;
    request_name(&connection)?;

    while let Ok(event) = event_rx.recv() {
        let mut inner = inner
            .lock()
            .map_err(|_| anyhow::anyhow!("transcription state is poisoned"))?;
        match event {
            EngineEvent::Waveform {
                session_id,
                rms,
                samples,
            } if inner.snapshot.state == State::Recording
                && inner.snapshot.session_id == session_id =>
            {
                inner.update(|snapshot| {
                    snapshot.rms = rms;
                    snapshot.waveform = Arc::new(samples);
                });
            }
            EngineEvent::CaptureFailed { session_id, error }
                if inner.snapshot.session_id == session_id =>
            {
                inner.recorder.take();
                inner.update(|snapshot| {
                    snapshot.state = State::Error;
                    snapshot.rms = 0.0;
                    snapshot.error = Arc::from(format!("Microphone capture failed: {error}"));
                });
            }
            EngineEvent::ModelLoaded { session_id }
                if inner.snapshot.session_id == session_id
                    && inner.snapshot.state == State::LoadingModel =>
            {
                inner.update(|snapshot| snapshot.state = State::Transcribing);
            }
            EngineEvent::Finished { session_id, result }
                if inner.snapshot.session_id == session_id && inner.snapshot.state.is_active() =>
            {
                inner.update(|snapshot| match result {
                    Ok(text) if text.trim().is_empty() => {
                        snapshot.state = State::Error;
                        snapshot.error = Arc::from("No speech was recognized");
                    }
                    Ok(text) => {
                        snapshot.state = State::Ready;
                        snapshot.transcript = Arc::from(text);
                        snapshot.error = Arc::from("");
                    }
                    Err(error) => {
                        snapshot.state = State::Error;
                        snapshot.error = Arc::from(error);
                    }
                });
            }
            _ => {}
        }
    }
    Ok(())
}

fn request_name(connection: &Connection) -> Result<()> {
    let reply = connection
        .request_name_with_flags(BUS_NAME, RequestNameFlags::DoNotQueue.into())
        .context("request transcription D-Bus name")?;
    match reply {
        RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner => Ok(()),
        RequestNameReply::Exists | RequestNameReply::InQueue => {
            anyhow::bail!("another transcription service owns {BUS_NAME}")
        }
    }
}
