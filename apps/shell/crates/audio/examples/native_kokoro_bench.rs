use std::{env, path::PathBuf, time::Instant};

use foyer_shell_audio::native::NativeKokoro;

fn proc_kib(field: &str) -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find(|line| line.starts_with(field))?
                .split_whitespace()
                .nth(1)?
                .parse()
                .ok()
        })
        .unwrap_or_default()
}

fn main() -> anyhow::Result<()> {
    let root = PathBuf::from(env::var("FOYER_SHELL_ROOT").unwrap_or_else(|_| ".".into()));
    let load_started = Instant::now();
    let mut kokoro = NativeKokoro::load(
        &root.join(".foyer-shell/models/kokoro/kokoro-v1.0.onnx"),
        &root.join(".foyer-shell/models/kokoro/voices-v1.0.bin"),
        "af_bella",
    )?;
    println!(
        "ready load_ms={} rss_kib={} hwm_kib={}",
        load_started.elapsed().as_millis(),
        proc_kib("VmRSS:"),
        proc_kib("VmHWM:"),
    );

    let text = "Two pieces of evidence converge on the cache key.";
    for run in 1..=3 {
        let started = Instant::now();
        let output = kokoro.synthesize(text, "en-gb", 1.0)?;
        println!(
            "run={run} total_ms={} phonemize_ms={} inference_ms={} samples={} audio_ms={} rtf={:.3} rss_kib={} hwm_kib={} phonemes={:?}",
            started.elapsed().as_millis(),
            output.phonemization.as_millis(),
            output.inference.as_millis(),
            output.samples.len(),
            output.samples.len() as u64 * 1000 / output.sample_rate as u64,
            output.inference.as_secs_f64()
                / (output.samples.len() as f64 / output.sample_rate as f64),
            proc_kib("VmRSS:"),
            proc_kib("VmHWM:"),
            output.phonemes,
        );
    }
    Ok(())
}
