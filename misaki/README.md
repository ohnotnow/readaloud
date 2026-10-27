# Misaki-RS

> **This is readaloud's modified copy of [misaki-rs](https://github.com/MicheleYin/misaki-rs) 0.6.0.**
> The changes:
>
> - `data/` holds the original dictionaries from Python [misaki](https://github.com/hexgrad/misaki)
>   0.9.4 (Apache-2.0). The ones misaki-rs ships were rewritten in espeak-ng's notation,
>   which writes diphthongs as two vowels joined by U+200D; Kokoro has no token for that
>   joiner and was trained on misaki's single-letter diphthongs (`Y`, `Q`, `I`, `A`, `W`).
>   On a 6,000-word sample the shipped dictionaries matched Python misaki on 48.5% of
>   words, the original ones on 91.3%.
> - Unknown words go to [espeak-ng-rs](https://github.com/eugenehp/espeak-ng-rs), a pure
>   Rust port of espeak-ng, instead of the C library, and its output is converted with
>   Python misaki's own mapping (`src/fallback.rs`).
> - Dashes between words (em and en dashes, `--`, and a hyphen with spaces round it) become
>   Kokoro's em dash token, which it pauses on, as Python misaki does; upstream dropped them.
> - Hyphenated words stay whole, so the dictionary is tried first ("day-to-day"); one it
>   doesn't know is said as a single word, as Python misaki does. Upstream split them
>   into separate words with gaps between.
> - Four-digit numbers are read as years ("nineteen ninety"), as Python misaki does.
> - Number ranges are read with "to": "12-18" is "twelve to eighteen". Python misaki says
>   "twelve eighteen"; this one is readaloud's own rule.
> - `G2P::new` no longer sets up a fallback itself; pass one to `G2P::with_fallback`.
> - A word the dictionary doesn't have but `is_known` accepts is spelled out letter by
>   letter, as Python misaki does; upstream only did that for proper nouns, so a lone
>   lowercase letter ("option b", `-v`) came out as the unknown marker and was dropped.
>   A leftover debugging `eprintln!` in `Lexicon::lookup` is gone too.
> - The unused `language-tokenizer` dependency is gone.
>
> The rest of this README is upstream's and may not match.

**misaki-rs** is a self-contained, high-performance Rust port of the [Misaki](https://github.com/hexgrad/misaki) G2P (Grapheme-to-Phoneme) engine. 

It is specifically designed for use with TTS models like **Kokoro**, providing accurate Part-of-Speech aware phonemization for English text.

## Features

- **Self-Contained**: All lexicons, dictionaries, and Part-of-Speech tagger weights are embedded directly into the binary at compile time. No external resource files are required at runtime.
- **POS-Aware Phonemization**: Uses an averaged perceptron tagger to handle heteronyms (words with different pronunciations based on context, e.g., *object* as a noun vs. verb).
- **Multi-Dialect Support**: Supports both **US English** (en-us) and **British English** (en-gb).
- **Morphological Stemming**: Intelligent handling of suffixes (plurals, past tense, continuous tense). Other rules may be added in the future. Currently those are:
    -  s plural stemming
    - ed past tense stemming
    - ing continuous tense stemming
- **Number Conversion**: Automatically converts numeric values into their spoken word equivalents.
- **Optional espeak fallback** (feature `espeak`, enabled by default): For out-of-vocabulary words, use [espeak-ng](https://github.com/espeak-ng/espeak-ng) to produce phonemes. Disable with `default-features = false` for a smaller build with no system espeak dependency; unknown words will then be spelled letter-by-letter or marked as unknown.


## Why “spelling out” when espeak is disabled?

When a word is not in the lexicon and no rule applies, the engine needs a fallback. With the `espeak` feature **enabled**, that fallback is espeak-ng: the word is sent to espeak and its IPA output is converted to the engine’s phoneme set. With the `espeak` feature **disabled**, there is no external fallback, so the engine falls back to **character-by-character spelling**: each letter is phonemized as its name (e.g. “B” → “bˈi”, “K” → “kˈe‍ɪ”). So for example “eBook” becomes the sequence of letter names (E, B, O, O, K) instead of the word “e-book”. Single-character tokens and unrecognized characters may be marked as unknown (❓) instead. This behavior is intentional so that builds without espeak still produce some output rather than failing.


## Testing espeak

To check that espeak fallback is working, phonemize an out-of-vocabulary word like `"eBook"` and assert it does not contain the unknown marker and is not spelled letter-by-letter:

```bash
cargo test test_ebook_with_espeak -- --nocapture
```

You should see output like `eBook (with espeak): ˈi bˈʊk.` (word-like). Without the espeak feature, the same word is spelled out: `cargo test test_ebook_without_espeak --no-default-features -- --nocapture` gives e.g. `eBook (without espeak): ˈiː  bˈi  ˈo‍ʊ  ˈo‍ʊ  kˈe‍ɪ` (E, B, O, O, K as letter names).


## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
misaki-rs = "0.3.0"
```

**Optional: disable espeak fallback** (smaller build, no espeak-ng dependency):

```toml
[dependencies]
misaki-rs = { version = "0.3.0", default-features = false }
```

To depend on misaki-rs without default features but still use espeak:

```toml
misaki-rs = { version = "0.3.0", default-features = false, features = ["espeak"] }
```

## Quick Start

```rust
use misaki_rs::G2P;

fn main() {
    // Initialize for US English (false = US, true = GB)
    let g2p = G2P::new(false); 
    
    let (phonemes, tokens) = g2p.g2p("Hello, world! 123");
    println!("US Phonemes: {}", phonemes);
    
    // Initialize for British English
    let g2p_gb = G2P::new(true);
    let (phonemes_gb, _) = g2p_gb.g2p("The schedule is full.");
    println!("GB Phonemes: {}", phonemes_gb);
}
```

## Pronunciations

The original misaki project had very few words and some were not pronunced correctly. Here I updated the original pronunciation dict to include more words and correct pronunciations using eSpeak.

## Scope

This repository aims to provide a lightweight and efficient alternative to ONNX-based phonemizers for Rust applications. It eliminates the need for external C++ dependencies or large model files by porting the logic and data into native Rust.

## License

This project is based on the original Misaki library. See the original repository for licensing details regarding the underlying dictionary data.
