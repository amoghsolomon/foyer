//! Reusable client and runtime primitives for Foyer Shell speech recognition.

mod client;
mod paste;
mod paths;

#[cfg(feature = "service")]
pub mod ngram;
#[cfg(feature = "service")]
pub mod recognizer;
#[cfg(feature = "service")]
pub mod recorder;
#[cfg(feature = "service")]
pub mod service;

pub use client::{Controller, Runtime, start};
pub use paste::copy_and_paste;
pub use paths::Config;

use std::sync::Arc;

pub const BUS_NAME: &str = "org.amazity.FoyerShell.Transcription1";
pub const OBJECT_PATH: &str = "/org/amazity/FoyerShell/Transcription1";
pub const INTERFACE: &str = "org.amazity.FoyerShell.Transcription1";
pub const WAVEFORM_SAMPLES: usize = 32;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Unavailable,
    Idle,
    Recording,
    LoadingModel,
    Transcribing,
    Ready,
    Error,
}

impl State {
    pub fn as_wire(&self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::Idle => "idle",
            Self::Recording => "recording",
            Self::LoadingModel => "loading-model",
            Self::Transcribing => "transcribing",
            Self::Ready => "ready",
            Self::Error => "error",
        }
    }

    pub fn from_wire(value: &str) -> Self {
        match value {
            "idle" => Self::Idle,
            "recording" => Self::Recording,
            "loading-model" => Self::LoadingModel,
            "transcribing" => Self::Transcribing,
            "ready" => Self::Ready,
            "error" => Self::Error,
            _ => Self::Unavailable,
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(
            self,
            Self::Recording | Self::LoadingModel | Self::Transcribing
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub generation: u64,
    pub state: State,
    pub session_id: u64,
    pub channel: Arc<str>,
    pub rms: f32,
    pub waveform: Arc<[f32; WAVEFORM_SAMPLES]>,
    pub transcript: Arc<str>,
    pub error: Arc<str>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            generation: 0,
            state: State::Unavailable,
            session_id: 0,
            channel: Arc::from(""),
            rms: 0.0,
            waveform: Arc::new([0.0; WAVEFORM_SAMPLES]),
            transcript: Arc::from(""),
            error: Arc::from("Transcription service is unavailable"),
        }
    }
}

pub type SnapshotWire = (u64, String, u64, String, f64, Vec<f64>, String, String);

impl Snapshot {
    pub fn from_wire(wire: SnapshotWire) -> Self {
        let (generation, state, session_id, channel, rms, waveform, transcript, error) = wire;
        let mut samples = [0.0; WAVEFORM_SAMPLES];
        for (target, source) in samples.iter_mut().zip(waveform) {
            *target = source.clamp(-1.0, 1.0) as f32;
        }
        Self {
            generation,
            state: State::from_wire(&state),
            session_id,
            channel: Arc::from(channel),
            rms: (rms as f32).clamp(0.0, 1.0),
            waveform: Arc::new(samples),
            transcript: Arc::from(transcript),
            error: Arc::from(error),
        }
    }

    pub fn to_wire(&self) -> SnapshotWire {
        (
            self.generation,
            self.state.as_wire().into(),
            self.session_id,
            self.channel.to_string(),
            self.rms.into(),
            self.waveform
                .iter()
                .map(|sample| f64::from(*sample))
                .collect(),
            self.transcript.to_string(),
            self.error.to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_snapshot_bounds_waveform_telemetry() {
        let snapshot = Snapshot::from_wire((
            7,
            "recording".into(),
            4,
            "dictation".into(),
            4.0,
            vec![2.0, -2.0],
            String::new(),
            String::new(),
        ));
        assert_eq!(snapshot.state, State::Recording);
        assert_eq!(snapshot.rms, 1.0);
        assert_eq!(snapshot.waveform[0], 1.0);
        assert_eq!(snapshot.waveform[1], -1.0);
        assert_eq!(snapshot.waveform[2], 0.0);
    }
}
