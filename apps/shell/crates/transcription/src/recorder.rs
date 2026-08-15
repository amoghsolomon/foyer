use std::{
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, bail};
use cpal::{
    Device, FromSample, Sample, SampleFormat, Stream, StreamConfig,
    traits::{DeviceTrait as _, HostTrait as _, StreamTrait as _},
};

use crate::{WAVEFORM_SAMPLES, service::EngineEvent};

const TARGET_RATE: u32 = 16_000;
const TELEMETRY_INTERVAL: Duration = Duration::from_millis(45);
const WAVEFORM_DISPLAY_GAIN: f32 = 8.0;

struct Capture {
    samples: Vec<f32>,
    last_telemetry: Instant,
}

pub struct Recorder {
    stream: Option<Stream>,
    capture: Arc<Mutex<Capture>>,
    sample_rate: u32,
}

impl Recorder {
    pub fn start(session_id: u64, events: mpsc::Sender<EngineEvent>) -> Result<Self> {
        let device = cpal::default_host()
            .default_input_device()
            .context("no default microphone is available")?;
        let supported = device
            .default_input_config()
            .context("query default microphone format")?;
        let sample_format = supported.sample_format();
        let config: StreamConfig = supported.into();
        let channels = config.channels as usize;
        let sample_rate = config.sample_rate;
        let capture = Arc::new(Mutex::new(Capture {
            samples: Vec::new(),
            last_telemetry: Instant::now() - TELEMETRY_INTERVAL,
        }));
        let stream = match sample_format {
            SampleFormat::F32 => build_stream::<f32>(
                &device,
                &config,
                channels,
                session_id,
                capture.clone(),
                events,
            )?,
            SampleFormat::I16 => build_stream::<i16>(
                &device,
                &config,
                channels,
                session_id,
                capture.clone(),
                events,
            )?,
            SampleFormat::U16 => build_stream::<u16>(
                &device,
                &config,
                channels,
                session_id,
                capture.clone(),
                events,
            )?,
            SampleFormat::I32 => build_stream::<i32>(
                &device,
                &config,
                channels,
                session_id,
                capture.clone(),
                events,
            )?,
            format => bail!("unsupported microphone sample format {format}"),
        };
        stream.play().context("start microphone capture")?;
        Ok(Self {
            stream: Some(stream),
            capture,
            sample_rate,
        })
    }

    pub fn stop(mut self) -> Result<Vec<f32>> {
        self.stream.take();
        let samples = self
            .capture
            .lock()
            .map_err(|_| anyhow::anyhow!("microphone sample buffer is poisoned"))?
            .samples
            .split_off(0);
        Ok(resample_linear(&samples, self.sample_rate, TARGET_RATE))
    }

    pub fn abort(mut self) {
        self.stream.take();
        if let Ok(mut capture) = self.capture.lock() {
            capture.samples.clear();
        }
    }
}

fn build_stream<T>(
    device: &Device,
    config: &StreamConfig,
    channels: usize,
    session_id: u64,
    capture: Arc<Mutex<Capture>>,
    events: mpsc::Sender<EngineEvent>,
) -> Result<Stream>
where
    T: Sample + cpal::SizedSample,
    f32: FromSample<T>,
{
    let error_events = events.clone();
    device
        .build_input_stream(
            *config,
            move |data: &[T], _| {
                let mono = data
                    .chunks(channels)
                    .map(|frame| {
                        frame.iter().copied().map(f32::from_sample).sum::<f32>() / channels as f32
                    })
                    .collect::<Vec<_>>();
                let Ok(mut capture) = capture.lock() else {
                    return;
                };
                capture.samples.extend_from_slice(&mono);
                if capture.last_telemetry.elapsed() < TELEMETRY_INTERVAL {
                    return;
                }
                capture.last_telemetry = Instant::now();
                let rms = if mono.is_empty() {
                    0.0
                } else {
                    (mono.iter().map(|sample| sample * sample).sum::<f32>() / mono.len() as f32)
                        .sqrt()
                };
                let level = ((20.0 * rms.max(1e-6).log10() + 60.0) / 50.0).clamp(0.0, 1.0);
                let waveform = waveform_telemetry(&mono);
                let _ = events.send(EngineEvent::Waveform {
                    session_id,
                    rms: level,
                    samples: waveform,
                });
            },
            move |error| {
                let _ = error_events.send(EngineEvent::CaptureFailed {
                    session_id,
                    error: error.to_string(),
                });
            },
            None,
        )
        .context("open microphone stream")
}

fn waveform_telemetry(mono: &[f32]) -> [f32; WAVEFORM_SAMPLES] {
    let mut waveform = [0.0; WAVEFORM_SAMPLES];
    if mono.is_empty() {
        return waveform;
    }

    for (index, target) in waveform.iter_mut().enumerate() {
        let start = index * mono.len() / WAVEFORM_SAMPLES;
        let end = ((index + 1) * mono.len() / WAVEFORM_SAMPLES)
            .max(start + 1)
            .min(mono.len());
        if start >= end {
            continue;
        }

        // Averaging a speech-frequency bucket tends toward zero and turns the display into an
        // RMS meter. Keep the strongest signed excursion so the bounded telemetry retains the
        // waveform's alternating shape without transporting raw audio.
        let peak = mono[start..end]
            .iter()
            .copied()
            .max_by(|left, right| left.abs().total_cmp(&right.abs()))
            .unwrap_or(0.0);
        *target = (peak * WAVEFORM_DISPLAY_GAIN).clamp(-1.0, 1.0);
    }
    waveform
}

fn resample_linear(samples: &[f32], source_rate: u32, target_rate: u32) -> Vec<f32> {
    if samples.is_empty() || source_rate == target_rate {
        return samples.to_vec();
    }
    let target_len = ((samples.len() as u64 * target_rate as u64) / source_rate as u64) as usize;
    let scale = source_rate as f64 / target_rate as f64;
    (0..target_len)
        .map(|index| {
            let position = index as f64 * scale;
            let left = position.floor() as usize;
            let right = (left + 1).min(samples.len() - 1);
            let fraction = (position - left as f64) as f32;
            samples[left] * (1.0 - fraction) + samples[right] * fraction
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampler_preserves_duration_and_endpoints() {
        let output = resample_linear(&[0.0, 1.0, 0.0, -1.0], 4, 8);
        assert_eq!(output.len(), 8);
        assert_eq!(output[0], 0.0);
        assert_eq!(output[2], 1.0);
    }

    #[test]
    fn waveform_telemetry_preserves_signed_peaks_instead_of_averaging_them_away() {
        let mut input = vec![0.0; WAVEFORM_SAMPLES * 4];
        input[0] = -0.09;
        input[1] = 0.08;
        input[4] = 0.03;
        input[5] = -0.12;

        let waveform = waveform_telemetry(&input);

        assert_eq!(waveform[0], -0.72);
        assert_eq!(waveform[1], -0.96);
    }

    #[test]
    fn waveform_telemetry_is_bounded() {
        let waveform = waveform_telemetry(&vec![0.5; WAVEFORM_SAMPLES]);
        assert!(waveform.iter().all(|sample| *sample == 1.0));
    }
}
