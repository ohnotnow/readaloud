# readaloud

Reads a text, Markdown or code file out loud with the
[Kokoro](https://huggingface.co/hexgrad/Kokoro-82M) voice model, on your own
machine.

```sh
readaloud some-doc.md
cat some-doc.md | readaloud
readaloud --voice bm_george some-doc.md
```

The default voice is `bf_isabella`. Any Kokoro voice name works, and
`readaloud --list-voices` lists them all: `af_` and `am_` voices read
American English, the rest read British English, whatever their accent.

What a file is read as depends on its name:

- `.md` and `.markdown` files, and anything piped in, are Markdown;
- `.html` and `.htm` files are read like Markdown, skipping the page's
  menus (`<nav>`), scripts and styles;
- `.txt` files and files with no extension are plain prose, a paragraph at
  a time;
- anything else is code: announced by its type ("Php file.") and read a line
  at a time, as below.

If there is nothing to read, readaloud says so and exits with status 1.

Markdown is read as prose: headings, paragraphs, list items and table rows
are read in order, links are read as their text, and images are skipped.

Code blocks are read too. Each one is announced with its language ("Python
code block."), read a line at a time, and closed with "End of code block."
In code, inline or in a block, the symbols that carry the meaning are said:

- a dash at the start of a word is "minus": `cargo build --release` is
  "cargo build minus minus release", `go test -v` is "go test minus v";
- a dot inside or at the start of a name is "dot": `some-doc.md` is
  "some-doc dot M D", `~/.config` is "dot config";
- `|` is "pipe";
- `_` is "underscore": `bm_george` is "bm underscore george".

Other symbols are passed over, so `{"reasoning_effort": "none"}` comes out
as "reasoning underscore effort none".

## Where it runs

Builds are made for Apple Silicon Macs and for 64-bit Linux on x86 and ARM.
On Linux it needs glibc 2.38 or newer (Fedora 39, Ubuntu 24.04, Debian 13
and later) and the ALSA library (`alsa-lib` on Fedora, `libasound2` on
Debian and Ubuntu), which desktop installs already have.

## Install

Download the build for your machine from the
[latest release](https://github.com/ohnotnow/readaloud/releases/latest) and
put the binary somewhere on your `PATH`. For example, on an Apple Silicon
Mac:

```sh
curl -fLO https://github.com/ohnotnow/readaloud/releases/latest/download/readaloud-aarch64-apple-darwin.tar.gz
tar -xzf readaloud-aarch64-apple-darwin.tar.gz
mv readaloud/readaloud ~/.local/bin/
```

On Linux use `readaloud-x86_64-unknown-linux-gnu.tar.gz` or
`readaloud-aarch64-unknown-linux-gnu.tar.gz`. Each release also has a
`SHA256SUMS` file to check the download against.

Download with `curl` rather than a browser on a Mac: macOS quarantines files
a browser saves, and then refuses to run the binary because it isn't
notarised. If that happens, `xattr -d com.apple.quarantine readaloud` clears
it.

## Updating

`readaloud version` shows the installed version and says if there is a
newer release. `readaloud self-update` installs it after showing what has
changed; `--yes` skips the question, and `--check` only reports (exit code 0
up to date, 1 newer release available, 2 check failed). A copy you built
yourself, or installed with `cargo install`, won't replace itself.

## First run

The first run downloads the model (310 MB) and the voice (half a megabyte)
into `~/.cache/readaloud`; each new voice is fetched the first time you use
it. Speech starts within about a second after that.

## Words it says wrong

Put fixes in `~/.config/readaloud/pronunciations.txt`, one per line:

```
# word = what to say instead
macOS = mac O S
kubectl = cube control
.env = dot env
```

[pronunciations.txt](pronunciations.txt) in this repository is a starting
point to copy. readaloud only reads the copy in your config folder, so
nothing is applied unless you put it there.

Words match whole and ignoring case, except that a word written in capitals
only matches capitals, so `IT = I T` leaves "it" alone. Anything not in Kokoro's dictionaries
goes to a rule-based fallback, which is where most odd pronunciations come
from, so technical words and names are the usual candidates.

`XDG_CACHE_HOME` and `XDG_CONFIG_HOME` are honoured if set.

## Build

```sh
cargo build --release   # the binary is target/release/readaloud
cargo test --workspace
```

On Linux the audio output needs the ALSA headers to build
(`libasound2-dev` on Debian and Ubuntu).

## How it works

Text becomes phonemes with `misaki/`, a modified copy of
[misaki-rs](https://github.com/MicheleYin/misaki-rs); its README says what
was changed and why. Words it does not know are handled by
[espeak-ng-rs](https://github.com/eugenehp/espeak-ng-rs), a pure Rust port
of espeak-ng, so there is no C library to install. The phonemes go to
Kokoro through [ONNX Runtime](https://onnxruntime.ai) on the CPU, which
made speech about seven times faster than it plays on the Apple Silicon
Mac it was written on.

## Built on

- [Kokoro-82M](https://huggingface.co/hexgrad/Kokoro-82M), the voice model,
  in [onnx-community's ONNX export](https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX). Apache-2.0.
- [misaki](https://github.com/hexgrad/misaki), whose dictionaries and
  espeak mapping readaloud uses. Apache-2.0.
- [misaki-rs](https://github.com/MicheleYin/misaki-rs), the Rust port it
  started from. MIT.
- [espeak-ng-rs](https://github.com/eugenehp/espeak-ng-rs). GPL-3.0-or-later.
- [ort](https://ort.pyke.io), the ONNX Runtime bindings, and
  [rodio](https://github.com/RustAudio/rodio) for playback.

## Licence

GPL-3.0-or-later, because espeak-ng-rs is. See [LICENSE](LICENSE).

The binary also carries parts of other projects under their own licences:

- misaki's pronunciation dictionaries (`misaki/data/`) and Kokoro's phoneme
  list (`src/vocab.json`): Apache-2.0, text in
  [misaki/data/LICENSE](misaki/data/LICENSE).
- misaki-rs, the code in `misaki/src/`: MIT, text in
  [misaki/LICENSE](misaki/LICENSE).
- ONNX Runtime 1.31.0, linked in as a prebuilt library: MIT, with its own
  third-party notices, both in [licenses/onnxruntime/](licenses/onnxruntime/).
- About 140 Rust crates, mostly MIT or Apache-2.0. The release downloads
  include `THIRD-PARTY-LICENSES.txt`, generated by
  [cargo-about](https://github.com/EmbarkStudios/cargo-about) from
  [about.toml](about.toml), which lists the licences the build accepts.
