//! What the launcher can find: installed apps, system commands, a calculator row and web search.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{calc, fuzzy};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    App,
    Command,
    Calculator,
    Web,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::App => "Application",
            Kind::Command => "Command",
            Kind::Calculator => "Copy",
            Kind::Web => "Search",
        }
    }
}

#[derive(Clone, Debug)]
pub enum Action {
    /// A macOS `.app` bundle, opened with `open`.
    OpenPath(PathBuf),
    /// A shell command line: a Linux `.desktop` Exec line or a system command.
    Shell(String),
    Url(String),
    Copy(String),
    Quit,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub title: String,
    pub subtitle: String,
    pub kind: Kind,
    pub action: Action,
}

impl Item {
    fn new(title: impl Into<String>, subtitle: impl Into<String>, kind: Kind, action: Action) -> Self {
        Item { title: title.into(), subtitle: subtitle.into(), kind, action }
    }
}

pub const MAX_RESULTS: usize = 8;

/// The rows for `query`: the calculator answer first, then the best fuzzy matches, then a web search.
/// An empty query lists the first apps and commands so the panel never opens blank.
pub fn search(items: &[Item], query: &str) -> Vec<Item> {
    let query = query.trim();
    if query.is_empty() {
        return items.iter().take(MAX_RESULTS).cloned().collect();
    }
    let mut out = Vec::new();
    if let Some(v) = calc::eval(query) {
        let answer = calc::format(v);
        out.push(Item::new(answer.clone(), format!("{query} ="), Kind::Calculator, Action::Copy(answer.replace(',', ""))));
    }
    let mut hits: Vec<(i32, &Item)> = items.iter().filter_map(|it| fuzzy::score(query, &it.title).map(|s| (s, it))).collect();
    hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.title.cmp(&b.1.title)));
    out.extend(hits.into_iter().take(MAX_RESULTS - 1).map(|(_, it)| it.clone()));
    let url = format!("https://www.google.com/search?q={}", encode(query));
    out.push(Item::new(format!("Search Google for “{query}”"), "Web", Kind::Web, Action::Url(url)));
    out.truncate(MAX_RESULTS);
    out
}

/// Everything searchable on this machine: apps by name, then the built-in commands.
pub fn load() -> Vec<Item> {
    let mut items = scan_apps();
    items.sort_by_key(|it| it.title.to_lowercase());
    items.dedup_by(|a, b| a.title == b.title);
    items.extend(commands());
    items
}

/// Runs an item's action that needs the OS. `Url`, `Copy` and `Quit` go through Rune's `Tx` instead.
pub fn spawn(action: &Action) {
    let result = match action {
        Action::OpenPath(path) => Command::new("open").arg(path).spawn(),
        Action::Shell(line) => Command::new("sh").arg("-c").arg(line).spawn(),
        _ => return,
    };
    if let Err(e) = result {
        eprintln!("runecast: couldn't run {action:?}: {e}");
    }
}

fn commands() -> Vec<Item> {
    let mut out = Vec::new();
    let mut cmd = |title: &str, line: &str| out.push(Item::new(title, "System", Kind::Command, Action::Shell(line.into())));
    if cfg!(target_os = "macos") {
        cmd("Lock Screen", "pmset displaysleepnow");
        cmd("Sleep", "pmset sleepnow");
        cmd("Empty Trash", r#"osascript -e 'tell application "Finder" to empty trash'"#);
        cmd("Toggle Dark Mode", r#"osascript -e 'tell application "System Events" to tell appearance preferences to set dark mode to not dark mode'"#);
        cmd("Open Downloads", "open ~/Downloads");
    } else {
        cmd("Lock Screen", "loginctl lock-session");
        cmd("Sleep", "systemctl suspend");
        cmd("Open Downloads", "xdg-open ~/Downloads");
    }
    out.push(Item::new("Quit Runecast", "Runecast", Kind::Command, Action::Quit));
    out
}

fn scan_apps() -> Vec<Item> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    if cfg!(target_os = "macos") {
        let roots = [
            PathBuf::from("/Applications"),
            PathBuf::from("/Applications/Utilities"),
            PathBuf::from("/System/Applications"),
            PathBuf::from("/System/Applications/Utilities"),
            home.join("Applications"),
        ];
        roots.iter().flat_map(|r| read_dir(r)).filter_map(|p| mac_app(&p)).collect()
    } else {
        let data_home = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or(home.join(".local/share"));
        let data_dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
        std::iter::once(data_home)
            .chain(data_dirs.split(':').map(PathBuf::from))
            .flat_map(|d| read_dir(&d.join("applications")))
            .filter_map(|p| desktop_entry(&p))
            .collect()
    }
}

fn read_dir(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.path()).collect()).unwrap_or_default()
}

fn mac_app(path: &Path) -> Option<Item> {
    (path.extension()? == "app").then(|| {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let folder = path.parent().map(|p| p.display().to_string()).unwrap_or_default();
        Item::new(name, folder, Kind::App, Action::OpenPath(path.to_path_buf()))
    })
}

/// A visible application from a freedesktop `.desktop` file, launched from its Exec line.
fn desktop_entry(path: &Path) -> Option<Item> {
    if path.extension()? != "desktop" {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let (mut name, mut exec, mut comment) = (None, None, String::new());
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        let Some((key, value)) = line.split_once('=').filter(|_| in_entry) else { continue };
        match key {
            "Name" => name = Some(value.to_string()),
            "Exec" => exec = Some(value.to_string()),
            "Comment" => comment = value.to_string(),
            "NoDisplay" | "Hidden" if value == "true" => return None,
            "Type" if value != "Application" => return None,
            _ => {}
        }
    }
    // Field codes (%f, %U, …) are for files the launcher would pass; there are none.
    let exec: String = exec?.split_whitespace().filter(|w| !w.starts_with('%')).collect::<Vec<_>>().join(" ");
    Some(Item::new(name?, comment, Kind::App, Action::Shell(exec)))
}

fn encode(s: &str) -> String {
    s.bytes().map(|b| match b {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
        b' ' => "+".into(),
        _ => format!("%{b:02X}"),
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str) -> Item {
        Item::new(name, "", Kind::App, Action::Shell(String::new()))
    }

    #[test]
    fn calculator_first_web_search_last() {
        let items = [app("Safari"), app("Calendar")];
        let rows = search(&items, "12*3");
        assert_eq!(rows[0].kind, Kind::Calculator);
        assert_eq!(rows[0].title, "36");
        assert_eq!(rows.last().unwrap().kind, Kind::Web);
    }

    #[test]
    fn best_match_ranks_first_and_rows_are_capped() {
        let mut items: Vec<Item> = (0..20).map(|i| app(&format!("Shutter {i}"))).collect();
        items.push(app("Terminal"));
        let rows = search(&items, "ter");
        assert_eq!(rows[0].title, "Terminal");
        assert_eq!(rows.len(), MAX_RESULTS);
    }

    #[test]
    fn desktop_entries_drop_field_codes_and_hidden_apps() {
        let dir = std::env::temp_dir().join(format!("runecast-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let shown = dir.join("code.desktop");
        std::fs::write(&shown, "[Desktop Entry]\nType=Application\nName=Code\nExec=code --new-window %F\n[Desktop Action new]\nName=New Window\n").unwrap();
        let hidden = dir.join("hidden.desktop");
        std::fs::write(&hidden, "[Desktop Entry]\nType=Application\nName=Helper\nExec=helper\nNoDisplay=true\n").unwrap();
        let item = desktop_entry(&shown).unwrap();
        assert_eq!(item.title, "Code");
        assert!(matches!(item.action, Action::Shell(ref s) if s == "code --new-window"));
        assert!(desktop_entry(&hidden).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
