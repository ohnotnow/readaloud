//! Where readaloud keeps things, and fetching the model and voices on first use.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

// Pinned to one commit of onnx-community's export of Kokoro-82M v1.0, so the files can't
// change underneath us.
const REPO: &str = "https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX/resolve/1939ad2a8e416c0acfeecc08a694d14ef25f2231";
const MODEL_SHA256: &str = "8fbea51ea711f2af382e88c833d9e288c6dc82ce5e98421ea61c058ce21a34cb";

/// Every voice at that commit, so the list can't go stale. The first letter is the
/// accent the voice was trained on, the second whether it is female or male; any of
/// them can read English.
pub const VOICES: [&str; 55] = [
    "af", "af_alloy", "af_aoede", "af_bella", "af_heart", "af_jessica", "af_kore", "af_nicole",
    "af_nova", "af_river", "af_sarah", "af_sky", "am_adam", "am_echo", "am_eric", "am_fenrir",
    "am_liam", "am_michael", "am_onyx", "am_puck", "am_santa", "bf_alice", "bf_emma",
    "bf_isabella", "bf_lily", "bm_daniel", "bm_fable", "bm_george", "bm_lewis", "ef_dora",
    "em_alex", "em_santa", "ff_siwis", "hf_alpha", "hf_beta", "hm_omega", "hm_psi", "if_sara",
    "im_nicola", "jf_alpha", "jf_gongitsune", "jf_nezumi", "jf_tebukuro", "jm_kumo", "pf_dora",
    "pm_alex", "pm_santa", "zf_xiaobei", "zf_xiaoni", "zf_xiaoxiao", "zf_xiaoyi", "zm_yunjian",
    "zm_yunxi", "zm_yunxia", "zm_yunyang",
];

/// The accent a voice's first letter stands for.
pub fn accent(voice: &str) -> &'static str {
    match voice.chars().next() {
        Some('a') => "American English",
        Some('b') => "British English",
        Some('e') => "Spanish",
        Some('f') => "French",
        Some('h') => "Hindi",
        Some('i') => "Italian",
        Some('j') => "Japanese",
        Some('p') => "Brazilian Portuguese",
        Some('z') => "Mandarin Chinese",
        _ => "Other",
    }
}

/// A voice file is a 510 x 256 table of f32 style vectors, one row per input length.
pub const VOICE_ROWS: usize = 510;
pub const STYLE_LEN: usize = 256;

fn home_dir(xdg_var: &str, fallback: &str) -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os(xdg_var).filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(dir).join("readaloud"));
    }
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(fallback).join("readaloud"))
}

pub fn cache_dir() -> Result<PathBuf> {
    home_dir("XDG_CACHE_HOME", ".cache")
}

pub fn pronunciations_file() -> Result<PathBuf> {
    Ok(home_dir("XDG_CONFIG_HOME", ".config")?.join("pronunciations.txt"))
}

pub fn model() -> Result<PathBuf> {
    let path = cache_dir()?.join("kokoro-v1.0.onnx");
    if !path.exists() {
        download(&format!("{REPO}/onnx/model.onnx"), &path, Some(MODEL_SHA256))?;
    }
    Ok(path)
}

/// Returns the voice's style table, downloading it the first time it is used.
pub fn voice(name: &str) -> Result<Vec<f32>> {
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        bail!("{name:?} is not a Kokoro voice name");
    }
    let path = cache_dir()?.join("voices").join(format!("{name}.bin"));
    if !path.exists() {
        download(&format!("{REPO}/voices/{name}.bin"), &path, None)
            .with_context(|| format!("could not fetch the voice {name:?}; is that a Kokoro voice?"))?;
    }
    let bytes = fs::read(&path)?;
    if bytes.len() != VOICE_ROWS * STYLE_LEN * 4 {
        bail!("{} is {} bytes, not a Kokoro voice; delete it and try again", path.display(), bytes.len());
    }
    Ok(bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect())
}

/// Downloads to a `.part` file next to `dest` and only renames it into place once complete
/// (and, when given, the checksum matches), so an interrupted download is never mistaken
/// for a good one.
pub fn download(url: &str, dest: &Path, sha256: Option<&str>) -> Result<()> {
    fs::create_dir_all(dest.parent().unwrap())?;
    let name = dest.file_name().unwrap().to_string_lossy();
    eprintln!("readaloud: downloading {name}");
    let mut response = ureq::get(url).call().with_context(|| format!("downloading {url}"))?;
    let total: Option<u64> = response.headers().get("content-length").and_then(|v| v.to_str().ok()?.parse().ok());
    let mut reader = response.body_mut().as_reader();

    let part = dest.with_extension("part");
    let mut file = fs::File::create(&part)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let (mut done, mut shown) = (0u64, 0u64);
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        hasher.update(&buf[..n]);
        done += n as u64;
        if let Some(total) = total.filter(|_| done - shown >= 16 << 20) {
            shown = done;
            eprint!("\r  {} of {} MB", done >> 20, total >> 20);
            io::stderr().flush().ok();
        }
    }
    if shown > 0 {
        eprintln!();
    }
    file.sync_all()?;

    if let Some(want) = sha256 {
        let got: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if got != want {
            fs::remove_file(&part).ok();
            bail!("{name} failed its checksum (got {got}, expected {want})");
        }
    }
    fs::rename(&part, dest)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_voice_is_a_valid_name_with_a_known_accent() {
        for name in VOICES {
            assert!(name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'), "{name}");
            assert_ne!(accent(name), "Other", "{name}");
        }
        assert!(VOICES.contains(&"bf_isabella"));
    }
}
