use std::{env, path::PathBuf};

#[derive(Clone, Debug)]
pub struct Config {
    pub model_dir: PathBuf,
    pub ngram_path: PathBuf,
    pub ngram_alpha: f32,
    pub execution_provider: String,
}

impl Config {
    pub fn from_env() -> Self {
        let data_home = foyer_shell_paths::data_root().join("transcription");
        Self {
            model_dir: env::var_os("FOYER_SHELL_TRANSCRIPTION_MODEL_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| data_home.join("parakeet-unified-en-0.6b-onnx")),
            ngram_path: env::var_os("FOYER_SHELL_TRANSCRIPTION_NGRAM")
                .map(PathBuf::from)
                .unwrap_or_else(|| data_home.join("software-engineering-unified-6gram.sng")),
            ngram_alpha: env::var("FOYER_SHELL_TRANSCRIPTION_NGRAM_ALPHA")
                .ok()
                .and_then(|value| value.parse().ok())
                .filter(|value: &f32| (0.0..=2.0).contains(value))
                .unwrap_or(0.2),
            execution_provider: env::var("FOYER_SHELL_TRANSCRIPTION_EXECUTION_PROVIDER")
                .unwrap_or_else(|_| "cpu".into()),
        }
    }
}
