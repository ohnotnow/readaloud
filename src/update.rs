//! `readaloud version` and `readaloud self-update`: compare against the latest GitHub
//! release and, if asked, swap the binary for the release's.

use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::assets;

const REPO: &str = "ohnotnow/readaloud";

/// Set by the release workflow from the tag. Anything built without it is a "dev" build.
pub const RELEASE: Option<&str> = option_env!("READALOUD_VERSION");

/// What `--version` prints.
pub const VERSION: &str = match RELEASE {
    Some(v) => v,
    None => "dev",
};

/// The release asset built for this machine, matching the release workflow's matrix.
const TARGET: Option<&str> = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
    Some("aarch64-apple-darwin")
} else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
    Some("x86_64-unknown-linux-gnu")
} else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
    Some("aarch64-unknown-linux-gnu")
} else {
    None
};

struct Release {
    tag: String,
    notes: String,
    assets: Vec<(String, String)>,
}

impl Release {
    fn version(&self) -> &str {
        self.tag.trim_start_matches('v')
    }

    fn asset_url(&self, name: &str) -> Result<&str> {
        self.assets
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, url)| url.as_str())
            .with_context(|| format!("release {} has no {name}", self.tag))
    }
}

fn latest_release() -> Result<Release> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(5)))
        .user_agent(format!("readaloud/{VERSION}"))
        .build()
        .into();
    let body = agent
        .get(&format!("https://api.github.com/repos/{REPO}/releases/latest"))
        .header("Accept", "application/vnd.github+json")
        .call()
        .context("asking GitHub for the latest release")?
        .body_mut()
        .read_to_string()?;
    let json: serde_json::Value = serde_json::from_str(&body)?;
    let text = |v: &serde_json::Value| v.as_str().unwrap_or_default().to_string();
    Ok(Release {
        tag: text(&json["tag_name"]),
        notes: text(&json["body"]),
        assets: json["assets"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|a| (text(&a["name"]), text(&a["browser_download_url"])))
            .collect(),
    })
}

/// "1.2.10" beats "1.2.9"; anything that isn't three numbers compares as 0.0.0.
fn is_newer(candidate: &str, current: &str) -> bool {
    let parse = |v: &str| -> (u64, u64, u64) {
        let mut parts = v.trim_start_matches('v').split('.').map(|p| p.parse().unwrap_or(0));
        (parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0))
    };
    parse(candidate) > parse(current)
}

pub fn version() -> ExitCode {
    println!("readaloud {VERSION}");
    // A dev build has nothing meaningful to compare against.
    let Some(current) = RELEASE else { return ExitCode::SUCCESS };
    match latest_release() {
        Ok(latest) if is_newer(latest.version(), current) => {
            println!("A newer release is available: {}. Run `readaloud self-update` to install it.", latest.tag);
        }
        Ok(_) => {}
        Err(e) => eprintln!("readaloud: could not check for a newer release: {e:#}"),
    }
    ExitCode::SUCCESS
}

/// Exit codes for `--check`: 0 up to date, 1 newer release available, 2 lookup failed.
pub fn self_update(yes: bool, check: bool) -> ExitCode {
    match run_self_update(yes, check) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("readaloud: {e:#}");
            ExitCode::from(2)
        }
    }
}

fn run_self_update(yes: bool, check: bool) -> Result<ExitCode> {
    let Some(current) = RELEASE else {
        bail!("this is a development build, so it won't replace itself; rebuild it from source instead");
    };
    let Some(target) = TARGET else {
        bail!("there is no release build for this platform");
    };
    let exe = fs::canonicalize(std::env::current_exe()?)?;
    if let Some(command) = installed_by_cargo(&exe) {
        bail!("this copy was installed by cargo; update it with:\n  {command}");
    }

    let latest = latest_release()?;
    if !is_newer(latest.version(), current) {
        println!("readaloud {current} is the latest release.");
        return Ok(ExitCode::SUCCESS);
    }
    if check {
        println!("readaloud {} is available (this is {current}).", latest.version());
        return Ok(ExitCode::from(1));
    }

    // Fail fast on a location we can't write to (say, one owned by root) before
    // downloading anything.
    let dir = exe.parent().context("the binary has no parent directory")?;
    probe_writable(dir)?;

    println!("Update readaloud {current} -> {}?", latest.version());
    if !latest.notes.trim().is_empty() {
        println!("\n{}\n", latest.notes.trim());
    }
    if !yes && !confirmed()? {
        println!("Left as it is.");
        return Ok(ExitCode::SUCCESS);
    }

    let asset = format!("readaloud-{target}.tar.gz");
    let sums = ureq::get(latest.asset_url("SHA256SUMS")?).call()?.body_mut().read_to_string()?;
    let sha256 = checksum_for(&sums, &asset).with_context(|| format!("SHA256SUMS has no entry for {asset}"))?;
    let tarball = assets::cache_dir()?.join("update").join(&asset);
    assets::download(latest.asset_url(&asset)?, &tarball, Some(sha256))?;
    let binary = binary_from_tarball(&fs::read(&tarball)?)?;
    fs::remove_file(&tarball).ok();

    replace(&exe, &binary)?;
    println!("Updated readaloud {current} -> {}.", latest.version());
    Ok(ExitCode::SUCCESS)
}

/// A binary under ~/.cargo/bin came from `cargo install`, so cargo should update it.
fn installed_by_cargo(exe: &Path) -> Option<String> {
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")))?;
    exe.starts_with(cargo_home.join("bin"))
        .then(|| format!("cargo install --locked --force --git https://github.com/{REPO}"))
}

fn probe_writable(dir: &Path) -> Result<()> {
    let probe = dir.join(format!(".readaloud-write-test-{}", std::process::id()));
    fs::File::create(&probe)
        .with_context(|| format!("can't write to {}; run self-update as a user who can", dir.display()))?;
    fs::remove_file(&probe)?;
    Ok(())
}

/// Asks on the terminal; with no terminal to ask on, insists on --yes instead of
/// guessing.
fn confirmed() -> Result<bool> {
    if !io::stdin().is_terminal() {
        bail!("no terminal to confirm on; run `readaloud self-update --yes` to update without asking");
    }
    print!("Install it? [y/N] ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().lock().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes"))
}

/// Finds a file's hash in `sha256sum` output ("<hash>  <name>" per line).
fn checksum_for<'a>(sums: &'a str, name: &str) -> Option<&'a str> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.split_once(char::is_whitespace)?;
        (file.trim_start_matches([' ', '*']) == name).then_some(hash)
    })
}

/// The release tarball holds a `readaloud/` folder; this takes just the binary out.
fn binary_from_tarball(tarball: &[u8]) -> Result<Vec<u8>> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(tarball));
    for entry in archive.entries()? {
        let mut entry = entry?;
        if entry.path()?.as_ref() == Path::new("readaloud/readaloud") {
            let mut binary = Vec::new();
            io::Read::read_to_end(&mut entry, &mut binary)?;
            return Ok(binary);
        }
    }
    bail!("the release tarball has no readaloud/readaloud in it")
}

/// Writes the new binary next to the old one and renames it over the top. A rename
/// gives the new binary its own inode, which macOS needs: overwriting a signed binary
/// in place gets it killed at its next launch.
fn replace(exe: &Path, binary: &[u8]) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let temp = exe.with_file_name(format!(".readaloud-new-{}", std::process::id()));
    let result = (|| -> Result<()> {
        let mut file = fs::File::create(&temp)?;
        file.write_all(binary)?;
        file.sync_all()?;
        fs::set_permissions(&temp, fs::Permissions::from_mode(0o755))?;
        fs::rename(&temp, exe)?;
        Ok(())
    })();
    if result.is_err() {
        fs::remove_file(&temp).ok();
    }
    result.with_context(|| format!("replacing {}", exe.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_versions_compare_numerically() {
        assert!(is_newer("1.2.10", "1.2.9"));
        assert!(is_newer("v2.0.0", "1.9.9"));
        assert!(!is_newer("1.2.3", "1.2.3"));
        assert!(!is_newer("1.2.2", "1.2.3"));
    }

    #[test]
    fn checksums_are_found_by_file_name() {
        let sums = "aaa  readaloud-aarch64-apple-darwin.tar.gz\nbbb *readaloud-x86_64-unknown-linux-gnu.tar.gz\n";
        assert_eq!(checksum_for(sums, "readaloud-aarch64-apple-darwin.tar.gz"), Some("aaa"));
        assert_eq!(checksum_for(sums, "readaloud-x86_64-unknown-linux-gnu.tar.gz"), Some("bbb"));
        assert_eq!(checksum_for(sums, "readaloud-aarch64-unknown-linux-gnu.tar.gz"), None);
    }

    #[test]
    fn the_binary_comes_out_of_a_release_shaped_tarball() {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        for (path, data) in [("readaloud/LICENSE", &b"licence"[..]), ("readaloud/readaloud", &b"binary"[..])] {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, path, data).unwrap();
        }
        let tarball = builder.into_inner().unwrap().finish().unwrap();
        assert_eq!(binary_from_tarball(&tarball).unwrap(), b"binary");
    }

    #[test]
    fn replace_swaps_the_file_and_makes_it_executable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("readaloud-replace-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("readaloud");
        fs::write(&exe, b"old").unwrap();
        replace(&exe, b"new").unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"new");
        assert_eq!(fs::metadata(&exe).unwrap().permissions().mode() & 0o777, 0o755);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1, "the temp file was left behind");
        fs::remove_dir_all(&dir).unwrap();
    }
}
