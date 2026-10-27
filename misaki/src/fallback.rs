use std::path::Path;
use std::sync::{LazyLock, Mutex};

use regex::Regex;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FallbackError {
    #[error("espeak error for '{word}': {error}")]
    Espeak {
        word: String,
        error: String,
    },
    #[error("no phonemes matched for '{word}'")]
    NoPhonemes {
        word: String,
    },
    #[error("mutex poisoned: {0}")]
    MutexPoisoned(String),
}

/// Trait for OOV (out-of-vocabulary) word fallback mechanisms
pub trait Fallback: Send + Sync {
    /// Convert unknown word to phonemes
    /// Returns phonemes
    /// Note: espeak-ng is rule-based and always produces output
    fn phonemize(&self, word: &str) -> Result<String, FallbackError>;
}

/// espeak-ng fallback through espeak-ng-rs, the pure Rust port, so there is no C library to link.
///
/// The translator is built once: building one per word re-reads the dictionary from disk,
/// about 15 ms a word.
pub struct EspeakFallback {
    british: bool,
    translator: Mutex<espeak_ng::Translator>,
}

impl EspeakFallback {
    /// Unpacks the bundled English data into `data_dir` (if it is not there yet) and loads it.
    pub fn new(british: bool, data_dir: &Path) -> Result<Self, String> {
        if !data_dir.join("en_dict").exists() {
            std::fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
            espeak_ng::install_bundled_language(data_dir, "en").map_err(|e| e.to_string())?;
        }
        let lang = if british { "en" } else { "en-us" };
        let translator = espeak_ng::Translator::new(lang, Some(data_dir)).map_err(|e| e.to_string())?;
        Ok(Self { british, translator: Mutex::new(translator) })
    }
}

impl Fallback for EspeakFallback {
    fn phonemize(&self, word: &str) -> Result<String, FallbackError> {
        let ipa = self
            .translator
            .lock()
            .map_err(|e| FallbackError::MutexPoisoned(format!("{:?}", e)))?
            .text_to_ipa(word)
            .map_err(|e| FallbackError::Espeak { word: word.to_string(), error: e.to_string() })?;
        let ps = espeak_to_misaki(ipa.trim(), self.british);
        if ps.is_empty() {
            return Err(FallbackError::NoPhonemes { word: word.to_string() });
        }
        Ok(ps)
    }
}

// Python misaki's `EspeakFallback.E2M` (misaki/espeak.py, 0.9.4), longest first. Python asks
// espeak for a tie character between the halves of a diphthong or affricate ("a^ɪ") and maps
// those; espeak-ng-rs has no tie option, so the pairs are matched untied. That can join a
// "t" and "ʃ" that sit either side of a syllable break, which is rare enough to live with.
const E2M: &[(&str, &str)] = &[
    ("ʔˌn\u{329}", "ʔn"),
    ("ʔn\u{329}", "ʔn"),
    ("aɪ", "I"),
    ("aʊ", "W"),
    ("dʒ", "ʤ"),
    ("eɪ", "A"),
    ("tʃ", "ʧ"),
    ("ɔɪ", "Y"),
    ("ʲo", "jo"),
    ("ʲə", "jə"),
    ("e", "A"),
    ("ʲ", ""),
    ("ɚ", "əɹ"),
    ("r", "ɹ"),
    ("x", "k"),
    ("ç", "k"),
    ("ɐ", "ə"),
    ("ɬ", "l"),
    ("\u{303}", ""),
];

// Python maps a tied "ə^l" (espeak's syllabic l, as in "bottle") to "ᵊl". Untied, a schwa
// before an l that starts the next syllable ("bəlˈuːn") looks the same, so only a schwa-l
// that is not followed by a vowel, stress mark or not, counts.
static SYLLABIC_L: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"əl([ˈˌ]?)([^aeiouæɑɒɔəɛɜɪʊʌIAWYOQˈˌ]|$)").unwrap());
static SYLLABIC_MARK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\S)\u{329}").unwrap());

/// Converts espeak-ng IPA into the phoneme set Kokoro was trained on, the way Python misaki does.
pub fn espeak_to_misaki(ipa: &str, british: bool) -> String {
    let mut ps = SYLLABIC_L.replace_all(ipa, "ᵊl$1$2").into_owned();
    for (old, new) in E2M {
        ps = ps.replace(old, new);
    }
    ps = SYLLABIC_MARK.replace_all(&ps, "ᵊ$1").replace('\u{329}', "");
    if british {
        // Python also maps "e^ə" to "ɛː" here, but its "e" -> "A" above has already
        // consumed the "e", so that rule never fires and "hair" comes out "hˈAə". Kept the
        // same, so readaloud sounds like Python misaki.
        ps = ps.replace("iə", "ɪə").replace("əʊ", "Q");
    } else {
        ps = ps
            .replace("oʊ", "O")
            .replace("ɜːɹ", "ɜɹ")
            .replace("ɜː", "ɜɹ")
            .replace("ɪə", "iə")
            .replace('ː', "");
    }
    ps.replace('o', "ɔ").replace('ɾ', "T").replace('ʔ', "t")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Inputs are espeak-ng-rs output; expected values are Python misaki's fallback for the
    // same words.
    #[test]
    fn british_matches_python_misaki() {
        for (ipa, want) in [
            ("bˈɒtəl", "bˈɒtᵊl"),
            ("ɐlˈɑːm", "əlˈɑːm"),
            ("hˈeə", "hˈAə"),
            ("ɡˈəʊ", "ɡˈQ"),
            ("nˈeɪʃən", "nˈAʃən"),
            ("tʃˈeə", "ʧˈAə"),
            ("dʒˈɔɪ", "ʤˈY"),
            ("kəkˈɔːɹəʊ", "kəkˈɔːɹQ"),
        ] {
            assert_eq!(espeak_to_misaki(ipa, true), want, "{ipa}");
        }
    }

    #[test]
    fn american_matches_python_misaki() {
        for (ipa, want) in [
            ("bˈɑːɾəl", "bˈɑTᵊl"),
            ("ɐlˈɑːɹm", "əlˈɑɹm"),
            ("ɡˈoʊ", "ɡˈO"),
            ("bˈʌʔn̩", "bˈʌtn"),
            ("bˈʌɾɚ", "bˈʌTəɹ"),
            ("kəkˈɔːɹoʊ", "kəkˈɔɹO"),
        ] {
            assert_eq!(espeak_to_misaki(ipa, false), want, "{ipa}");
        }
    }

    #[test]
    fn schwa_before_a_vowel_is_not_syllabic() {
        assert_eq!(espeak_to_misaki("bəlˈuːn", true), "bəlˈuːn");
    }
}
