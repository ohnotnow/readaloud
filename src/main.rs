mod assets;
mod kokoro;
mod text;
mod update;

use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::Parser;
use misaki_rs::{EspeakFallback, G2P, Language};
use rodio::buffer::SamplesBuffer;

use kokoro::{Kokoro, SAMPLE_RATE};
use text::Pronunciations;

/// Reads a text, Markdown or code file aloud with the Kokoro voice model.
#[derive(Parser)]
#[command(
    version = update::VERSION,
    after_help = "Also: `readaloud version` checks for a newer release, and `readaloud self-update` installs it.\nTo read a file called version or self-update, give its path: readaloud ./version"
)]
struct Args {
    /// The document to read. Reads standard input when there is none.
    file: Option<PathBuf>,

    /// Kokoro voice, such as bf_isabella, bm_george or af_heart. Defaults to the voice in
    /// ~/.config/readaloud/config.txt, or bf_isabella.
    #[arg(short, long)]
    voice: Option<String>,

    /// List the Kokoro voices, grouped by accent, and exit.
    #[arg(long)]
    list_voices: bool,
}

/// Swaps this binary for the latest GitHub release.
#[derive(Parser)]
#[command(name = "readaloud self-update", bin_name = "readaloud self-update")]
struct SelfUpdateArgs {
    /// Install without asking.
    #[arg(short, long)]
    yes: bool,

    /// Only report: exit 0 if up to date, 1 if a newer release exists, 2 if the check failed.
    #[arg(long)]
    check: bool,
}

fn main() -> Result<ExitCode> {
    // The two subcommands are the exact words; anything else is a document to read.
    match std::env::args().nth(1).as_deref() {
        Some("version") => return Ok(update::version()),
        Some("self-update") => {
            let args = SelfUpdateArgs::parse_from(std::env::args().skip(1));
            return Ok(update::self_update(args.yes, args.check));
        }
        _ => {}
    }

    let args = Args::parse();
    if args.list_voices {
        list_voices();
        return Ok(ExitCode::SUCCESS);
    }
    if args.file.is_none() && std::io::stdin().is_terminal() {
        eprintln!("readaloud: give a file to read, or pipe text in (readaloud --help for more)");
        return Ok(ExitCode::from(2));
    }
    let document = match &args.file {
        Some(path) => std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?,
        None => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s).context("reading standard input")?;
            s
        }
    };
    let blocks = text::Format::of(args.file.as_deref()).blocks(&document);
    if blocks.is_empty() {
        let source = args.file.as_ref().map_or("standard input".to_string(), |p| p.display().to_string());
        eprintln!("readaloud: nothing to read in {source}");
        return Ok(ExitCode::FAILURE);
    }
    let pronunciations = Pronunciations::parse(&read_user_file(&assets::pronunciations_file()?));

    let config_file = assets::config_file()?;
    let voice_name = match (args.voice, configured_voice(&config_file)) {
        (Some(name), _) => name,
        (None, Some(name)) if !assets::is_voice(&name) => {
            bail!("{name:?} in {} is not a Kokoro voice (readaloud --list-voices lists them)", config_file.display())
        }
        (None, Some(name)) => name,
        (None, None) => DEFAULT_VOICE.to_string(),
    };
    // American voices (af_, am_) read American English; every other voice reads British.
    let british = !voice_name.starts_with('a');
    let language = if british { Language::EnglishGB } else { Language::EnglishUS };
    let fallback = EspeakFallback::new(british, &assets::cache_dir()?.join("espeak-ng-data"))
        .map_err(anyhow::Error::msg)
        .context("setting up espeak-ng")?;
    let g2p = G2P::with_fallback(language, Some(Box::new(fallback)));

    let voice = assets::voice(&voice_name)?;
    let mut kokoro = Kokoro::load(&assets::model()?)?;

    // rodio asks for a fixed 50 ms buffer by default, which some devices refuse (one here
    // accepts at most 1024 samples), so leave the buffer size to the OS.
    let mut device = rodio::DeviceSinkBuilder::from_default_device()
        .and_then(|builder| builder.with_buffer_size(rodio::cpal::BufferSize::Default).open_stream())
        .context("opening the audio output")?;
    device.log_on_drop(false);
    let player = rodio::Player::connect_new(device.mixer());
    let channels = rodio::nz!(1);
    let rate = std::num::NonZero::new(SAMPLE_RATE).unwrap();
    let pause = vec![0.0; SAMPLE_RATE as usize * 3 / 10];

    // Kokoro makes speech much faster than it plays, so queueing each piece as it is
    // made keeps the player fed and the first words start almost at once.
    for block in blocks {
        let (phonemes, _) = g2p.g2p(&pronunciations.apply(&block))?;
        for chunk in kokoro.chunks(&phonemes) {
            player.append(SamplesBuffer::new(channels, rate, kokoro.speak(&chunk, &voice)?));
        }
        player.append(SamplesBuffer::new(channels, rate, pause.clone()));
    }
    player.sleep_until_end();
    Ok(ExitCode::SUCCESS)
}

const DEFAULT_VOICE: &str = "bf_isabella";

/// Reads one of the user's `key = value` files, warning about any line that isn't one.
/// A file that isn't there reads as empty.
fn read_user_file(path: &Path) -> String {
    let source = std::fs::read_to_string(path).unwrap_or_default();
    for line in text::entries(&source).filter_map(Result::err) {
        eprintln!("readaloud: skipping {line:?} in {}: it isn't `name = value`", path.display());
    }
    source
}

/// The voice the user's config file sets, warning about any setting it doesn't know.
fn configured_voice(path: &Path) -> Option<String> {
    let source = read_user_file(path);
    let mut voice = None;
    for (key, value) in text::entries(&source).filter_map(Result::ok) {
        match key {
            // A later line overrides an earlier one.
            "voice" => voice = Some(value.to_string()),
            _ => eprintln!("readaloud: skipping the unknown setting {key:?} in {}", path.display()),
        }
    }
    voice
}

/// One line per accent, accents in alphabetical order.
fn list_voices() {
    for accent in assets::VOICES.iter().map(|v| assets::accent(v)).collect::<std::collections::BTreeSet<_>>() {
        let names: Vec<_> = assets::VOICES.iter().copied().filter(|v| assets::accent(v) == accent).collect();
        println!("{accent}: {}", names.join(" "));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_example_config_file_sets_the_default_voice() {
        let settings: Vec<_> = text::entries(include_str!("../config.txt")).collect();
        assert_eq!(settings, [Ok(("voice", DEFAULT_VOICE))]);
    }
}
