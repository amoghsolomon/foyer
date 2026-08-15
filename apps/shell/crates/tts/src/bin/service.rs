use std::{env, fs, io::Write as _, path::Path};

use anyhow::{Context as _, Result, bail};
use foyer_shell_tts::{Client, Config};
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().without_time())
        .with(
            tracing_subscriber::EnvFilter::from_default_env().add_directive(
                "foyer_shell_tts=info"
                    .parse()
                    .expect("valid tracing directive"),
            ),
        )
        .try_init()
        .ok();
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    match arguments.as_slice() {
        [] => foyer_shell_tts::service::run(Config::from_env()),
        [operation] if operation == "status" => {
            let (state, voice, load_ms, error) = Client::connect()?.status()?;
            println!("state={state} voice={voice} load_ms={load_ms} error={error}");
            Ok(())
        }
        [operation, channel, output, text @ ..]
            if operation == "synthesize" && !text.is_empty() =>
        {
            let text = text.join(" ");
            let synthesis = Client::connect()?.synthesize(channel, &text, "neutral", 1.0)?;
            write_pcm16_wav(Path::new(output), &synthesis.samples, synthesis.sample_rate)?;
            println!(
                "output={output} sample_rate={} audio_ms={} synthesis_ms={}",
                synthesis.sample_rate,
                synthesis.samples.len() as u64 * 1_000 / u64::from(synthesis.sample_rate),
                synthesis.synthesis_ms
            );
            Ok(())
        }
        _ => bail!("expected no arguments, status, or synthesize <channel> <output.wav> <text>"),
    }
}

fn write_pcm16_wav(path: &Path, samples: &[f32], sample_rate: u32) -> Result<()> {
    let data_len = samples.len().saturating_mul(2).min(u32::MAX as usize) as u32;
    let mut file = fs::File::create(path).with_context(|| format!("create {}", path.display()))?;
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
        let pcm = ((*sample).clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
        file.write_all(&pcm.to_le_bytes())?;
    }
    Ok(())
}
