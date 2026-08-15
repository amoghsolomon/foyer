//! CPU-only Kokoro inference through ONNX Runtime's Rust API.

use std::{
    fs::File,
    io::Read,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use ort::{ep, inputs, session::Session, value::Tensor};

const SAMPLE_RATE: u32 = 24_000;
const STYLE_WIDTH: usize = 256;
const MAX_PHONEMES: usize = 510;

const VOCAB: &[(char, i64)] = &[
    (';', 1),
    (':', 2),
    (',', 3),
    ('.', 4),
    ('!', 5),
    ('?', 6),
    ('—', 9),
    ('…', 10),
    ('"', 11),
    ('(', 12),
    (')', 13),
    ('“', 14),
    ('”', 15),
    (' ', 16),
    ('\u{0303}', 17),
    ('ʣ', 18),
    ('ʥ', 19),
    ('ʦ', 20),
    ('ʨ', 21),
    ('ᵝ', 22),
    ('\u{ab67}', 23),
    ('A', 24),
    ('I', 25),
    ('O', 31),
    ('Q', 33),
    ('S', 35),
    ('T', 36),
    ('W', 39),
    ('Y', 41),
    ('ᵊ', 42),
    ('a', 43),
    ('b', 44),
    ('c', 45),
    ('d', 46),
    ('e', 47),
    ('f', 48),
    ('h', 50),
    ('i', 51),
    ('j', 52),
    ('k', 53),
    ('l', 54),
    ('m', 55),
    ('n', 56),
    ('o', 57),
    ('p', 58),
    ('q', 59),
    ('r', 60),
    ('s', 61),
    ('t', 62),
    ('u', 63),
    ('v', 64),
    ('w', 65),
    ('x', 66),
    ('y', 67),
    ('z', 68),
    ('ɑ', 69),
    ('ɐ', 70),
    ('ɒ', 71),
    ('æ', 72),
    ('β', 75),
    ('ɔ', 76),
    ('ɕ', 77),
    ('ç', 78),
    ('ɖ', 80),
    ('ð', 81),
    ('ʤ', 82),
    ('ə', 83),
    ('ɚ', 85),
    ('ɛ', 86),
    ('ɜ', 87),
    ('ɟ', 90),
    ('ɡ', 92),
    ('ɥ', 99),
    ('ɨ', 101),
    ('ɪ', 102),
    ('ʝ', 103),
    ('ɯ', 110),
    ('ɰ', 111),
    ('ŋ', 112),
    ('ɳ', 113),
    ('ɲ', 114),
    ('ɴ', 115),
    ('ø', 116),
    ('ɸ', 118),
    ('θ', 119),
    ('œ', 120),
    ('ɹ', 123),
    ('ɾ', 125),
    ('ɻ', 126),
    ('ʁ', 128),
    ('ɽ', 129),
    ('ʂ', 130),
    ('ʃ', 131),
    ('ʈ', 132),
    ('ʧ', 133),
    ('ʊ', 135),
    ('ʋ', 136),
    ('ʌ', 138),
    ('ɣ', 139),
    ('ɤ', 140),
    ('χ', 142),
    ('ʎ', 143),
    ('ʒ', 147),
    ('ʔ', 148),
    ('ˈ', 156),
    ('ˌ', 157),
    ('ː', 158),
    ('ʰ', 162),
    ('ʲ', 164),
    ('↓', 169),
    ('→', 171),
    ('↗', 172),
    ('↘', 173),
    ('ᵻ', 177),
];

#[derive(Debug)]
pub struct Synthesis {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub phonemes: String,
    pub phonemization: Duration,
    pub inference: Duration,
}

pub struct NativeKokoro {
    session: Session,
    voice_rows: Vec<f32>,
    voice_row_count: usize,
    phonemizer: String,
}

impl NativeKokoro {
    pub fn load(model: &Path, voices: &Path, voice: &str) -> Result<Self> {
        ort::init()
            .with_name("foyer-shell-kokoro")
            .with_execution_providers([ep::CPU::default().build()])
            .commit();

        let session = Session::builder()?
            // Leave two physical cores available for GPUI and the real-time audio callback on the
            // target six-core laptop. Unbounded ORT inference can otherwise starve CPAL.
            .with_intra_threads(4)
            .map_err(|error| anyhow!(error.to_string()))?
            .with_inter_threads(1)
            .map_err(|error| anyhow!(error.to_string()))?
            .with_execution_providers([ep::CPU::default().build()])
            .map_err(|error| anyhow!(error.to_string()))?
            .commit_from_file(model)
            .with_context(|| format!("loading Kokoro model {}", model.display()))?;
        let (voice_rows, voice_row_count) = load_voice(voices, voice)?;

        let phonemizer = std::env::var("FOYER_SHELL_ESPEAK").unwrap_or_else(|_| "espeak-ng".into());
        let status = Command::new(&phonemizer)
            .arg("--version")
            .output()
            .with_context(|| format!("starting phonemizer {phonemizer:?}"))?;
        ensure!(
            status.status.success(),
            "phonemizer {phonemizer:?} is unavailable"
        );

        Ok(Self {
            session,
            voice_rows,
            voice_row_count,
            phonemizer,
        })
    }

    pub fn synthesize(&mut self, text: &str, language: &str, speed: f32) -> Result<Synthesis> {
        ensure!(
            (0.5..=2.0).contains(&speed),
            "speed must be between 0.5 and 2.0"
        );
        let phoneme_started = Instant::now();
        let phonemes = self.phonemize(text, language)?;
        let phonemization = phoneme_started.elapsed();
        let tokens = tokenize(&phonemes);
        ensure!(!tokens.is_empty(), "phonemizer produced no Kokoro tokens");
        ensure!(
            tokens.len() <= MAX_PHONEMES,
            "narration exceeds {MAX_PHONEMES} phonemes"
        );
        ensure!(
            tokens.len() < self.voice_row_count,
            "voice has no style row for {} tokens",
            tokens.len()
        );

        let row_start = tokens.len() * STYLE_WIDTH;
        let style = self.voice_rows[row_start..row_start + STYLE_WIDTH].to_vec();
        let mut padded_tokens = Vec::with_capacity(tokens.len() + 2);
        padded_tokens.push(0);
        padded_tokens.extend(tokens);
        padded_tokens.push(0);

        let token_count = padded_tokens.len();
        let inference_started = Instant::now();
        let outputs = self.session.run(inputs![
            "tokens" => Tensor::from_array(([1, token_count], padded_tokens))?,
            "style" => Tensor::from_array(([1, STYLE_WIDTH], style))?,
            "speed" => Tensor::from_array(([1], vec![speed]))?,
        ])?;
        let (_, output) = outputs["audio"].try_extract_tensor::<f32>()?;
        let samples = trim_silence(output);
        let inference = inference_started.elapsed();

        Ok(Synthesis {
            samples,
            sample_rate: SAMPLE_RATE,
            phonemes,
            phonemization,
            inference,
        })
    }

    fn phonemize(&self, text: &str, language: &str) -> Result<String> {
        let output = Command::new(&self.phonemizer)
            .args(["-q", "--ipa=3", "-v", language, text.trim()])
            .output()
            .with_context(|| format!("running phonemizer {:?}", self.phonemizer))?;
        if !output.status.success() {
            bail!(
                "phonemizer failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let raw = String::from_utf8(output.stdout).context("phonemizer returned non-UTF-8 IPA")?;
        Ok(raw
            .chars()
            .filter(|character| vocab_id(*character).is_some())
            .collect())
    }
}

fn vocab_id(character: char) -> Option<i64> {
    VOCAB
        .iter()
        .find_map(|(candidate, id)| (*candidate == character).then_some(*id))
}

fn tokenize(phonemes: &str) -> Vec<i64> {
    phonemes.chars().filter_map(vocab_id).collect()
}

fn load_voice(path: &Path, voice: &str) -> Result<(Vec<f32>, usize)> {
    let file =
        File::open(path).with_context(|| format!("opening voice archive {}", path.display()))?;
    let mut archive = zip::ZipArchive::new(file).context("reading voice NPZ archive")?;
    let entry_name = format!("{voice}.npy");
    let mut entry = archive
        .by_name(&entry_name)
        .with_context(|| format!("voice {voice:?} not found"))?;
    let mut npy = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut npy)?;
    parse_voice_npy(&npy)
}

fn parse_voice_npy(npy: &[u8]) -> Result<(Vec<f32>, usize)> {
    ensure!(
        npy.starts_with(b"\x93NUMPY"),
        "voice entry is not an NPY array"
    );
    ensure!(npy.len() >= 12, "truncated NPY header");
    let major = npy[6];
    let (header_len, header_start) = match major {
        1 => (u16::from_le_bytes([npy[8], npy[9]]) as usize, 10),
        2 | 3 => (
            u32::from_le_bytes([npy[8], npy[9], npy[10], npy[11]]) as usize,
            12,
        ),
        version => return Err(anyhow!("unsupported NPY version {version}")),
    };
    let data_start = header_start + header_len;
    ensure!(data_start <= npy.len(), "truncated NPY payload");
    let header = std::str::from_utf8(&npy[header_start..data_start])?;
    ensure!(
        header.contains("'<f4'"),
        "voice NPY must contain little-endian float32 values"
    );
    ensure!(
        header.contains("False"),
        "Fortran-ordered voice arrays are unsupported"
    );

    let payload = &npy[data_start..];
    ensure!(
        payload.len().is_multiple_of(4),
        "voice payload is not aligned to float32"
    );
    let values: Vec<f32> = payload
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes(bytes.try_into().expect("four-byte chunk")))
        .collect();
    ensure!(
        values.len().is_multiple_of(STYLE_WIDTH),
        "voice style width is not {STYLE_WIDTH}"
    );
    let rows = values.len() / STYLE_WIDTH;
    ensure!(
        rows >= MAX_PHONEMES,
        "voice contains only {rows} style rows"
    );
    Ok((values, rows))
}

fn trim_silence(samples: &[f32]) -> Vec<f32> {
    const FRAME: usize = 2048;
    const HOP: usize = 512;
    if samples.is_empty() {
        return Vec::new();
    }

    let pad = FRAME / 2;
    let padded_len = samples.len() + FRAME;
    let frame_count = 1 + padded_len.saturating_sub(FRAME) / HOP;
    let mut rms = Vec::with_capacity(frame_count);
    for frame_index in 0..frame_count {
        let padded_start = frame_index * HOP;
        let mut energy = 0.0f32;
        for offset in 0..FRAME {
            let padded_index = padded_start + offset;
            let value = if padded_index < pad || padded_index >= pad + samples.len() {
                0.0
            } else {
                samples[padded_index - pad]
            };
            energy += value * value;
        }
        rms.push((energy / FRAME as f32).sqrt());
    }

    let reference = rms.iter().copied().fold(0.0f32, f32::max).max(1e-5);
    let threshold = reference * 10.0f32.powf(-60.0 / 20.0);
    let first = rms.iter().position(|value| *value > threshold);
    let last = rms.iter().rposition(|value| *value > threshold);
    match (first, last) {
        (Some(first), Some(last)) => {
            let start = (first * HOP).min(samples.len());
            let end = ((last + 1) * HOP).min(samples.len());
            samples[start..end].to_vec()
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vocab_matches_the_reference_smoke_tokens() {
        let phonemes = "tˈuː pˈiːsᵻz ʌv ˈɛvɪdəns kənvˈɜːdʒ ɔnðə kˈæʃ kˈiː.";
        assert_eq!(
            tokenize(phonemes),
            vec![
                62, 156, 63, 158, 16, 58, 156, 51, 158, 61, 177, 68, 16, 138, 64, 16, 156, 86, 64,
                102, 46, 83, 56, 61, 16, 53, 83, 56, 64, 156, 87, 158, 46, 147, 16, 76, 56, 81, 83,
                16, 53, 156, 72, 131, 16, 53, 156, 51, 158, 4
            ]
        );
    }
}
