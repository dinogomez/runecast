//! How much of their plan's usage limits Claude and ChatGPT (Codex) subscribers have left.
//!
//! Uses the sign-in their CLIs already saved: Claude Code's OAuth token (`~/.claude/.credentials.json`,
//! or the "Claude Code-credentials" Keychain item on macOS) and Codex's (`~/.codex/auth.json`). Each
//! asks the endpoint the CLI's own `/usage` screen reads. Neither endpoint is documented, so replies
//! are read leniently. Tokens go to curl on stdin, never on its command line.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::catalog::{Action, Item, Kind};

/// One rate-limit window, such as Claude's 5-hour session.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub label: String,
    /// Percent of the window used, 0–100.
    pub used: f64,
    /// When it resets, in Unix seconds.
    pub resets_at: Option<i64>,
}

/// One provider's windows, or why they couldn't be read.
#[derive(Clone, Debug)]
pub struct Report {
    pub provider: &'static str,
    /// Where ↩ goes: the provider's own usage page.
    pub page: &'static str,
    pub windows: Result<Vec<Window>, String>,
}

/// Each provider's logo, for rows when its desktop app isn't installed to lend its icon.
pub const LOGOS: [(&str, &[u8]); 2] = [
    ("Claude", include_bytes!("../assets/claude.png")),
    ("ChatGPT", include_bytes!("../assets/chatgpt.png")),
];

const CLAUDE_PAGE: &str = "https://claude.ai/settings/usage";
const CHATGPT_PAGE: &str = "https://chatgpt.com/codex/settings/usage";

/// Both providers, fetched one after the other. Blocks; call it off the UI thread.
pub fn fetch_all() -> Vec<Report> {
    let now = jiff::Timestamp::now().as_second();
    vec![
        Report { provider: "Claude", page: CLAUDE_PAGE, windows: claude() },
        Report { provider: "ChatGPT", page: CHATGPT_PAGE, windows: codex(now) },
    ]
}

/// A row per window with its provider, which heads its group; a provider that failed gets one
/// row saying why. ↩ on any of them opens the provider's usage page.
pub fn items(reports: &[Report], now: i64) -> Vec<(&'static str, Item)> {
    let mut out = Vec::new();
    for r in reports {
        let row = |title: &str, subtitle: String| Item::new(title, subtitle, Kind::Usage, Action::Url(r.page.into()));
        match &r.windows {
            Ok(windows) if windows.is_empty() => out.push((r.provider, row("No usage limits reported", String::new()))),
            Ok(windows) => out.extend(windows.iter().map(|w| {
                (r.provider, Item { meter: Some(w.left()), ..row(&w.label, w.resets(now).unwrap_or_default()) })
            })),
            Err(e) => {
                let (title, hint) = e.split_once(": ").unwrap_or((e, ""));
                out.push((r.provider, row(title, hint.to_string())));
            }
        }
    }
    out
}

/// Whether a root query asks for usage, so the panel pins the limits above its other rows: a start
/// of "ai usage" (two letters or more) or of "usage" (three or more).
pub fn asked_for(query: &str) -> bool {
    let q = query.trim().to_lowercase();
    (q.len() >= 2 && "ai usage".starts_with(&q)) || (q.len() >= 3 && "usage".starts_with(&q))
}

impl Window {
    /// Percent of the window left, 0–100.
    pub fn left(&self) -> f64 {
        (100.0 - self.used).clamp(0.0, 100.0)
    }

    /// "resets in 2h 14m", once the reset time is known and still ahead.
    pub fn resets(&self, now: i64) -> Option<String> {
        self.resets_at.filter(|&at| at > now).map(|at| format!("resets in {}", span(at - now)))
    }
}

fn span(secs: i64) -> String {
    let (d, h, m) = (secs / 86_400, secs % 86_400 / 3600, (secs % 3600 + 59) / 60);
    match (d, h) {
        (0, 0) => format!("{}m", m.max(1)),
        (0, _) => format!("{h}h {m}m"),
        _ => format!("{d}d {h}h"),
    }
}

// ------------------------------------------------------------------------------------------
// Claude

fn claude() -> Result<Vec<Window>, String> {
    let creds = claude_credentials().ok_or("Not signed in: run `claude` and log in with your Claude plan")?;
    let token = serde_json::from_str::<Value>(&creds).ok()
        .and_then(|v| v["claudeAiOauth"]["accessToken"].as_str().map(str::to_string))
        .ok_or("Claude Code's sign-in has no plan token (API key logins have no usage limits)")?;
    let body = get("https://api.anthropic.com/api/oauth/usage", &[
        format!("Authorization: Bearer {token}"),
        "anthropic-beta: oauth-2025-04-20".into(),
    ])?;
    parse_claude(&body)
}

fn claude_credentials() -> Option<String> {
    let dir = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).or_else(|| home().map(|h| h.join(".claude")))?;
    if let Ok(text) = std::fs::read_to_string(dir.join(".credentials.json")) {
        return Some(text);
    }
    if !cfg!(target_os = "macos") {
        return None;
    }
    let out = Command::new("security").args(["find-generic-password", "-s", "Claude Code-credentials", "-w"]).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The windows Claude's own usage screen shows; other keys in the reply are internal.
const CLAUDE_WINDOWS: [(&str, &str); 4] = [
    ("five_hour", "5-hour session"),
    ("seven_day", "Weekly"),
    ("seven_day_opus", "Weekly Opus"),
    ("seven_day_sonnet", "Weekly Sonnet"),
];

fn parse_claude(body: &str) -> Result<Vec<Window>, String> {
    let v: Value = serde_json::from_str(body).map_err(|_| "Claude sent an unexpected reply")?;
    Ok(CLAUDE_WINDOWS.iter().filter_map(|(key, label)| {
        let w = &v[key];
        let resets_at = w["resets_at"].as_str().and_then(|t| t.parse::<jiff::Timestamp>().ok()).map(|t| t.as_second());
        Some(Window { label: label.to_string(), used: w["utilization"].as_f64()?, resets_at })
    }).collect())
}

// ------------------------------------------------------------------------------------------
// ChatGPT (Codex)

fn codex(now: i64) -> Result<Vec<Window>, String> {
    let dir = std::env::var_os("CODEX_HOME").map(PathBuf::from).or_else(|| home().map(|h| h.join(".codex")));
    let auth = dir.and_then(|d| std::fs::read_to_string(d.join("auth.json")).ok())
        .ok_or("Not signed in: run `codex` and sign in with ChatGPT")?;
    let auth: Value = serde_json::from_str(&auth).map_err(|_| "Codex's auth.json isn't readable")?;
    let tokens = &auth["tokens"];
    let token = tokens["access_token"].as_str().ok_or("Codex uses an API key, not a ChatGPT plan")?;
    let mut headers = vec![format!("Authorization: Bearer {token}"), "Accept: application/json".into()];
    if let Some(account) = tokens["account_id"].as_str() {
        headers.push(format!("ChatGPT-Account-Id: {account}"));
    }
    parse_codex(&get("https://chatgpt.com/backend-api/wham/usage", &headers)?, now)
}

fn parse_codex(body: &str, now: i64) -> Result<Vec<Window>, String> {
    let v: Value = serde_json::from_str(body).map_err(|_| "ChatGPT sent an unexpected reply")?;
    let limits = &v["rate_limit"];
    Ok(["primary_window", "secondary_window"].iter().filter_map(|key| {
        let w = &limits[key];
        let used = w["used_percent"].as_f64()?;
        let resets_at = w["reset_at"].as_i64().filter(|&t| t > 0)
            .or_else(|| w["reset_after_seconds"].as_i64().map(|s| now + s));
        // The windows aren't named, only sized: 18000 s is the 5-hour one, 604800 s the week.
        let label = match w["limit_window_seconds"].as_i64() {
            Some(s) if s >= 6 * 86_400 && s <= 8 * 86_400 => "Weekly".to_string(),
            Some(s) if s >= 86_400 => format!("{}-day", s / 86_400),
            Some(s) if s > 0 => format!("{}-hour session", (s + 1800) / 3600),
            _ if *key == "primary_window" => "Session".to_string(),
            _ => "Weekly".to_string(),
        };
        Some(Window { label, used, resets_at })
    }).collect())
}

// ------------------------------------------------------------------------------------------

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// GETs `url` with curl, passing it and the headers on stdin so tokens stay out of `ps`.
fn get(url: &str, headers: &[String]) -> Result<String, String> {
    let mut child = Command::new("curl").args(["-s", "--max-time", "10", "-w", "\n%{http_code}", "-K", "-"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
        .spawn().map_err(|_| "curl isn't installed")?;
    let mut config = format!("url = \"{url}\"\n");
    for h in headers {
        config += &format!("header = \"{h}\"\n");
    }
    child.stdin.take().unwrap().write_all(config.as_bytes()).map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout);
    let (body, status) = text.rsplit_once('\n').unwrap_or(("", &text));
    match status.trim() {
        "200" => Ok(body.to_string()),
        "401" | "403" => Err("Sign-in expired: open the CLI once to refresh it".into()),
        "429" => Err("Rate limited; try again in a minute".into()),
        "000" => Err("Couldn't reach the server".into()),
        code => Err(format!("The server answered {code}")),
    }
}

/// Fixed readings for headless scenes.
pub fn demo(now: i64) -> Vec<Report> {
    let w = |label: &str, used: f64, secs: i64| Window { label: label.into(), used, resets_at: Some(now + secs) };
    vec![
        Report { provider: "Claude", page: CLAUDE_PAGE, windows: Ok(vec![w("5-hour session", 38.0, 2 * 3600 + 14 * 60), w("Weekly", 61.0, 3 * 86_400 + 5 * 3600), w("Weekly Opus", 88.0, 3 * 86_400 + 5 * 3600)]) },
        Report { provider: "ChatGPT", page: CHATGPT_PAGE, windows: Ok(vec![w("5-hour session", 12.0, 4 * 3600 + 2 * 60), w("Weekly", 27.0, 5 * 86_400 + 9 * 3600)]) },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_reads_known_windows_and_skips_empty_ones() {
        let body = r#"{
            "five_hour": {"utilization": 33.0, "resets_at": "2026-11-09T07:00:00+00:00"},
            "seven_day": {"utilization": 71, "resets_at": "2026-11-12T18:00:00Z"},
            "seven_day_opus": null,
            "seven_day_sonnet": {"utilization": null, "resets_at": null},
            "iguana_necktie": {"utilization": 5}
        }"#;
        let windows = parse_claude(body).unwrap();
        assert_eq!(windows, [
            Window { label: "5-hour session".into(), used: 33.0, resets_at: Some(1_794_207_600) },
            Window { label: "Weekly".into(), used: 71.0, resets_at: Some(1_794_506_400) },
        ]);
    }

    #[test]
    fn codex_names_windows_by_size_and_falls_back_to_reset_after() {
        let body = r#"{"plan_type": "plus", "rate_limit": {
            "primary_window": {"used_percent": 4, "limit_window_seconds": 18000, "reset_after_seconds": 3600},
            "secondary_window": {"used_percent": 12.5, "limit_window_seconds": 604800, "reset_at": 1762970400}
        }}"#;
        let windows = parse_codex(body, 1_000).unwrap();
        assert_eq!(windows, [
            Window { label: "5-hour session".into(), used: 4.0, resets_at: Some(4_600) },
            Window { label: "Weekly".into(), used: 12.5, resets_at: Some(1_762_970_400) },
        ]);
        // Only a weekly window (as some plans now report) is labelled weekly, not session.
        let weekly_only = r#"{"rate_limit": {"primary_window": {"used_percent": 1, "limit_window_seconds": 604800, "reset_at": -1}}}"#;
        assert_eq!(parse_codex(weekly_only, 0).unwrap(), [Window { label: "Weekly".into(), used: 1.0, resets_at: None }]);
    }

    #[test]
    fn windows_say_what_is_left_and_when_it_resets() {
        let w = Window { label: "Weekly".into(), used: 61.4, resets_at: Some(100 + 3 * 86_400 + 5 * 3600) };
        assert_eq!((w.left().round(), w.resets(100).as_deref()), (39.0, Some("resets in 3d 5h")));
        assert_eq!(w.resets(w.resets_at.unwrap()), None, "a reset that has passed isn't shown");
        assert_eq!(Window { used: 104.0, ..w.clone() }.left(), 0.0);
        let reports = [
            Report { provider: "Claude", page: CLAUDE_PAGE, windows: Ok(vec![w.clone()]) },
            Report { provider: "ChatGPT", page: CHATGPT_PAGE, windows: Err("Not signed in: run `codex` and sign in with ChatGPT".into()) },
        ];
        let rows: Vec<_> = items(&reports, 100).into_iter().map(|(p, it)| (p, it.title, it.subtitle, it.meter)).collect();
        assert_eq!(rows, [
            ("Claude", "Weekly".into(), "resets in 3d 5h".into(), Some(w.left())),
            ("ChatGPT", "Not signed in".into(), "run `codex` and sign in with ChatGPT".into(), None),
        ]);
        assert_eq!(span(2 * 3600 + 14 * 60), "2h 14m");
        assert_eq!(span(30), "1m");
    }

    #[test]
    fn only_a_start_of_ai_usage_or_usage_asks_for_it() {
        for q in ["ai", "AI Us", " ai usage ", "usa", "usage"] {
            assert!(asked_for(q), "{q:?}");
        }
        for q in ["a", "us", "aiu", "ai usage now", "music"] {
            assert!(!asked_for(q), "{q:?}");
        }
    }
}
