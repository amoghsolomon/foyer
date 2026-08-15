use foyer_shell_transcription::Config;
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().without_time())
        .with(
            tracing_subscriber::EnvFilter::from_default_env().add_directive(
                "foyer_shell_transcription=info"
                    .parse()
                    .expect("valid tracing directive"),
            ),
        )
        .try_init()
        .ok();
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if let [operation, path] = arguments.as_slice()
        && operation == "--transcribe-wav"
    {
        let mut recognizer =
            foyer_shell_transcription::recognizer::Recognizer::load(&Config::from_env())?;
        println!("{}", recognizer.transcribe(load_wav(path)?)?);
        return Ok(());
    }
    if !arguments.is_empty() {
        anyhow::bail!("expected no arguments or --transcribe-wav <path>");
    }
    foyer_shell_transcription::service::run(Config::from_env())
}

fn load_wav(path: &str) -> anyhow::Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let interleaved = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<Vec<_>, _>>()?,
        hound::SampleFormat::Int if spec.bits_per_sample <= 16 => reader
            .samples::<i16>()
            .map(|sample| sample.map(|sample| sample as f32 / 32_768.0))
            .collect::<Result<Vec<_>, _>>()?,
        hound::SampleFormat::Int => reader
            .samples::<i32>()
            .map(|sample| sample.map(|sample| sample as f32 / 2_147_483_648.0))
            .collect::<Result<Vec<_>, _>>()?,
    };
    let channels = spec.channels as usize;
    let mono = interleaved
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect::<Vec<_>>();
    anyhow::ensure!(!mono.is_empty(), "WAV file contains no samples");
    Ok(resample(&mono, spec.sample_rate, 16_000))
}

fn resample(samples: &[f32], source_rate: u32, target_rate: u32) -> Vec<f32> {
    if source_rate == target_rate {
        return samples.to_vec();
    }
    let target_len = samples.len() * target_rate as usize / source_rate as usize;
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
