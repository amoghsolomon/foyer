use std::{
    fs::File,
    io::{self, BufReader, Read as _},
    path::Path,
};

use anyhow::{Context as _, Result, ensure};

const MAGIC: &[u8; 8] = b"SHELLNG1";
const FORMAT_VERSION: u32 = 1;

#[derive(Debug)]
pub struct NGramModel {
    vocab_size: usize,
    max_order: usize,
    arcs_weights: Vec<f32>,
    to_states: Vec<u32>,
    labels: Vec<u32>,
    backoff_weights: Vec<f32>,
    backoff_states: Vec<u32>,
    arc_ranges: Vec<[u32; 2]>,
}

impl NGramModel {
    pub fn load(path: &Path, expected_vocab_size: usize) -> Result<Self> {
        let file =
            File::open(path).with_context(|| format!("open 6-gram model at {}", path.display()))?;
        let mut reader = BufReader::new(file);
        let mut magic = [0; 8];
        reader.read_exact(&mut magic)?;
        ensure!(&magic == MAGIC, "invalid Foyer Shell n-gram magic");
        let version = read_u32(&mut reader)?;
        ensure!(
            version == FORMAT_VERSION,
            "unsupported n-gram format {version}"
        );
        let vocab_size = read_u32(&mut reader)? as usize;
        let max_order = read_u32(&mut reader)? as usize;
        let state_count = read_u32(&mut reader)? as usize;
        let arc_count = read_u32(&mut reader)? as usize;
        ensure!(
            vocab_size == expected_vocab_size,
            "n-gram vocabulary mismatch"
        );
        ensure!(
            vocab_size > 0 && vocab_size <= 65_536,
            "invalid n-gram vocabulary size"
        );
        ensure!((1..=16).contains(&max_order), "invalid n-gram order");
        ensure!(
            (2..=100_000_000).contains(&state_count),
            "invalid n-gram state count"
        );
        ensure!(
            arc_count >= vocab_size && arc_count <= 200_000_000,
            "invalid n-gram arc count"
        );

        let arcs_weights = read_f32_vec(&mut reader, arc_count)?;
        let to_states = read_u32_vec(&mut reader, arc_count)?;
        let labels = read_u32_vec(&mut reader, arc_count)?;
        let backoff_weights = read_f32_vec(&mut reader, state_count)?;
        let backoff_states = read_u32_vec(&mut reader, state_count)?;
        let ranges = read_u32_vec(&mut reader, state_count * 2)?;
        let arc_ranges = ranges
            .chunks_exact(2)
            .map(|range| [range[0], range[1]])
            .collect::<Vec<_>>();

        let model = Self {
            vocab_size,
            max_order,
            arcs_weights,
            to_states,
            labels,
            backoff_weights,
            backoff_states,
            arc_ranges,
        };
        model.validate()?;
        Ok(model)
    }

    pub fn initial_state(&self) -> u32 {
        1
    }

    pub fn vocab_size(&self) -> usize {
        self.vocab_size
    }

    pub fn advance(&self, state: u32, scores: &mut [f32], next_states: &mut [u32]) -> Result<()> {
        ensure!(
            scores.len() == self.vocab_size,
            "n-gram score buffer has wrong size"
        );
        ensure!(
            next_states.len() == self.vocab_size,
            "n-gram state buffer has wrong size"
        );
        ensure!(
            (state as usize) < self.arc_ranges.len(),
            "n-gram state is out of range"
        );
        scores.fill(0.0);
        next_states.fill(u32::MAX);

        let mut current = state;
        let mut accumulated_backoff = 0.0;
        for _ in 0..=self.max_order {
            let [start, end] = self.arc_ranges[current as usize];
            for arc in start as usize..end as usize {
                let label = self.labels[arc] as usize;
                if next_states[label] == u32::MAX {
                    scores[label] = accumulated_backoff + self.arcs_weights[arc];
                    next_states[label] = self.to_states[arc];
                }
            }
            if current == 0 {
                break;
            }
            accumulated_backoff += self.backoff_weights[current as usize];
            current = self.backoff_states[current as usize];
        }
        ensure!(
            next_states.iter().all(|state| *state != u32::MAX),
            "n-gram backoff did not resolve the complete vocabulary"
        );
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        let state_count = self.arc_ranges.len();
        let arc_count = self.labels.len();
        ensure!(self.backoff_weights.len() == state_count);
        ensure!(self.backoff_states.len() == state_count);
        ensure!(self.arcs_weights.len() == arc_count);
        ensure!(self.to_states.len() == arc_count);
        for (index, [start, end]) in self.arc_ranges.iter().copied().enumerate() {
            ensure!(
                start <= end && end as usize <= arc_count,
                "invalid arc range for state {index}"
            );
            ensure!(
                end - start <= self.vocab_size as u32,
                "state {index} has too many arcs"
            );
        }
        ensure!(
            self.labels
                .iter()
                .all(|label| (*label as usize) < self.vocab_size)
        );
        ensure!(
            self.to_states
                .iter()
                .all(|state| (*state as usize) < state_count)
        );
        ensure!(
            self.backoff_states
                .iter()
                .all(|state| (*state as usize) < state_count)
        );
        ensure!(
            self.arc_ranges[0][1] - self.arc_ranges[0][0] == self.vocab_size as u32,
            "root state must cover the vocabulary"
        );
        Ok(())
    }
}

fn read_u32(reader: &mut impl io::Read) -> io::Result<u32> {
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u32_vec(reader: &mut impl io::Read, len: usize) -> io::Result<Vec<u32>> {
    (0..len).map(|_| read_u32(reader)).collect()
}

fn read_f32_vec(reader: &mut impl io::Read, len: usize) -> io::Result<Vec<f32>> {
    (0..len)
        .map(|_| read_u32(reader).map(f32::from_bits))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> NGramModel {
        // Root has unigrams a=-2, b=-3. BOS overrides a=-0.2 then backs off by -0.5 for b.
        NGramModel {
            vocab_size: 2,
            max_order: 2,
            arcs_weights: vec![-2.0, -3.0, -0.2],
            to_states: vec![0, 0, 0],
            labels: vec![0, 1, 0],
            backoff_weights: vec![0.0, -0.5],
            backoff_states: vec![0, 0],
            arc_ranges: vec![[0, 2], [2, 3]],
        }
    }

    #[test]
    fn backs_off_and_keeps_nearest_matching_arc() {
        let model = fixture();
        model.validate().unwrap();
        let mut scores = [0.0; 2];
        let mut states = [0; 2];
        model
            .advance(model.initial_state(), &mut scores, &mut states)
            .unwrap();
        assert_eq!(scores, [-0.2, -3.5]);
        assert_eq!(states, [0, 0]);
    }

    #[test]
    fn rejects_unresolved_vocabularies() {
        let mut model = fixture();
        model.arc_ranges[0] = [0, 1];
        assert!(model.validate().is_err());
    }

    #[test]
    fn exported_software_engineering_model_matches_nemo_reference_when_available() {
        let Some(path) = std::env::var_os("FOYER_SHELL_TEST_NGRAM") else {
            return;
        };
        let model = NGramModel::load(Path::new(&path), 1024).unwrap();
        let mut scores = vec![0.0; 1024];
        let mut states = vec![0; 1024];
        model
            .advance(model.initial_state(), &mut scores, &mut states)
            .unwrap();
        let reference = [
            (0, -11.386_756, 2),
            (1, -7.421_784_4, 1025),
            (2, -6.207_037, 1026),
            (42, -13.199_003, 44),
            (100, -13.174_511, 102),
            (512, -9.696_422, 1245),
            (1023, -17.304_287, 1024),
        ];
        for (token, score, state) in reference {
            assert!((scores[token] - score).abs() < 1e-5, "token {token}");
            assert_eq!(states[token], state, "token {token}");
        }
    }
}
