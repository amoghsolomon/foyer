//! Shared local Chatterbox or OpenRouter synthesis with audio-device-clock-driven playback.

use std::{
    collections::{BTreeMap, VecDeque},
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};
use async_channel::{Receiver, Sender};
use cpal::{
    Device, ErrorKind, FromSample, OutputCallbackInfo, Sample, SampleFormat, SizedSample, Stream,
    StreamConfig, StreamInstant,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use foyer_shell_protocol::{CueAction, NarrationBeat, NarrationStyle};

#[cfg(feature = "kokoro")]
pub mod native;

#[derive(Clone)]
pub struct AudioConfig {
    pub backend: AudioBackend,
    pub endpoint: String,
    pub api_key: String,
    pub model: String,
    pub voice: String,
    pub tts_channel: String,
    pub sample_rate: u32,
    pub kokoro_model: PathBuf,
    pub kokoro_voices: PathBuf,
    pub kokoro_language: String,
    pub kokoro_speed: f32,
    /// When set, the exact normalized mono PCM queued for each beat is retained as a WAV file.
    pub recording_dir: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioBackend {
    Chatterbox,
    Kokoro,
    OpenRouter,
}

impl AudioConfig {
    pub fn for_workspace(root: &Path) -> Self {
        let local = read_local_environment(root);
        let value = |name: &str, fallback: &str| {
            env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
                .or_else(|| {
                    local
                        .get(name)
                        .cloned()
                        .filter(|value| !value.trim().is_empty())
                })
                .unwrap_or_else(|| fallback.to_string())
        };
        let backend = match value("FOYER_SHELL_TTS_BACKEND", "chatterbox")
            .to_ascii_lowercase()
            .as_str()
        {
            "openrouter" | "mai" => AudioBackend::OpenRouter,
            "kokoro" => AudioBackend::Kokoro,
            _ => AudioBackend::Chatterbox,
        };
        let voice = match backend {
            AudioBackend::Chatterbox => value("FOYER_SHELL_TTS_VOICE", "tifa"),
            AudioBackend::Kokoro => value("FOYER_SHELL_KOKORO_VOICE", "af_bella"),
            AudioBackend::OpenRouter => value("FOYER_SHELL_TTS_VOICE", "alloy"),
        };
        let default_model = default_kokoro_asset(root, "kokoro-v1.0.onnx")
            .to_string_lossy()
            .into_owned();
        let default_voices = default_kokoro_asset(root, "voices-v1.0.bin")
            .to_string_lossy()
            .into_owned();
        Self {
            backend,
            endpoint: value(
                "FOYER_SHELL_TTS_ENDPOINT",
                "https://openrouter.ai/api/v1/audio/speech",
            ),
            api_key: value("OPENROUTER_API_KEY", ""),
            model: value("FOYER_SHELL_TTS_MODEL", "openai/gpt-4o-mini-tts-2025-12-15"),
            voice,
            tts_channel: value("FOYER_SHELL_TTS_CHANNEL", "presentation"),
            sample_rate: 24_000,
            kokoro_model: PathBuf::from(value("FOYER_SHELL_KOKORO_MODEL", &default_model)),
            kokoro_voices: PathBuf::from(value("FOYER_SHELL_KOKORO_VOICES", &default_voices)),
            kokoro_language: value("FOYER_SHELL_KOKORO_LANGUAGE", "en-gb"),
            kokoro_speed: value("FOYER_SHELL_KOKORO_SPEED", "1.0")
                .parse()
                .ok()
                .filter(|speed| (0.5..=2.0).contains(speed))
                .unwrap_or(1.0),
            recording_dir: None,
        }
    }

    pub fn with_recording_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.recording_dir = Some(path.into());
        self
    }
}

fn default_kokoro_asset(root: &Path, file_name: &str) -> PathBuf {
    let workspace_asset = root.join(".foyer-shell/models/kokoro").join(file_name);
    if workspace_asset.is_file() {
        return workspace_asset;
    }
    let data_home = env::var_os("XDG_DATA_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME")
                .filter(|path| !path.is_empty())
                .map(|home| PathBuf::from(home).join(".local/share"))
        });
    data_home
        .map(|data_home| data_home.join("foyer-shell/models/kokoro").join(file_name))
        .unwrap_or(workspace_asset)
}

fn read_local_environment(root: &Path) -> BTreeMap<String, String> {
    let Ok(contents) = fs::read_to_string(root.join(".env")) else {
        return BTreeMap::new();
    };
    contents
        .lines()
        .filter_map(|raw_line| {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (name, raw_value) = line.split_once('=')?;
            let value = raw_value.trim();
            let value = if value.len() >= 2
                && ((value.starts_with('"') && value.ends_with('"'))
                    || (value.starts_with('\'') && value.ends_with('\'')))
            {
                &value[1..value.len() - 1]
            } else {
                value
            };
            Some((name.trim().to_string(), value.to_string()))
        })
        .collect()
}

#[derive(Clone, Debug)]
pub enum AudioEvent {
    WorkerReady {
        load_ms: u64,
    },
    Synthesizing {
        beat_id: String,
    },
    PlaybackStarted {
        beat_id: String,
        focus: Vec<String>,
        duration_ms: u64,
        synthesis_ms: u64,
        voice: String,
    },
    Position {
        beat_id: String,
        position_ms: u64,
        duration_ms: u64,
    },
    Cue {
        beat_id: String,
        action: CueAction,
    },
    PlaybackFinished {
        beat_id: String,
    },
    RecordingStored {
        beat_id: String,
        path: PathBuf,
        duration_ms: u64,
    },
    Paused,
    Resumed,
    Stopped,
    Failed {
        beat_id: Option<String>,
        message: String,
    },
}

pub struct NarrationRuntime {
    pub requests: Sender<NarrationBeat>,
    pub controls: Sender<AudioCommand>,
    pub events: Receiver<AudioEvent>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioCommand {
    Pause,
    Resume,
    Stop,
}

#[derive(Clone, Debug)]
pub struct RecordedNarration {
    pub beat: NarrationBeat,
    pub path: PathBuf,
}

pub struct PlaybackRuntime {
    pub controls: Sender<AudioCommand>,
    pub events: Receiver<AudioEvent>,
}

struct PreparedBeat {
    beat: NarrationBeat,
    samples: Vec<f32>,
    sample_rate: u32,
    synthesis_ms: u64,
}

enum SynthesisOutcome {
    Prepared(PreparedBeat),
    Failed { beat_id: String, message: String },
}

impl NarrationRuntime {
    pub fn spawn(config: AudioConfig) -> Self {
        let (request_sender, request_receiver) = async_channel::unbounded();
        let (control_sender, control_receiver) = async_channel::unbounded();
        let (event_sender, event_receiver) = async_channel::unbounded();
        thread::Builder::new()
            .name("foyer-shell-narration".into())
            .spawn(move || {
                if let Err(error) = run(config, request_receiver, control_receiver, &event_sender) {
                    let _ = event_sender.send_blocking(AudioEvent::Failed {
                        beat_id: None,
                        message: format!("{error:#}"),
                    });
                }
            })
            .expect("failed to start narration thread");
        Self {
            requests: request_sender,
            controls: control_sender,
            events: event_receiver,
        }
    }
}

impl PlaybackRuntime {
    pub fn spawn(recordings: Vec<RecordedNarration>) -> Self {
        let (control_sender, control_receiver) = async_channel::unbounded();
        let (event_sender, event_receiver) = async_channel::unbounded();
        thread::Builder::new()
            .name("foyer-shell-recorded-narration".into())
            .spawn(move || {
                if let Err(error) = run_recorded(recordings, control_receiver, &event_sender) {
                    let _ = event_sender.send_blocking(AudioEvent::Failed {
                        beat_id: None,
                        message: format!("{error:#}"),
                    });
                }
            })
            .expect("failed to start recorded narration thread");
        Self {
            controls: control_sender,
            events: event_receiver,
        }
    }
}

fn run(
    config: AudioConfig,
    requests: Receiver<NarrationBeat>,
    controls: Receiver<AudioCommand>,
    events: &Sender<AudioEvent>,
) -> Result<()> {
    match config.backend {
        AudioBackend::Chatterbox => run_chatterbox(config, requests, controls, events),
        AudioBackend::Kokoro => run_kokoro(config, requests, controls, events),
        AudioBackend::OpenRouter => run_openrouter(config, requests, controls, events),
    }
}

fn run_chatterbox(
    config: AudioConfig,
    requests: Receiver<NarrationBeat>,
    controls: Receiver<AudioCommand>,
    events: &Sender<AudioEvent>,
) -> Result<()> {
    let (_, service_voice, load_ms, _) =
        foyer_shell_tts::Client::wait_ready(Duration::from_secs(120))?;
    let client = foyer_shell_tts::Client::connect()?;
    events.send_blocking(AudioEvent::WorkerReady { load_ms })?;
    eprintln!(
        "Foyer Shell TTS provider: shared Chatterbox Nano CUDA · {service_voice} · loaded in {load_ms} ms"
    );

    let (prepared_sender, prepared_receiver) = mpsc::sync_channel(2);
    let playback_events = events.clone();
    let playback_voice = format!("Chatterbox Nano · {service_voice}");
    let recording_dir = config.recording_dir.clone();
    let playback_thread = thread::Builder::new()
        .name("foyer-shell-playback".into())
        .spawn(move || {
            playback_loop(
                prepared_receiver,
                controls,
                &playback_events,
                &playback_voice,
                recording_dir,
            )
        })
        .context("starting the narration playback thread")?;

    while let Ok(beat) = requests.recv_blocking() {
        let beat_id = beat.id.clone();
        events.send_blocking(AudioEvent::Synthesizing {
            beat_id: beat_id.clone(),
        })?;
        match client.synthesize(
            &config.tts_channel,
            &beat.text,
            narration_style_name(beat.style),
            beat.style_degree,
        ) {
            Ok(output) => {
                let prepared = PreparedBeat {
                    beat,
                    samples: output.samples,
                    sample_rate: output.sample_rate,
                    synthesis_ms: output.synthesis_ms,
                };
                if prepared_sender.send(prepared).is_err() {
                    break;
                }
            }
            Err(error) => events.send_blocking(AudioEvent::Failed {
                beat_id: Some(beat_id),
                message: format!("Chatterbox synthesis failed: {error:#}"),
            })?,
        }
    }
    drop(prepared_sender);
    playback_thread
        .join()
        .map_err(|_| anyhow!("narration playback thread panicked"))?
}

#[cfg(feature = "kokoro")]
fn run_kokoro(
    config: AudioConfig,
    requests: Receiver<NarrationBeat>,
    controls: Receiver<AudioCommand>,
    events: &Sender<AudioEvent>,
) -> Result<()> {
    let load_started = Instant::now();
    let mut kokoro =
        native::NativeKokoro::load(&config.kokoro_model, &config.kokoro_voices, &config.voice)?;
    let load_ms = load_started.elapsed().as_millis() as u64;
    events.send_blocking(AudioEvent::WorkerReady { load_ms })?;
    eprintln!(
        "Foyer Shell TTS provider: local Kokoro CPU · {} · loaded in {load_ms} ms",
        config.voice
    );

    // A single warm model session synthesizes ahead while the playback thread owns CPAL. This
    // avoids duplicating the model in memory and still hides later inference behind current speech.
    let (prepared_sender, prepared_receiver) = mpsc::sync_channel(2);
    let playback_events = events.clone();
    let playback_voice = format!("Kokoro · {}", config.voice);
    let recording_dir = config.recording_dir.clone();
    let playback_thread = thread::Builder::new()
        .name("foyer-shell-playback".into())
        .spawn(move || {
            playback_loop(
                prepared_receiver,
                controls,
                &playback_events,
                &playback_voice,
                recording_dir,
            )
        })
        .context("starting the narration playback thread")?;

    while let Ok(beat) = requests.recv_blocking() {
        let beat_id = beat.id.clone();
        events.send_blocking(AudioEvent::Synthesizing {
            beat_id: beat_id.clone(),
        })?;
        let synthesis_started = Instant::now();
        match kokoro.synthesize(&beat.text, &config.kokoro_language, config.kokoro_speed) {
            Ok(output) => {
                let prepared = PreparedBeat {
                    beat,
                    samples: output.samples,
                    sample_rate: output.sample_rate,
                    synthesis_ms: synthesis_started.elapsed().as_millis() as u64,
                };
                if prepared_sender.send(prepared).is_err() {
                    break;
                }
            }
            Err(error) => events.send_blocking(AudioEvent::Failed {
                beat_id: Some(beat_id),
                message: format!("Kokoro synthesis failed: {error:#}"),
            })?,
        }
    }
    drop(prepared_sender);
    playback_thread
        .join()
        .map_err(|_| anyhow!("narration playback thread panicked"))?
}

#[cfg(not(feature = "kokoro"))]
fn run_kokoro(
    _config: AudioConfig,
    _requests: Receiver<NarrationBeat>,
    _controls: Receiver<AudioCommand>,
    _events: &Sender<AudioEvent>,
) -> Result<()> {
    bail!("Kokoro support was not compiled into foyer-shell-audio")
}

fn run_openrouter(
    config: AudioConfig,
    requests: Receiver<NarrationBeat>,
    controls: Receiver<AudioCommand>,
    events: &Sender<AudioEvent>,
) -> Result<()> {
    if config.api_key.trim().is_empty() {
        bail!("OPENROUTER_API_KEY is missing from .env");
    }
    events.send_blocking(AudioEvent::WorkerReady { load_ms: 0 })?;
    eprintln!(
        "Foyer Shell TTS provider: OpenRouter · {} · {}",
        config.model, config.voice
    );

    // Three network workers synthesize ahead. A separate collector restores authored order before
    // the bounded playback queue, so a slow later request can never reorder the presentation.
    let (prepared_sender, prepared_receiver) = mpsc::sync_channel(3);
    let playback_events = events.clone();
    let voice = format!("{} · {}", config.model, config.voice);
    let recording_dir = config.recording_dir.clone();
    let playback_thread = thread::Builder::new()
        .name("foyer-shell-playback".into())
        .spawn(move || {
            playback_loop(
                prepared_receiver,
                controls,
                &playback_events,
                &voice,
                recording_dir,
            )
        })
        .context("starting the narration playback thread")?;

    let (job_sender, job_receiver) = async_channel::bounded::<(u64, NarrationBeat)>(3);
    let (result_sender, result_receiver) = mpsc::channel::<(u64, SynthesisOutcome)>();
    let mut workers = Vec::new();
    for worker_index in 0..3 {
        let jobs = job_receiver.clone();
        let results = result_sender.clone();
        let worker_events = events.clone();
        let worker_config = config.clone();
        workers.push(
            thread::Builder::new()
                .name(format!("foyer-shell-tts-{worker_index}"))
                .spawn(move || {
                    while let Ok((sequence, beat)) = jobs.recv_blocking() {
                        let beat_id = beat.id.clone();
                        let _ = worker_events.send_blocking(AudioEvent::Synthesizing {
                            beat_id: beat_id.clone(),
                        });
                        let outcome = match synthesize_openrouter(&worker_config, beat) {
                            Ok(prepared) => SynthesisOutcome::Prepared(prepared),
                            Err(error) => SynthesisOutcome::Failed {
                                beat_id,
                                message: format!("{error:#}"),
                            },
                        };
                        if results.send((sequence, outcome)).is_err() {
                            break;
                        }
                    }
                })
                .context("starting an OpenRouter TTS worker")?,
        );
    }
    drop(result_sender);

    let collector_events = events.clone();
    let collector = thread::Builder::new()
        .name("foyer-shell-tts-order".into())
        .spawn(move || -> Result<()> {
            let mut expected = 0_u64;
            let mut pending = BTreeMap::new();
            while let Ok((sequence, outcome)) = result_receiver.recv() {
                pending.insert(sequence, outcome);
                while let Some(outcome) = pending.remove(&expected) {
                    match outcome {
                        SynthesisOutcome::Prepared(prepared) => {
                            if prepared_sender.send(prepared).is_err() {
                                return Ok(());
                            }
                        }
                        SynthesisOutcome::Failed { beat_id, message } => {
                            collector_events.send_blocking(AudioEvent::Failed {
                                beat_id: Some(beat_id),
                                message,
                            })?;
                        }
                    }
                    expected += 1;
                }
            }
            Ok(())
        })
        .context("starting the ordered TTS collector")?;

    let mut sequence = 0_u64;
    while let Ok(beat) = requests.recv_blocking() {
        if job_sender.send_blocking((sequence, beat)).is_err() {
            break;
        }
        sequence += 1;
    }
    drop(job_sender);
    for worker in workers {
        worker
            .join()
            .map_err(|_| anyhow!("an OpenRouter TTS worker panicked"))?;
    }
    collector
        .join()
        .map_err(|_| anyhow!("the ordered TTS collector panicked"))??;
    playback_thread
        .join()
        .map_err(|_| anyhow!("narration playback thread panicked"))?
}

fn synthesize_openrouter(config: &AudioConfig, beat: NarrationBeat) -> Result<PreparedBeat> {
    let synthesis_started = Instant::now();
    let mut request = serde_json::json!({
        "model": config.model,
        "input": beat.text,
        "voice": config.voice,
        "response_format": "pcm",
        "speed": 1.0
    });
    if uses_azure_style(&config.model)
        && let Some(style) = azure_style(beat.style)
    {
        request["provider"] = serde_json::json!({
            "options": {
                "azure": {
                    "style": style,
                    // The planner applies this range too; clamp again so recorded or hand-authored
                    // plans cannot accidentally produce a theatrical outlier.
                    "styledegree": beat.style_degree.clamp(0.8, 1.15)
                }
            }
        });
    }
    let body = serde_json::to_vec(&request)?;
    let mut response = ureq::post(&config.endpoint)
        .header("Authorization", &format!("Bearer {}", config.api_key))
        .header("Content-Type", "application/json")
        .header("HTTP-Referer", "https://shell.local")
        .header("X-Title", "Foyer Shell")
        .send(body.as_slice())
        .with_context(|| format!("requesting {} from OpenRouter", config.model))?;
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = response
        .body_mut()
        .with_config()
        .limit(16 * 1024 * 1024)
        .read_to_vec()
        .context("reading OpenRouter speech bytes")?;
    if content_type.contains("json") {
        bail!(
            "OpenRouter returned JSON instead of audio: {}",
            String::from_utf8_lossy(&bytes)
        );
    }
    let samples = decode_pcm16(&bytes)?;
    Ok(PreparedBeat {
        beat,
        samples,
        sample_rate: config.sample_rate,
        synthesis_ms: synthesis_started.elapsed().as_millis() as u64,
    })
}

fn uses_azure_style(model: &str) -> bool {
    model.starts_with("microsoft/mai-voice-")
}

fn narration_style_name(style: NarrationStyle) -> &'static str {
    match style {
        NarrationStyle::Neutral => "neutral",
        NarrationStyle::Angry => "angry",
        NarrationStyle::Confused => "confused",
        NarrationStyle::Determined => "determined",
        NarrationStyle::Embarrassed => "embarrassed",
        NarrationStyle::Excited => "excited",
        NarrationStyle::Happy => "happy",
        NarrationStyle::Hopeful => "hopeful",
        NarrationStyle::Joyful => "joyful",
        NarrationStyle::Regretful => "regretful",
        NarrationStyle::Relieved => "relieved",
        NarrationStyle::Sad => "sad",
        NarrationStyle::Shouting => "shouting",
        NarrationStyle::Softvoice => "softvoice",
        NarrationStyle::Whispering => "whispering",
    }
}

fn azure_style(style: NarrationStyle) -> Option<&'static str> {
    match style {
        NarrationStyle::Neutral => None,
        NarrationStyle::Angry => Some("angry"),
        NarrationStyle::Confused => Some("confused"),
        NarrationStyle::Determined => Some("determined"),
        NarrationStyle::Embarrassed => Some("embarrassed"),
        NarrationStyle::Excited => Some("excited"),
        NarrationStyle::Happy => Some("happy"),
        NarrationStyle::Hopeful => Some("hopeful"),
        NarrationStyle::Joyful => Some("joyful"),
        NarrationStyle::Regretful => Some("regretful"),
        NarrationStyle::Relieved => Some("relieved"),
        NarrationStyle::Sad => Some("sad"),
        NarrationStyle::Shouting => Some("shouting"),
        NarrationStyle::Softvoice => Some("softvoice"),
        NarrationStyle::Whispering => Some("whispering"),
    }
}

fn decode_pcm16(bytes: &[u8]) -> Result<Vec<f32>> {
    if bytes.is_empty() {
        bail!("OpenRouter returned empty PCM audio");
    }
    if bytes.starts_with(b"RIFF") {
        bail!("OpenRouter returned a WAV container when raw PCM was requested");
    }
    if !bytes.len().is_multiple_of(2) {
        bail!("OpenRouter returned an odd-length PCM buffer");
    }
    Ok(bytes
        .chunks_exact(2)
        .map(|sample| i16::from_le_bytes([sample[0], sample[1]]) as f32 / 32_768.0)
        .collect())
}

fn playback_loop(
    prepared: mpsc::Receiver<PreparedBeat>,
    controls: Receiver<AudioCommand>,
    events: &Sender<AudioEvent>,
    voice: &str,
    recording_dir: Option<PathBuf>,
) -> Result<()> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .context("no default audio output device")?;
    let device_name = device.to_string();
    let supported = device
        .default_output_config()
        .context("querying the default audio output format")?;
    let sample_format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let output_rate = config.sample_rate;
    let channels = config.channels as usize;
    let queue = Arc::new(Mutex::new(VecDeque::<QueuedAudioBeat>::new()));
    let paused = Arc::new(AtomicBool::new(false));
    let stopped = Arc::new(AtomicBool::new(false));
    let (callback_tx, callback_rx) = mpsc::channel();

    let stream = match sample_format {
        SampleFormat::F32 => build_continuous_stream::<f32>(
            &device,
            config,
            channels,
            output_rate,
            queue.clone(),
            paused.clone(),
            stopped.clone(),
            callback_tx,
        )?,
        SampleFormat::I16 => build_continuous_stream::<i16>(
            &device,
            config,
            channels,
            output_rate,
            queue.clone(),
            paused.clone(),
            stopped.clone(),
            callback_tx,
        )?,
        SampleFormat::U16 => build_continuous_stream::<u16>(
            &device,
            config,
            channels,
            output_rate,
            queue.clone(),
            paused.clone(),
            stopped.clone(),
            callback_tx,
        )?,
        format => bail!("unsupported output sample format {format}"),
    };
    stream
        .play()
        .context("starting continuous audio playback")?;
    eprintln!(
        "Foyer Shell audio: persistent device={device_name:?} rate={output_rate}Hz channels={channels}"
    );

    let mut input_closed = false;
    let mut scheduled = VecDeque::new();
    let mut active_cues = BTreeMap::<String, VecDeque<(u64, CueAction)>>::new();
    let mut idle_since = None;
    loop {
        while let Ok(command) = controls.try_recv() {
            match command {
                AudioCommand::Pause if !paused.swap(true, Ordering::SeqCst) => {
                    events.send_blocking(AudioEvent::Paused)?;
                }
                AudioCommand::Resume if paused.swap(false, Ordering::SeqCst) => {
                    events.send_blocking(AudioEvent::Resumed)?;
                }
                AudioCommand::Stop => stopped.store(true, Ordering::SeqCst),
                AudioCommand::Pause | AudioCommand::Resume => {}
            }
        }
        if stopped.load(Ordering::SeqCst) {
            events.send_blocking(AudioEvent::Stopped)?;
            break;
        }
        while !input_closed {
            match prepared.try_recv() {
                Ok(prepared) => enqueue_audio(
                    &queue,
                    prepared,
                    output_rate,
                    recording_dir.as_deref(),
                    events,
                )?,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    input_closed = true;
                    break;
                }
            }
        }
        while let Ok(event) = callback_rx.try_recv() {
            match event {
                CallbackEvent::Scheduled(event) => scheduled.push_back(event),
                CallbackEvent::Warning(warning) => {
                    eprintln!("Foyer Shell audio: transient stream warning: {warning}");
                }
                CallbackEvent::Error(error) => return Err(anyhow!(error)),
            }
        }

        let now = stream.now();
        while scheduled.front().is_some_and(|event| event.playback <= now) {
            let event = scheduled.pop_front().expect("checked scheduled event");
            match event.kind {
                ScheduledKind::Start {
                    beat,
                    duration_ms,
                    synthesis_ms,
                } => {
                    active_cues.insert(beat.id.clone(), resolve_cues(&beat, duration_ms).into());
                    events.send_blocking(AudioEvent::PlaybackStarted {
                        beat_id: beat.id,
                        focus: beat.focus,
                        duration_ms,
                        synthesis_ms,
                        voice: voice.to_string(),
                    })?;
                }
                ScheduledKind::Progress {
                    beat_id,
                    position_ms,
                    duration_ms,
                } => {
                    if let Some(cues) = active_cues.get_mut(&beat_id) {
                        while cues
                            .front()
                            .is_some_and(|(cue_ms, _)| *cue_ms <= position_ms)
                        {
                            let (_, action) = cues.pop_front().expect("checked cue");
                            events.send_blocking(AudioEvent::Cue {
                                beat_id: beat_id.clone(),
                                action,
                            })?;
                        }
                    }
                    events.send_blocking(AudioEvent::Position {
                        beat_id,
                        position_ms,
                        duration_ms,
                    })?;
                }
                ScheduledKind::End { beat_id } => {
                    if let Some(mut cues) = active_cues.remove(&beat_id) {
                        while let Some((_, action)) = cues.pop_front() {
                            events.send_blocking(AudioEvent::Cue {
                                beat_id: beat_id.clone(),
                                action,
                            })?;
                        }
                    }
                    events.send_blocking(AudioEvent::PlaybackFinished { beat_id })?;
                }
            }
        }

        let queue_empty = queue.lock().map_or(true, |queue| queue.is_empty());
        if input_closed && queue_empty && scheduled.is_empty() {
            let since = idle_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= Duration::from_millis(120) {
                break;
            }
        } else {
            idle_since = None;
        }
        thread::sleep(Duration::from_millis(3));
    }
    drop(stream);
    Ok(())
}

fn resolve_cues(beat: &NarrationBeat, duration_ms: u64) -> Vec<(u64, CueAction)> {
    let lowercase = beat.text.to_lowercase();
    let total_chars = lowercase.chars().count().max(1);
    let cue_count = beat.anchors.len();
    let mut cues: Vec<_> = beat
        .anchors
        .iter()
        .enumerate()
        .map(|(index, anchor)| {
            let phrase = anchor.phrase.to_lowercase();
            let offset = anchor
                .at_char
                .map(|character| {
                    duration_ms.saturating_mul(character.min(total_chars as u32) as u64)
                        / total_chars as u64
                })
                .or_else(|| {
                    lowercase
                        .find(&phrase)
                        .filter(|_| !phrase.is_empty())
                        .map(|byte_offset| {
                            duration_ms
                                .saturating_mul(lowercase[..byte_offset].chars().count() as u64)
                                / total_chars as u64
                        })
                })
                // Unknown phrases used to fall back to zero, so several model cues could fire at
                // playback start and overwrite one another. Preserve authored order instead.
                .unwrap_or_else(|| {
                    duration_ms.saturating_mul(index as u64 + 1) / (cue_count as u64 + 1)
                });
            (offset, anchor.cue.clone())
        })
        .collect();
    cues.sort_by_key(|(offset, _)| *offset);
    cues
}

struct QueuedAudioBeat {
    beat: NarrationBeat,
    samples: Vec<f32>,
    cursor: usize,
    duration_ms: u64,
    synthesis_ms: u64,
    started: bool,
    last_progress_frame: usize,
}

#[derive(Debug)]
enum CallbackEvent {
    Scheduled(ScheduledEvent),
    Warning(String),
    Error(String),
}

#[derive(Debug)]
struct ScheduledEvent {
    playback: StreamInstant,
    kind: ScheduledKind,
}

#[derive(Debug)]
enum ScheduledKind {
    Start {
        beat: NarrationBeat,
        duration_ms: u64,
        synthesis_ms: u64,
    },
    Progress {
        beat_id: String,
        position_ms: u64,
        duration_ms: u64,
    },
    End {
        beat_id: String,
    },
}

fn enqueue_audio(
    queue: &Arc<Mutex<VecDeque<QueuedAudioBeat>>>,
    prepared: PreparedBeat,
    output_rate: u32,
    recording_dir: Option<&Path>,
    events: &Sender<AudioEvent>,
) -> Result<()> {
    let (normalized, source_peak, gain) = normalize_for_playback(prepared.samples)?;
    let mut samples = resample_linear(&normalized, prepared.sample_rate, output_rate);
    if let Some(recording_dir) = recording_dir {
        fs::create_dir_all(recording_dir)?;
        let path = recording_dir.join(narration_file_name(&prepared.beat.id));
        write_pcm16_wav(&path, &samples, output_rate)?;
        events.send_blocking(AudioEvent::RecordingStored {
            beat_id: prepared.beat.id.clone(),
            path,
            duration_ms: samples.len() as u64 * 1_000 / u64::from(output_rate),
        })?;
    }
    let mut queue = queue
        .lock()
        .map_err(|_| anyhow!("continuous audio queue was poisoned"))?;
    blend_boundary(queue.back_mut(), &mut samples, output_rate);
    let duration_ms = samples.len() as u64 * 1_000 / output_rate as u64;
    eprintln!(
        "Foyer Shell audio: queued {} duration={}ms source_peak={source_peak:.3} gain={gain:.2}x",
        prepared.beat.id, duration_ms
    );
    queue.push_back(QueuedAudioBeat {
        beat: prepared.beat,
        samples,
        cursor: 0,
        duration_ms,
        synthesis_ms: prepared.synthesis_ms,
        started: false,
        last_progress_frame: usize::MAX,
    });
    Ok(())
}

fn blend_boundary(previous: Option<&mut QueuedAudioBeat>, next: &mut Vec<f32>, rate: u32) {
    let Some(previous) = previous else {
        return;
    };
    let requested = (rate as usize * 12 / 1_000).max(1);
    let remaining = previous.samples.len().saturating_sub(previous.cursor);
    let overlap = requested.min(remaining).min(next.len());
    if overlap < 2 {
        return;
    }
    let previous_start = previous.samples.len() - overlap;
    if previous_start < previous.cursor {
        return;
    }
    for index in 0..overlap {
        let progress = index as f32 / (overlap - 1) as f32;
        let fade_out = (1.0 - progress).sqrt();
        let fade_in = progress.sqrt();
        previous.samples[previous_start + index] =
            previous.samples[previous_start + index] * fade_out + next[index] * fade_in;
    }
    next.drain(..overlap);
}

fn build_continuous_stream<T>(
    device: &Device,
    config: StreamConfig,
    channels: usize,
    output_rate: u32,
    queue: Arc<Mutex<VecDeque<QueuedAudioBeat>>>,
    paused: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
    messages: mpsc::Sender<CallbackEvent>,
) -> Result<Stream>
where
    T: Sample + SizedSample + FromSample<f32>,
{
    let errors = messages.clone();
    let stream = device.build_output_stream(
        config,
        move |output: &mut [T], info: &OutputCallbackInfo| {
            let Ok(mut queue) = queue.try_lock() else {
                for sample in output {
                    *sample = T::from_sample(0.0);
                }
                return;
            };
            for (frame_index, frame) in output.chunks_mut(channels).enumerate() {
                if paused.load(Ordering::Relaxed) || stopped.load(Ordering::Relaxed) {
                    for sample in frame {
                        *sample = T::from_sample(0.0);
                    }
                    continue;
                }
                while queue
                    .front()
                    .is_some_and(|beat| beat.cursor >= beat.samples.len())
                {
                    let beat = queue.pop_front().expect("checked completed beat");
                    let playback = info.timestamp().playback
                        + Duration::from_secs_f64(frame_index as f64 / output_rate as f64);
                    let _ = messages.send(CallbackEvent::Scheduled(ScheduledEvent {
                        playback,
                        kind: ScheduledKind::End {
                            beat_id: beat.beat.id,
                        },
                    }));
                }
                let playback = info.timestamp().playback
                    + Duration::from_secs_f64(frame_index as f64 / output_rate as f64);
                let value = if let Some(beat) = queue.front_mut() {
                    if !beat.started {
                        beat.started = true;
                        let _ = messages.send(CallbackEvent::Scheduled(ScheduledEvent {
                            playback,
                            kind: ScheduledKind::Start {
                                beat: beat.beat.clone(),
                                duration_ms: beat.duration_ms,
                                synthesis_ms: beat.synthesis_ms,
                            },
                        }));
                    }
                    let progress_interval = (output_rate as usize / 50).max(1);
                    if beat.last_progress_frame == usize::MAX
                        || beat.cursor.saturating_sub(beat.last_progress_frame) >= progress_interval
                    {
                        beat.last_progress_frame = beat.cursor;
                        let _ = messages.send(CallbackEvent::Scheduled(ScheduledEvent {
                            playback,
                            kind: ScheduledKind::Progress {
                                beat_id: beat.beat.id.clone(),
                                position_ms: beat.cursor as u64 * 1_000 / output_rate as u64,
                                duration_ms: beat.duration_ms,
                            },
                        }));
                    }
                    let value = beat.samples[beat.cursor];
                    beat.cursor += 1;
                    value
                } else {
                    0.0
                };
                let value = T::from_sample(value);
                for sample in frame {
                    *sample = value;
                }
            }
        },
        move |error| {
            let message = error.to_string();
            let event = if matches!(
                error.kind(),
                ErrorKind::Xrun | ErrorKind::DeviceChanged | ErrorKind::RealtimeDenied
            ) {
                CallbackEvent::Warning(message)
            } else {
                CallbackEvent::Error(message)
            };
            let _ = errors.send(event);
        },
        None,
    )?;
    Ok(stream)
}

fn run_recorded(
    recordings: Vec<RecordedNarration>,
    controls: Receiver<AudioCommand>,
    events: &Sender<AudioEvent>,
) -> Result<()> {
    let (prepared_sender, prepared_receiver) = mpsc::sync_channel(2);
    let playback_events = events.clone();
    let playback = thread::Builder::new()
        .name("foyer-shell-recorded-playback".into())
        .spawn(move || {
            playback_loop(
                prepared_receiver,
                controls,
                &playback_events,
                "Recorded narration",
                None,
            )
        })
        .context("starting recorded playback")?;
    for recording in recordings {
        let (samples, sample_rate) = read_pcm16_wav(&recording.path)
            .with_context(|| format!("reading {}", recording.path.display()))?;
        if prepared_sender
            .send(PreparedBeat {
                beat: recording.beat,
                samples,
                sample_rate,
                synthesis_ms: 0,
            })
            .is_err()
        {
            break;
        }
    }
    drop(prepared_sender);
    playback
        .join()
        .map_err(|_| anyhow!("recorded playback thread panicked"))?
}

fn narration_file_name(beat_id: &str) -> String {
    let bounded = beat_id
        .chars()
        .take(96)
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    format!("{}.wav", if bounded.is_empty() { "beat" } else { &bounded })
}

fn write_pcm16_wav(path: &Path, samples: &[f32], sample_rate: u32) -> io::Result<()> {
    let temporary = path.with_extension("wav.tmp");
    let data_len = samples.len().saturating_mul(2).min(u32::MAX as usize) as u32;
    let mut file = fs::File::create(&temporary)?;
    file.write_all(b"RIFF")?;
    file.write_all(&(36_u32.saturating_add(data_len)).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16_u32.to_le_bytes())?;
    file.write_all(&1_u16.to_le_bytes())?;
    file.write_all(&1_u16.to_le_bytes())?;
    file.write_all(&sample_rate.to_le_bytes())?;
    file.write_all(&sample_rate.saturating_mul(2).to_le_bytes())?;
    file.write_all(&2_u16.to_le_bytes())?;
    file.write_all(&16_u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_len.to_le_bytes())?;
    for sample in samples.iter().take(data_len as usize / 2) {
        let pcm = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
        file.write_all(&pcm.to_le_bytes())?;
    }
    file.sync_all()?;
    fs::rename(temporary, path)
}

fn read_pcm16_wav(path: &Path) -> Result<(Vec<f32>, u32)> {
    let bytes = fs::read(path)?;
    if bytes.len() < 44 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        bail!("unsupported narration WAV");
    }
    let channels = u16::from_le_bytes(bytes[22..24].try_into()?) as usize;
    let sample_rate = u32::from_le_bytes(bytes[24..28].try_into()?);
    let bits = u16::from_le_bytes(bytes[34..36].try_into()?);
    if channels != 1 || bits != 16 || sample_rate == 0 || &bytes[36..40] != b"data" {
        bail!("narration WAV must be mono PCM16 with a canonical header");
    }
    let declared = u32::from_le_bytes(bytes[40..44].try_into()?) as usize;
    let end = 44_usize.saturating_add(declared).min(bytes.len());
    let samples = bytes[44..end]
        .chunks_exact(2)
        .map(|sample| i16::from_le_bytes([sample[0], sample[1]]) as f32 / 32_768.0)
        .collect();
    Ok((samples, sample_rate))
}

fn resample_linear(samples: &[f32], source_rate: u32, output_rate: u32) -> Vec<f32> {
    if samples.is_empty() || source_rate == output_rate {
        return samples.to_vec();
    }
    let output_len = (samples.len() as u64 * output_rate as u64 / source_rate as u64) as usize;
    let ratio = source_rate as f64 / output_rate as f64;
    (0..output_len)
        .map(|index| {
            let source_position = index as f64 * ratio;
            let left = source_position.floor() as usize;
            let right = (left + 1).min(samples.len() - 1);
            let fraction = (source_position - left as f64) as f32;
            samples[left] + (samples[right] - samples[left]) * fraction
        })
        .collect()
}

fn normalize_for_playback(mut samples: Vec<f32>) -> Result<(Vec<f32>, f32, f32)> {
    let peak = samples.iter().copied().map(f32::abs).fold(0.0, f32::max);
    if !peak.is_finite() || peak < 1.0e-5 {
        bail!("speech provider produced silent audio");
    }
    let gain = (0.82 / peak).clamp(1.0, 4.0);
    if gain > 1.0 {
        for sample in &mut samples {
            *sample = (*sample * gain).clamp(-0.98, 0.98);
        }
    }
    Ok((samples, peak, gain))
}

#[cfg(test)]
mod tests {
    use foyer_shell_protocol::NarrationAnchor;

    use super::*;

    #[test]
    fn narration_cues_resolve_against_the_audio_duration() {
        let beat = NarrationBeat {
            id: "beat".into(),
            text: "First evidence, then the cache key.".into(),
            style: Default::default(),
            style_degree: 1.0,
            focus: Vec::new(),
            anchors: vec![NarrationAnchor {
                phrase: "cache key".into(),
                at_char: None,
                cue: CueAction::Focus {
                    ids: vec!["cache".into()],
                },
            }],
        };
        let cues = resolve_cues(&beat, 4_000);
        assert_eq!(cues.len(), 1);
        assert!(cues[0].0 > 2_000);
        assert!(cues[0].0 < 4_000);
    }

    #[test]
    fn missing_anchor_phrases_are_spaced_instead_of_collapsing_at_start() {
        let beat = NarrationBeat {
            id: "beat".into(),
            text: "A short presentation.".into(),
            style: Default::default(),
            style_degree: 1.0,
            focus: Vec::new(),
            anchors: (0..3)
                .map(|index| NarrationAnchor {
                    phrase: format!("__missing-{index}__"),
                    at_char: None,
                    cue: CueAction::Focus {
                        ids: vec![format!("card-{index}")],
                    },
                })
                .collect(),
        };
        let cues = resolve_cues(&beat, 4_000);
        assert_eq!(
            cues.iter().map(|(offset, _)| *offset).collect::<Vec<_>>(),
            [1_000, 2_000, 3_000]
        );
    }

    #[test]
    fn exact_character_offsets_override_phrase_matching() {
        let beat = NarrationBeat {
            id: "beat".into(),
            text: "01234567890123456789".into(),
            style: Default::default(),
            style_degree: 1.0,
            focus: Vec::new(),
            anchors: vec![NarrationAnchor {
                phrase: "not present".into(),
                at_char: Some(5),
                cue: CueAction::Focus {
                    ids: vec!["second".into()],
                },
            }],
        };
        assert_eq!(resolve_cues(&beat, 4_000)[0].0, 1_000);
    }

    #[test]
    fn resampling_preserves_duration() {
        let source = vec![0.25; 24_000];
        let output = resample_linear(&source, 24_000, 48_000);
        assert_eq!(output.len(), 48_000);
        assert!(
            output
                .iter()
                .all(|sample| (*sample - 0.25).abs() < f32::EPSILON)
        );
    }

    #[test]
    fn retained_pcm_wav_round_trips_for_replay() {
        let path =
            env::temp_dir().join(format!("foyer-shell-audio-{}-test.wav", std::process::id()));
        let source = vec![0.0, 0.25, -0.5, 0.75];
        write_pcm16_wav(&path, &source, 24_000).unwrap();
        let (decoded, rate) = read_pcm16_wav(&path).unwrap();
        assert_eq!(rate, 24_000);
        assert_eq!(decoded.len(), source.len());
        for (left, right) in decoded.iter().zip(source) {
            assert!((left - right).abs() < 0.0001);
        }
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn prepared_boundaries_are_blended_without_adding_silence() {
        let mut previous = QueuedAudioBeat {
            beat: NarrationBeat {
                id: "one".into(),
                text: "one".into(),
                style: Default::default(),
                style_degree: 1.0,
                focus: Vec::new(),
                anchors: Vec::new(),
            },
            samples: vec![0.5; 1_000],
            cursor: 100,
            duration_ms: 1_000,
            synthesis_ms: 10,
            started: true,
            last_progress_frame: 0,
        };
        let mut next = vec![0.25; 1_000];
        blend_boundary(Some(&mut previous), &mut next, 1_000);
        assert_eq!(previous.samples.len(), 1_000);
        assert_eq!(next.len(), 988);
        assert!((previous.samples[988] - 0.5).abs() < 0.001);
        assert!((previous.samples[999] - 0.25).abs() < 0.001);
    }

    #[test]
    fn quiet_audio_is_boosted_without_clipping() {
        let (samples, peak, gain) = normalize_for_playback(vec![0.05, -0.10, 0.08]).unwrap();
        assert_eq!(peak, 0.10);
        assert_eq!(gain, 4.0);
        assert!(samples.iter().all(|sample| sample.abs() <= 0.98));
        assert!(samples[1].abs() > 0.39);
    }

    #[test]
    fn openrouter_pcm_is_decoded_as_little_endian_sixteen_bit_audio() {
        let samples = decode_pcm16(&[0x00, 0x40, 0x00, 0xc0]).unwrap();
        assert!((samples[0] - 0.5).abs() < 0.0001);
        assert!((samples[1] + 0.5).abs() < 0.0001);
    }

    #[test]
    fn portable_delivery_styles_use_mai_voice_names() {
        assert_eq!(azure_style(NarrationStyle::Excited), Some("excited"));
        assert_eq!(azure_style(NarrationStyle::Softvoice), Some("softvoice"));
        assert_eq!(azure_style(NarrationStyle::Neutral), None);
    }

    #[test]
    fn azure_style_options_are_only_sent_to_mai_models() {
        assert!(uses_azure_style("microsoft/mai-voice-2-flash"));
        assert!(!uses_azure_style("openai/gpt-4o-mini-tts-2025-12-15"));
    }
}
