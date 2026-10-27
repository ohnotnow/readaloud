//! Kokoro-82M through ONNX Runtime: phonemes in, 24 kHz mono samples out.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use ort::session::Session;
use ort::session::builder::AutoDevicePolicy;
use ort::value::Tensor;

use crate::assets::{STYLE_LEN, VOICE_ROWS};

pub const SAMPLE_RATE: u32 = 24_000;

/// The model takes at most 512 tokens, two of which are the padding at either end.
const MAX_TOKENS: usize = VOICE_ROWS;

pub struct Kokoro {
    session: Session,
    vocab: Vocab,
    _environment: ort::environment::Environment,
}

impl Kokoro {
    pub fn load(model: &Path) -> Result<Self> {
        let environment = ort::init().build()?;
        // On macOS ort registers CoreML by default, which spends 3 s converting the model on
        // every start and then runs no faster than the CPU.
        let session = Session::builder(&environment)?
            .with_auto_device(AutoDevicePolicy::PreferCPU)
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .commit_from_file(model)
            .with_context(|| format!("loading {}", model.display()))?;
        Ok(Self { session, vocab: Vocab::load(), _environment: environment })
    }

    pub fn chunks(&self, phonemes: &str) -> Vec<String> {
        self.vocab.chunks(phonemes)
    }

    pub fn speak(&mut self, phonemes: &str, voice: &[f32]) -> Result<Vec<f32>> {
        let tokens = self.vocab.tokens(phonemes);
        let n = tokens.len().min(MAX_TOKENS);
        // The voice holds one style vector per input length.
        let row = n.min(VOICE_ROWS - 1);
        let style = voice[row * STYLE_LEN..(row + 1) * STYLE_LEN].to_vec();

        let mut ids = Vec::with_capacity(n + 2);
        ids.push(0);
        ids.extend_from_slice(&tokens[..n]);
        ids.push(0);

        let outputs = self.session.run(ort::inputs![
            "input_ids" => Tensor::from_array(([1usize, ids.len()], ids))?,
            "style" => Tensor::from_array(([1usize, STYLE_LEN], style))?,
            "speed" => Tensor::from_array(([1usize], vec![1.0f32]))?,
        ])?;
        let (_, samples) = outputs[0].try_extract_tensor::<f32>()?;
        Ok(samples.to_vec())
    }
}

/// Kokoro's phoneme vocabulary, from the model's config.json (Apache-2.0, like the model;
/// the licence text is in misaki/data/LICENSE).
struct Vocab(HashMap<char, i64>);

impl Vocab {
    fn load() -> Self {
        let vocab: HashMap<String, i64> = serde_json::from_str(include_str!("vocab.json")).unwrap();
        Self(vocab.into_iter().filter_map(|(k, v)| Some((k.chars().next()?, v))).collect())
    }

    /// Characters Kokoro has no token for are dropped, as every Kokoro front end does.
    fn tokens(&self, phonemes: &str) -> Vec<i64> {
        phonemes.chars().filter_map(|c| self.0.get(&c).copied()).collect()
    }

    fn fits(&self, phonemes: &str) -> bool {
        self.tokens(phonemes).len() <= MAX_TOKENS
    }

    /// Splits a block's phonemes into pieces the model can take in one go, breaking at
    /// sentence ends where it can, then at commas and the like, then at spaces.
    fn chunks(&self, phonemes: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for sentence in split_after(phonemes, &['.', '!', '?']) {
            match out.last_mut() {
                Some(last) if self.fits(&format!("{last}{sentence}")) => last.push_str(&sentence),
                _ if self.fits(&sentence) => out.push(sentence),
                _ => out.extend(self.pack(&sentence)),
            }
        }
        out.into_iter().map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect()
    }

    fn pack(&self, sentence: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for clause in split_after(sentence, &[',', ';', ':']) {
            for word in split_after(&clause, &[' ']) {
                match out.last_mut() {
                    Some(last) if self.fits(&format!("{last}{word}")) => last.push_str(&word),
                    // A single "word" too long to fit is cut, which only nonsense input does.
                    _ => out.push(word.chars().take(MAX_TOKENS).collect()),
                }
            }
        }
        out
    }
}

/// Splits after each separator, keeping it on the piece before.
fn split_after(text: &str, separators: &[char]) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, c) in text.char_indices() {
        if separators.contains(&c) {
            let end = i + c.len_utf8();
            out.push(text[start..end].to_string());
            start = end;
        }
    }
    if start < text.len() {
        out.push(text[start..].to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_one_chunk() {
        let vocab = Vocab::load();
        assert_eq!(vocab.chunks("hˈɛlQ. ɡʊdbˈI!"), ["hˈɛlQ. ɡʊdbˈI!"]);
    }

    #[test]
    fn long_text_splits_at_sentences_without_losing_anything() {
        let vocab = Vocab::load();
        let sentence = "ðə kwˈɪk bɹˈWn fˈɒks ʤˈʌmps ˈQvə ðə lˈAzi dˈɒɡ. ";
        let text = sentence.repeat(20);
        let chunks = vocab.chunks(&text);
        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|c| vocab.fits(c) && c.ends_with('.')));
        assert_eq!(chunks.join(" "), text.trim());
    }

    #[test]
    fn one_enormous_sentence_splits_at_commas_then_spaces() {
        let vocab = Vocab::load();
        let text = "wˈʌn, tˈuː, θɹˈiː ".repeat(60);
        let chunks = vocab.chunks(&text);
        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|c| vocab.fits(c)));
        let squash = |s: &str| s.split_whitespace().collect::<String>();
        assert_eq!(squash(&chunks.concat()), squash(&text));
    }
}
