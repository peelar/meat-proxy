//! Keeps the installed app at the latest release.
//!
//! The app asks GitHub for the newest release every few minutes. A newer one is downloaded into a
//! `staged` folder. It is installed when the user presses the button, or by itself at a quiet moment.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const REPO: &str = "peelar/meat-proxy";
const APP_NAME: &str = "Meat Proxy.app";

/// How often the app asks GitHub whether there is a newer release.
pub const CHECK_EVERY_SECS: f64 = 300.0;
/// How long the window must have been unfocused before a ready update installs by itself.
pub const QUIET_FOR_SECS: u64 = 120;

/// `v0.3.18` or `0.3.18` as numbers, so `0.3.10` is newer than `0.3.9`.
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.trim().trim_start_matches('v').split('.');
    let parsed = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(parsed)
}

/// True when `latest` is a release after `current`. A dev build has no release to compare with.
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

/// When a ready update may install without being asked: the app is in the background, has been for a
/// while, and nothing it started is still running.
pub fn quiet_moment(unfocused_for: Option<std::time::Duration>, working: bool) -> bool {
    !working && unfocused_for.is_some_and(|d| d.as_secs() >= QUIET_FOR_SECS)
}

/// The `.app` this process runs from, when it is the installed one. A `cargo run` build, and the dev
/// profile, have none and never update themselves.
pub fn running_bundle() -> Option<PathBuf> {
    if crate::profile::is_dev() {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let bundle = exe.parent()?.parent()?.parent()?;
    (bundle.extension()? == "app").then(|| bundle.to_owned())
}

fn support_dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support/meat-proxy"))
}

/// The newest release's tag, which GitHub's `/releases/latest` address redirects to.
fn latest_tag() -> Result<String, String> {
    let out = Command::new("curl")
        .args([
            "-fsSLI",
            "-m",
            "20",
            "-o",
            "/dev/null",
            "-w",
            "%{url_effective}",
        ])
        .arg(format!("https://github.com/{REPO}/releases/latest"))
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "GitHub did not answer: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let url = String::from_utf8_lossy(&out.stdout);
    let tag = url.trim().rsplit('/').next().unwrap_or_default();
    parse_version(tag)
        .map(|_| tag.to_owned())
        .ok_or_else(|| format!("no release yet: {url}"))
}

fn run(command: &mut Command) -> Result<(), String> {
    let out = command.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

/// Downloads `tag` into `<support>/staged`, checks its signature and records its version there.
fn stage(support: &Path, tag: &str) -> Result<(), String> {
    let staged = support.join("staged");
    if std::fs::read_to_string(staged.join("version")).is_ok_and(|v| v.trim() == tag)
        && staged.join(APP_NAME).is_dir()
    {
        return Ok(());
    }
    let work = support.join(format!("download-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let result = (|| {
        let zip = work.join("app.zip");
        run(Command::new("curl")
            .args(["-fsSL", "-m", "300", "-o"])
            .arg(&zip)
            .arg(format!(
                "https://github.com/{REPO}/releases/download/{tag}/meat-proxy-macos.zip"
            )))?;
        let unpacked = work.join("unpacked");
        run(Command::new("ditto")
            .args(["-x", "-k"])
            .arg(&zip)
            .arg(&unpacked))?;
        let app = unpacked.join(APP_NAME);
        if !app.is_dir() {
            return Err(format!("{tag}'s zip has no {APP_NAME}"));
        }
        let _ = Command::new("xattr")
            .args(["-dr", "com.apple.quarantine"])
            .arg(&app)
            .output();
        run(Command::new("codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(&app))?;
        let _ = std::fs::remove_dir_all(&staged);
        std::fs::create_dir_all(&staged).map_err(|e| e.to_string())?;
        std::fs::rename(&app, staged.join(APP_NAME)).map_err(|e| e.to_string())?;
        std::fs::write(staged.join("version"), tag).map_err(|e| e.to_string())
    })();
    let _ = std::fs::remove_dir_all(&work);
    result
}

/// Earlier installers ran a launchd agent through `sh`, which Login Items listed as "sh". The app
/// updates itself now, so it takes that agent away.
fn remove_legacy_updater() {
    const LABEL: &str = "co.anomaly.meat-proxy.updater";
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return;
    };
    let agent = home.join(format!("Library/LaunchAgents/{LABEL}.plist"));
    if !agent.exists() {
        return;
    }
    if let Ok(uid) = Command::new("id").arg("-u").output() {
        let uid = String::from_utf8_lossy(&uid.stdout).trim().to_owned();
        let _ = Command::new("launchctl")
            .args(["bootout", &format!("gui/{uid}/{LABEL}")])
            .output();
    }
    let _ = std::fs::remove_file(agent);
    if let Some(support) = support_dir() {
        let _ = std::fs::remove_file(support.join("update.sh"));
    }
}

/// Asks GitHub for the newest release; when it is after `current`, has it downloaded and returns its tag.
pub fn check_and_stage(current: &str) -> Result<Option<String>, String> {
    remove_legacy_updater();
    let tag = latest_tag()?;
    if !is_newer(&tag, current) {
        return Ok(None);
    }
    let support = support_dir().ok_or("no HOME")?;
    std::fs::create_dir_all(&support).map_err(|e| e.to_string())?;
    stage(&support, &tag)?;
    Ok(Some(tag))
}

/// Swaps the downloaded release in for the running app and opens it. The swap waits for this process
/// to quit, so it is done by a detached shell; the caller must exit right after.
pub fn install_and_relaunch(tag: &str) -> Result<(), String> {
    let bundle = running_bundle().ok_or("not running from an installed app")?;
    let support = support_dir().ok_or("no HOME")?;
    let staged = support.join("staged").join(APP_NAME);
    if !staged.is_dir() {
        return Err("the update is not downloaded".to_owned());
    }
    let script = r#"
while kill -0 "$1" 2>/dev/null; do sleep 0.2; done
rm -rf "$3.old"
mv "$3" "$3.old" && mv "$2" "$3" || { [ -d "$3.old" ] && mv "$3.old" "$3"; open "$3"; exit 1; }
rm -rf "$3.old" "$(dirname "$2")"
echo "$4" > "$5/version"
open "$3"
"#;
    Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .arg("meat-proxy-update")
        .arg(std::process::id().to_string())
        .arg(&staged)
        .arg(&bundle)
        .arg(tag)
        .arg(&support)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn versions_compare_as_numbers() {
        assert!(is_newer("v0.3.10", "0.3.9"));
        assert!(is_newer("v0.4.0", "0.3.99"));
        assert!(!is_newer("v0.3.18", "0.3.18"));
        assert!(!is_newer("v0.3.17", "0.3.18"));
    }

    #[test]
    fn a_dev_build_or_a_odd_tag_is_never_updated() {
        assert!(!is_newer("v0.3.18", "0.3.0 (dev build)"));
        assert!(!is_newer("nightly", "0.3.0"));
        assert_eq!(parse_version("v1.2.3.4"), None);
    }

    #[test]
    fn it_installs_by_itself_only_when_idle_in_the_background() {
        let long = Some(Duration::from_secs(QUIET_FOR_SECS));
        assert!(quiet_moment(long, false));
        assert!(!quiet_moment(long, true), "a merge is running");
        assert!(!quiet_moment(None, false), "the window is focused");
        assert!(
            !quiet_moment(Some(Duration::from_secs(5)), false),
            "it just lost focus"
        );
    }
}
