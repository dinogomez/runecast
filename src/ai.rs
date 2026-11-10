//! How much of their plan's usage limits Claude and ChatGPT (Codex) subscribers have left.
//!
//! Uses the sign-in their CLIs already saved: Claude Code's OAuth token (`~/.claude/.credentials.json`,
//! or the "Claude Code-credentials" Keychain item on macOS) and Codex's (`~/.codex/auth.json`). Each
//! asks the endpoint the CLI's own `/usage` screen reads. Neither endpoint is documented, so replies
//! are read leniently. Tokens go to curl on stdin, never on its command line.
//!
//! Claude's endpoint locks a token out for many minutes after a handful of calls, so, like CodexBar
//! and ccstatusline, readings are reused for a few minutes, each 429 doubles the wait, and the last
//! good reading stays on screen with its age while a provider can't be asked.

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
    /// When `windows` was read, in Unix seconds.
    pub read_at: i64,
}

/// What a fetch brings back: each asked provider, by index into `PROVIDERS`, and its windows.
pub type Fetched = Vec<(usize, Fetch)>;
type Fetch = Result<Vec<Window>, String>;
/// A provider's name, usage page, and reader.
type Provider = (&'static str, &'static str, fn() -> Fetch);

/// Each provider's last reading, kept between openings so AI Usage shows it at once, and when
/// each may be asked again.
#[derive(Clone, Debug, Default)]
pub struct Readings {
    pub reports: Vec<Report>,
    /// Per provider: when it may be asked again, and the 429s in a row.
    retry: [(i64, u32); 2],
}

/// Each provider's logo, for rows when its desktop app isn't installed to lend its icon.
pub const LOGOS: [(&str, &[u8]); 2] = [
    ("Claude", include_bytes!("../assets/claude.png")),
    ("ChatGPT", include_bytes!("../assets/chatgpt.png")),
];

const CLAUDE_PAGE: &str = "https://claude.ai/settings/usage";
const CHATGPT_PAGE: &str = "https://chatgpt.com/codex/settings/usage";

/// The providers, in display order.
const PROVIDERS: [Provider; 2] = [
    ("Claude", CLAUDE_PAGE, claude),
    ("ChatGPT", CHATGPT_PAGE, codex),
];

/// How long a reading is reused before asking again, in seconds; ccstatusline settles on 3 minutes.
const FRESH: i64 = 180;
/// The wait after a first 429, doubled for each one after, up to `MAX_BACKOFF`.
const BACKOFF: i64 = 300;
const MAX_BACKOFF: i64 = 1800;
/// The wait after any other failure, so a typed query doesn't ask again on every keystroke.
const RETRY: i64 = 30;

const RATE_LIMITED: &str = "Rate limited";
const UNREACHABLE: &str = "Couldn't reach the server";

/// Asks the given providers at once, each on its own thread. Blocks; call it off the UI thread.
pub fn fetch(due: Vec<usize>) -> Fetched {
    std::thread::scope(|s| {
        let asked: Vec<_> = due.into_iter().map(|i| (i, s.spawn(PROVIDERS[i].2))).collect();
        asked.into_iter().map(|(i, h)| (i, h.join().unwrap_or_else(|_| Err("Couldn't check usage".into())))).collect()
    })
}

impl Readings {
    /// The providers whose readings are due a fetch, by index.
    pub fn due(&self, now: i64) -> Vec<usize> {
        (0..PROVIDERS.len()).filter(|&i| self.retry[i].0 <= now).collect()
    }

    /// Takes in a fetch that landed at `now`, and sets when each provider may be asked again.
    pub fn update(&mut self, fetched: Fetched, now: i64) {
        for (i, windows) in fetched {
            let (provider, page, _) = PROVIDERS[i];
            let (retry_at, fails) = &mut self.retry[i];
            let failed = windows.as_ref().err().map(String::as_str);
            *fails = if failed == Some(RATE_LIMITED) { *fails + 1 } else { 0 };
            *retry_at = now + match failed {
                None => FRESH,
                Some(RATE_LIMITED) => (BACKOFF << (*fails - 1).min(3)).min(MAX_BACKOFF),
                Some(_) => RETRY,
            };
            let slot = self.reports.iter_mut().find(|r| r.provider == provider);
            // A rate limit or a dropped connection keeps the last good reading; its age shows instead.
            let had_reading = slot.as_ref().is_some_and(|r| r.windows.is_ok());
            if had_reading && matches!(failed, Some(RATE_LIMITED | UNREACHABLE)) {
                continue;
            }
            let report = Report { provider, page, windows, read_at: now };
            match slot {
                Some(r) => *r = report,
                None => self.reports.push(report),
            }
        }
    }

    /// A row per window with its provider, which heads its group; a provider that failed gets one
    /// row saying why. ↩ on any of them opens the provider's usage page.
    pub fn items(&self, now: i64) -> Vec<(&'static str, Item)> {
        let mut out = Vec::new();
        for r in &self.reports {
            let row = |title: &str, subtitle: String| Item::new(title, subtitle, Kind::Usage, Action::Url(r.page.into()));
            // A reading kept past its freshness, because the provider couldn't be asked, says how old it is.
            let age = (now - r.read_at >= FRESH).then(|| format!("checked {} ago", span(now - r.read_at)));
            match &r.windows {
                Ok(windows) if windows.is_empty() => out.push((r.provider, row("No usage limits reported", String::new()))),
                Ok(windows) => out.extend(windows.iter().map(|w| {
                    let subtitle = [w.resets(now), age.clone()].into_iter().flatten().collect::<Vec<_>>().join(" · ");
                    (r.provider, Item { meter: Some(w.left()), ..row(&w.label, subtitle) })
                })),
                Err(e) if e == RATE_LIMITED => {
                    let at = PROVIDERS.iter().position(|p| p.0 == r.provider).map_or(now, |i| self.retry[i].0);
                    let next = if at > now { format!("in {}", span(at - now)) } else { "now".into() };
                    out.push((r.provider, row(RATE_LIMITED, format!("{} allows few usage checks; next try {next}", r.provider))));
                }
                Err(e) => {
                    let (title, hint) = e.split_once(": ").unwrap_or((e, ""));
                    out.push((r.provider, row(title, hint.to_string())));
                }
            }
        }
        out
    }
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
        // Requests not from Claude Code land in a far stricter rate limit; CodexBar sends the same.
        format!("User-Agent: claude-cli/{} (external, cli)", claude_code_version()),
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

/// The installed Claude Code's version, from `claude --version`, read once per run; "2.1.0" when it
/// can't be found. A launcher opened from the Dock has a bare PATH, so the usual install spots are
/// tried too.
fn claude_code_version() -> &'static str {
    static VERSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    VERSION.get_or_init(|| {
        let home = home().unwrap_or_default();
        let candidates = [
            PathBuf::from("claude"),
            home.join(".local/bin/claude"),
            home.join(".claude/local/claude"),
            "/opt/homebrew/bin/claude".into(),
            "/usr/local/bin/claude".into(),
        ];
        candidates.iter().find_map(|bin| {
            let out = Command::new(bin).arg("--version").stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
            let text = String::from_utf8_lossy(&out.stdout);
            out.status.success().then(|| parse_version(&text).map(str::to_string))?
        }).unwrap_or_else(|| "2.1.0".into())
    })
}

/// "2.1.322" from "2.1.322 (Claude Code)".
fn parse_version(out: &str) -> Option<&str> {
    let v = out.split_whitespace().next()?;
    (v.contains('.') && v.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))).then_some(v)
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

fn codex() -> Result<Vec<Window>, String> {
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
    parse_codex(&get("https://chatgpt.com/backend-api/wham/usage", &headers)?, jiff::Timestamp::now().as_second())
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
        "429" => Err(RATE_LIMITED.into()),
        "000" => Err(UNREACHABLE.into()),
        code => Err(format!("The server answered {code}")),
    }
}

/// Fixed readings for headless scenes.
pub fn demo(now: i64) -> Readings {
    let w = |label: &str, used: f64, secs: i64| Window { label: label.into(), used, resets_at: Some(now + secs) };
    let mut readings = Readings::default();
    readings.update(vec![
        (0, Ok(vec![w("5-hour session", 38.0, 2 * 3600 + 14 * 60), w("Weekly", 61.0, 3 * 86_400 + 5 * 3600), w("Weekly Opus", 88.0, 3 * 86_400 + 5 * 3600)])),
        (1, Ok(vec![w("5-hour session", 12.0, 4 * 3600 + 2 * 60), w("Weekly", 27.0, 5 * 86_400 + 9 * 3600)])),
    ], now);
    readings
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
        assert_eq!(parse_version("2.1.322 (Claude Code)\n"), Some("2.1.322"));
        assert_eq!(parse_version("command not found: node"), None);
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
        let mut readings = Readings::default();
        readings.update(vec![(0, Ok(vec![w.clone()])), (1, Err("Not signed in: run `codex` and sign in with ChatGPT".into()))], 100);
        let rows: Vec<_> = readings.items(100).into_iter().map(|(p, it)| (p, it.title, it.subtitle, it.meter)).collect();
        assert_eq!(rows, [
            ("Claude", "Weekly".into(), "resets in 3d 5h".into(), Some(w.left())),
            ("ChatGPT", "Not signed in".into(), "run `codex` and sign in with ChatGPT".into(), None),
        ]);
        assert_eq!(span(2 * 3600 + 14 * 60), "2h 14m");
        assert_eq!(span(30), "1m");
    }

    #[test]
    fn readings_are_reused_and_rate_limits_back_off_keeping_the_last_reading() {
        let w = |used: f64| Window { label: "Weekly".into(), used, resets_at: None };
        let mut r = Readings::default();
        assert_eq!(r.due(0), [0, 1], "nothing read yet");
        r.update(vec![(0, Ok(vec![w(40.0)])), (1, Ok(vec![w(10.0)]))], 0);
        assert!(r.due(FRESH - 1).is_empty(), "a fresh reading is reused");
        assert_eq!(r.due(FRESH), [0, 1]);

        // Claude answers 429: its last reading stays, aged, and it waits 5 minutes; ChatGPT doesn't.
        r.update(vec![(0, Err(RATE_LIMITED.into())), (1, Ok(vec![w(11.0)]))], 200);
        let rows: Vec<_> = r.items(200).into_iter().map(|(p, it)| (p, it.subtitle, it.meter)).collect();
        assert_eq!(rows, [("Claude", "checked 4m ago".into(), Some(60.0)), ("ChatGPT", String::new(), Some(89.0))]);
        assert_eq!(r.due(200 + FRESH), [1]);
        assert_eq!(r.due(200 + 300), [0, 1]);
        // Each 429 in a row doubles the wait, up to half an hour.
        for (at, wait) in [(500, 600), (1100, 1200), (2300, 1800), (4100, 1800)] {
            r.update(vec![(0, Err(RATE_LIMITED.into()))], at);
            assert!(!r.due(at + wait - 1).contains(&0) && r.due(at + wait).contains(&0), "after the 429 at {at}");
        }
        // A reading resets the backoff.
        r.update(vec![(0, Ok(vec![w(50.0)]))], 6000);
        r.update(vec![(0, Err(RATE_LIMITED.into()))], 6000 + FRESH);
        assert!(r.due(6000 + FRESH + 300).contains(&0));

        // With no reading to keep, the row says so and when it tries again.
        let mut r = Readings::default();
        r.update(vec![(0, Err(RATE_LIMITED.into()))], 0);
        let (_, row) = &r.items(60)[0];
        assert_eq!((row.title.as_str(), row.subtitle.as_str()), ("Rate limited", "Claude allows few usage checks; next try in 4m"));
        // A sign-in failure replaces a reading rather than hiding behind it.
        r.update(vec![(0, Ok(vec![w(1.0)]))], 400);
        r.update(vec![(0, Err("Sign-in expired: open the CLI once to refresh it".into()))], 600);
        assert_eq!(r.items(600)[0].1.title, "Sign-in expired");
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
