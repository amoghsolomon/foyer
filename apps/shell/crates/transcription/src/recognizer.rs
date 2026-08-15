use std::f32::consts::PI;

use anyhow::{Context as _, Result, ensure};
use ndarray::{Array2, Array3, s};
use parakeet_rs::{
    ExecutionConfig, ExecutionProvider, FeatureCache, ParakeetUnifiedModel, PreprocessorConfig,
    SentencePieceVocab, UnifiedModelConfig,
};

use crate::{Config, ngram::NGramModel};

const SAMPLE_RATE: usize = 16_000;
const FEATURE_SIZE: usize = 128;
const HOP_LENGTH: usize = 160;
const N_FFT: usize = 512;
const WIN_LENGTH: usize = 400;
const PREEMPHASIS: f32 = 0.97;
const DECODER_LSTM_DIM: usize = 640;
const DECODER_LSTM_LAYERS: usize = 2;
const MAX_SYMBOLS_PER_STEP: usize = 10;
const MIN_RMS: f32 = 0.0005;

pub struct Recognizer {
    model: ParakeetUnifiedModel,
    vocab: SentencePieceVocab,
    feature_cache: FeatureCache,
    ngram: NGramModel,
    ngram_alpha: f32,
    blank_id: usize,
}

impl Recognizer {
    pub fn load(config: &Config) -> Result<Self> {
        let vocab = SentencePieceVocab::from_file(config.model_dir.join("tokenizer.model"))
            .context("load Parakeet tokenizer")?;
        let blank_id = vocab.size();
        let execution_provider = match config.execution_provider.as_str() {
            "cpu" => ExecutionProvider::Cpu,
            "cuda" => ExecutionProvider::Cuda,
            provider => anyhow::bail!("unsupported execution provider {provider:?}"),
        };
        let execution = ExecutionConfig::new()
            .with_execution_provider(execution_provider)
            .with_intra_threads(4)
            .with_inter_threads(1);
        let model = ParakeetUnifiedModel::from_pretrained(
            &config.model_dir,
            execution,
            UnifiedModelConfig {
                vocab_size: blank_id + 1,
                blank_id,
                decoder_lstm_dim: DECODER_LSTM_DIM,
                decoder_lstm_layers: DECODER_LSTM_LAYERS,
                subsampling_factor: 8,
            },
        )
        .context("load Parakeet Unified ONNX model")?;
        let ngram = NGramModel::load(&config.ngram_path, blank_id)?;
        let preprocessor = PreprocessorConfig {
            feature_extractor_type: "ParakeetFeatureExtractor".into(),
            feature_size: FEATURE_SIZE,
            hop_length: HOP_LENGTH,
            n_fft: N_FFT,
            padding_side: "right".into(),
            padding_value: 0.0,
            preemphasis: PREEMPHASIS,
            processor_class: "ParakeetProcessor".into(),
            return_attention_mask: true,
            sampling_rate: SAMPLE_RATE,
            win_length: WIN_LENGTH,
        };
        Ok(Self {
            model,
            vocab,
            feature_cache: FeatureCache::from_config(&preprocessor),
            ngram,
            ngram_alpha: config.ngram_alpha,
            blank_id,
        })
    }

    pub fn transcribe(&mut self, audio: Vec<f32>) -> Result<String> {
        ensure!(
            audio.len() >= SAMPLE_RATE / 5,
            "recording was too short to transcribe"
        );
        let rms =
            (audio.iter().map(|sample| sample * sample).sum::<f32>() / audio.len() as f32).sqrt();
        if rms < MIN_RMS {
            return Ok(String::new());
        }

        let features = extract_features(audio, &self.feature_cache)?;
        let (encoded, encoded_len) = self.model.run_encoder(&features)?;
        let frame_count = (encoded_len as usize).min(encoded.shape()[2]);
        let hidden_dim = encoded.shape()[1];
        let mut state_1 = Array3::zeros((DECODER_LSTM_LAYERS, 1, DECODER_LSTM_DIM));
        let mut state_2 = Array3::zeros((DECODER_LSTM_LAYERS, 1, DECODER_LSTM_DIM));
        let mut last_token = self.blank_id as i32;
        let mut lm_state = self.ngram.initial_state();
        let mut lm_scores = vec![0.0; self.ngram.vocab_size()];
        let mut lm_next_states = vec![0; self.ngram.vocab_size()];
        let mut tokens = Vec::new();

        for frame_index in 0..frame_count {
            let frame = encoded
                .slice(s![0, .., frame_index])
                .to_owned()
                .to_shape((1, hidden_dim, 1))
                .context("reshape Parakeet encoder frame")?
                .to_owned();
            for _ in 0..MAX_SYMBOLS_PER_STEP {
                let (logits, new_state_1, new_state_2) = self
                    .model
                    .run_decoder(&frame, last_token, &state_1, &state_2)?;
                ensure!(
                    logits.len() == self.blank_id + 1,
                    "unexpected decoder vocabulary size"
                );
                self.ngram
                    .advance(lm_state, &mut lm_scores, &mut lm_next_states)?;
                let token = fused_argmax(
                    logits
                        .as_slice()
                        .context("decoder logits are not contiguous")?,
                    &lm_scores,
                    self.blank_id,
                    self.ngram_alpha,
                );
                if token == self.blank_id {
                    break;
                }
                tokens.push(token);
                last_token = token as i32;
                state_1 = new_state_1;
                state_2 = new_state_2;
                lm_state = lm_next_states[token];
            }
        }
        Ok(self.vocab.decode(&tokens).trim().to_string())
    }
}

fn fused_argmax(logits: &[f32], lm_scores: &[f32], blank_id: usize, alpha: f32) -> usize {
    logits
        .iter()
        .enumerate()
        .max_by(|(left_index, left), (right_index, right)| {
            let left = **left
                + if *left_index == blank_id {
                    0.0
                } else {
                    alpha * lm_scores[*left_index]
                };
            let right = **right
                + if *right_index == blank_id {
                    0.0
                } else {
                    alpha * lm_scores[*right_index]
                };
            left.partial_cmp(&right)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(index, _)| index)
        .unwrap_or(blank_id)
}

// This mirrors NeMo's Parakeet preprocessor and the audited implementation in parakeet-rs. The
// upstream crate exposes its cached FFT/filterbank but not the raw-sample extraction entry point.
fn extract_features(mut audio: Vec<f32>, cache: &FeatureCache) -> Result<Array2<f32>> {
    for index in (1..audio.len()).rev() {
        audio[index] -= PREEMPHASIS * audio[index - 1];
    }

    let pad = N_FFT / 2;
    let mut padded = vec![0.0; pad];
    padded.extend(audio);
    padded.resize(padded.len() + pad, 0.0);
    let frame_count = (padded.len() - N_FFT) / HOP_LENGTH + 1;
    let frequency_bins = N_FFT / 2 + 1;
    let window = (0..WIN_LENGTH)
        .map(|index| 0.5 - 0.5 * ((2.0 * PI * index as f32) / (WIN_LENGTH as f32 - 1.0)).cos())
        .collect::<Vec<_>>();
    let mut spectrogram = Array2::<f32>::zeros((frequency_bins, frame_count));
    let mut input = vec![0.0; N_FFT];
    let mut output = cache.fft_plan.make_output_vec();
    let mut scratch = cache.fft_plan.make_scratch_vec();
    for frame_index in 0..frame_count {
        let start = frame_index * HOP_LENGTH;
        input.fill(0.0);
        for index in 0..WIN_LENGTH {
            input[index] = padded[start + index] * window[index];
        }
        cache
            .fft_plan
            .process_with_scratch(&mut input, &mut output, &mut scratch)
            .map_err(|error| anyhow::anyhow!("FFT failed: {error}"))?;
        for bin in 0..frequency_bins {
            spectrogram[[bin, frame_index]] = output[bin].norm_sqr();
        }
    }

    let guard = 2.0f32.powi(-24);
    let mut features = cache
        .mel_basis
        .dot(&spectrogram)
        .mapv(|value| (value + guard).ln())
        .t()
        .to_owned();
    if frame_count > 1 {
        for feature_index in 0..FEATURE_SIZE {
            let mut column = features.column_mut(feature_index);
            let mean = column.iter().sum::<f32>() / frame_count as f32;
            let variance = column
                .iter()
                .map(|value| (value - mean).powi(2))
                .sum::<f32>()
                / (frame_count as f32 - 1.0);
            let standard_deviation = variance.sqrt() + 1e-5;
            for value in column.iter_mut() {
                *value = (*value - mean) / standard_deviation;
            }
        }
    }
    Ok(features)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_model_can_change_non_blank_choice_without_biasing_blank() {
        let logits = [1.0, 0.9, 1.05];
        let lm = [-4.0, 0.0];
        assert_eq!(fused_argmax(&logits, &lm, 2, 0.2), 2);
        let logits = [1.0, 0.9, 0.8];
        assert_eq!(fused_argmax(&logits, &lm, 2, 0.2), 1);
    }
}
