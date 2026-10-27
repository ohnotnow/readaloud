mod assets;
mod kokoro;
mod text;

use std::io::Read;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use misaki_rs::{EspeakFallback, G2P, Language};
use rodio::buffer::SamplesBuffer;

use kokoro::{Kokoro, SAMPLE_RATE};
use text::Pronunciations;

/// Reads a text or Markdown document aloud with the Kokoro voice model.
#[derive(Parser)]
#[command(version)]
struct Args {
    /// The document to read. Reads standard input when there is none.
    file: Option<PathBuf>,

    /// Kokoro voice, such as bf_isabella, bm_george or af_heart.
    #[arg(short, long, default_value = "bf_isabella")]
    voice: String,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let document = match &args.file {
        Some(path) => std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?,
        None => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s).context("reading standard input")?;
            s
        }
    };
    let pronunciations = match std::fs::read_to_string(assets::pronunciations_file()?) {
        Ok(source) => Pronunciations::parse(&source),
        Err(_) => Pronunciations::parse(""),
    };

    // American voices (af_, am_) read American English; every other voice reads British.
    let british = !args.voice.starts_with('a');
    let language = if british { Language::EnglishGB } else { Language::EnglishUS };
    let fallback = EspeakFallback::new(british, &assets::cache_dir()?.join("espeak-ng-data"))
        .map_err(anyhow::Error::msg)
        .context("setting up espeak-ng")?;
    let g2p = G2P::with_fallback(language, Some(Box::new(fallback)));

    let voice = assets::voice(&args.voice)?;
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
    for block in text::blocks(&document) {
        let (phonemes, _) = g2p.g2p(&pronunciations.apply(&block))?;
        for chunk in kokoro.chunks(&phonemes) {
            player.append(SamplesBuffer::new(channels, rate, kokoro.speak(&chunk, &voice)?));
        }
        player.append(SamplesBuffer::new(channels, rate, pause.clone()));
    }
    player.sleep_until_end();
    Ok(())
}
