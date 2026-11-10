//! Updates: once a day Runecast asks GitHub for the latest release, and when it's newer than this
//! build the menu bar offers it. On macOS picking it downloads the release's zip, checks that the
//! new app is signed by the same Developer ID and accepted by Gatekeeper, swaps it in place of the
//! running Runecast.app and relaunches. Elsewhere, or when any of that fails, it opens the release
//! page instead.

#[cfg(target_os = "macos")]
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use rune::AppHandle;
use serde::Deserialize;

use crate::State;

const LATEST_URL: &str = "https://api.github.com/repos/dinogomez/runecast/releases/latest";
const EVERY: Duration = Duration::from_secs(24 * 3600);

/// A release newer than this build.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    /// Without the `v`, such as `0.3.0`.
    pub version: String,
    pub page: String,
    /// The macOS zip's download URL.
    pub zip: Option<String>,
}

impl Release {
    /// The menu item that offers it.
    pub fn label(&self) -> String {
        if cfg!(target_os = "macos") { format!("Update to v{}", self.version) } else { format!("Download v{}…", self.version) }
    }
}

/// Checks a little after launch and then daily until a newer release turns up, then hands it to
/// `on_release`.
pub fn check_in_background(on_release: impl FnOnce(Release) + Send + 'static) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(10));
        loop {
            match latest() {
                Ok(Some(release)) => return on_release(release),
                Ok(None) => {}
                Err(e) => eprintln!("runecast: couldn't check for updates: {e}"),
            }
            std::thread::sleep(EVERY);
        }
    });
}

fn latest() -> Result<Option<Release>, String> {
    let out = Command::new("curl").args(["-sf", "--max-time", "10", "-H", "Accept: application/vnd.github+json", LATEST_URL])
        .output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("GitHub didn't answer ({})", out.status));
    }
    from_reply(&out.stdout, env!("CARGO_PKG_VERSION"))
}

/// The release in GitHub's `releases/latest` reply, if it's newer than `current`. A release still
/// waiting on Apple's notarization is skipped until Staple swaps in the notarized downloads.
fn from_reply(json: &[u8], current: &str) -> Result<Option<Release>, String> {
    #[derive(Deserialize)]
    struct Reply {
        tag_name: String,
        html_url: String,
        #[serde(default)]
        body: Option<String>,
        assets: Vec<Asset>,
    }
    #[derive(Deserialize)]
    struct Asset {
        name: String,
        browser_download_url: String,
    }
    let reply: Reply = serde_json::from_slice(json).map_err(|e| format!("unexpected reply: {e}"))?;
    let version = reply.tag_name.trim_start_matches('v').to_string();
    if !newer(&version, current) || reply.body.unwrap_or_default().contains("<!-- notarization:pending") {
        return Ok(None);
    }
    let zip_name = format!("Runecast-{version}-macos.zip");
    let zip = reply.assets.into_iter().find(|a| a.name == zip_name).map(|a| a.browser_download_url);
    Ok(Some(Release { version, page: reply.html_url, zip }))
}

/// Whether `version` (`major.minor.patch`) is later than `current`; false when either doesn't parse.
fn newer(version: &str, current: &str) -> bool {
    fn parse(v: &str) -> Option<(u64, u64, u64)> {
        let mut parts = v.split('.').map(|p| p.parse().ok());
        let parsed = (parts.next()??, parts.next()??, parts.next()??);
        parts.next().is_none().then_some(parsed)
    }
    matches!((parse(version), parse(current)), (Some(v), Some(c)) if v > c)
}

/// Installs `release` off the UI thread and relaunches, or opens its page when it can't. Hands the
/// release back to the menu if that fails, so it can be tried again.
pub fn install(release: Release, launcher: AppHandle<State>) {
    std::thread::spawn(move || match swap(&release) {
        Ok(()) => launcher.quit(),
        Err(e) => {
            eprintln!("runecast: couldn't install v{}: {e}", release.version);
            let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
            let _ = Command::new(opener).arg(&release.page).spawn();
            launcher.update(move |s, _| s.update = Some(release));
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn swap(_release: &Release) -> Result<(), String> {
    Err("updates install themselves on macOS only".into())
}

/// Replaces the running Runecast.app with the release's and starts a shell that opens the new app
/// once this process has quit.
#[cfg(target_os = "macos")]
fn swap(release: &Release) -> Result<(), String> {
    let zip_url = release.zip.as_deref().ok_or("the release has no macOS zip")?;
    let app = running_app().ok_or("Runecast isn't running from an app bundle")?;
    let team = team_id(&app).ok_or("this copy isn't signed with a Developer ID")?;
    // Staged beside the app so the swap is two renames on one volume.
    let staging = app.parent().ok_or("the app has no folder")?.join(".runecast-update");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| format!("can't write beside {}: {e}", app.display()))?;
    let zip = staging.join("update.zip");
    run(Command::new("curl").args(["-sfL", "--max-time", "300", "-o"]).arg(&zip).arg(zip_url))?;
    run(Command::new("ditto").args(["-x", "-k"]).arg(&zip).arg(&staging))?;
    let new = staging.join("Runecast.app");
    run(Command::new("codesign").args(["--verify", "--deep", "--strict"]).arg(&new))?;
    if team_id(&new).as_deref() != Some(&team) {
        return Err("the download isn't signed by the same developer".into());
    }
    run(Command::new("spctl").args(["--assess", "--type", "execute"]).arg(&new))?;
    let old = staging.join("Runecast-old.app");
    std::fs::rename(&app, &old).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::rename(&new, &app) {
        let _ = std::fs::rename(&old, &app);
        return Err(e.to_string());
    }
    // The old copy stays until this process is gone, since it still loads files from it.
    let script = r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done; rm -rf "$2"; open "$3""#;
    Command::new("/bin/sh").args(["-c", script, "sh", &std::process::id().to_string()]).arg(&staging).arg(&app)
        .spawn().map_err(|e| e.to_string())?;
    Ok(())
}

/// The `.app` this process runs from: `Runecast.app/Contents/MacOS/runecast`.
#[cfg(target_os = "macos")]
fn running_app() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let app = exe.ancestors().nth(3)?;
    (app.extension()? == "app" && exe.parent()?.ends_with("Contents/MacOS")).then(|| app.to_path_buf())
}

/// The Developer ID team that signed `app`; `None` when it's ad-hoc signed or unsigned.
#[cfg(target_os = "macos")]
fn team_id(app: &Path) -> Option<String> {
    let out = Command::new("codesign").args(["-dv", "--verbose=2"]).arg(app).output().ok()?;
    let info = String::from_utf8_lossy(&out.stderr);
    let team = info.lines().find_map(|l| l.strip_prefix("TeamIdentifier="))?.trim();
    (team != "not set").then(|| team.to_string())
}

#[cfg(target_os = "macos")]
fn run(cmd: &mut Command) -> Result<(), String> {
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        return Ok(());
    }
    let program = cmd.get_program().to_string_lossy().into_owned();
    Err(format!("{program} failed: {}", String::from_utf8_lossy(&out.stderr).trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number_not_text() {
        assert!(newer("0.10.0", "0.9.0"));
        assert!(newer("1.0.0", "0.99.99"));
        assert!(!newer("0.2.0", "0.2.0"));
        assert!(!newer("0.1.9", "0.2.0"));
        assert!(!newer("0.3", "0.2.0"));
        assert!(!newer("0.3.0-beta", "0.2.0"));
    }

    #[test]
    fn offers_a_newer_release_once_apple_has_notarized_it() {
        let reply = |body: &str| format!(r#"{{
            "tag_name": "v0.3.0", "html_url": "https://github.com/dinogomez/runecast/releases/tag/v0.3.0",
            "body": {body:?},
            "assets": [
                {{"name": "runecast-0.3.0-linux-x86_64.tar.gz", "browser_download_url": "https://x/linux.tar.gz"}},
                {{"name": "Runecast-0.3.0-macos.zip", "browser_download_url": "https://x/macos.zip"}}
            ]
        }}"#);
        let accepted = reply("<!-- notarization:accepted -->\nSigned with a Developer ID and notarized by Apple.");
        assert_eq!(from_reply(accepted.as_bytes(), "0.2.0").unwrap(), Some(Release {
            version: "0.3.0".into(),
            page: "https://github.com/dinogomez/runecast/releases/tag/v0.3.0".into(),
            zip: Some("https://x/macos.zip".into()),
        }));
        assert_eq!(from_reply(accepted.as_bytes(), "0.3.0").unwrap(), None);
        let pending = reply("<!-- notarization:pending:0b5a2e1c-1111-2222-3333-444455556666 -->\n> [!NOTE]");
        assert_eq!(from_reply(pending.as_bytes(), "0.2.0").unwrap(), None);
    }
}
