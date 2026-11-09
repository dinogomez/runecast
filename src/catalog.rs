//! What the launcher can find: installed apps, system commands, a calculator row and web search.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::Config;
use crate::hotkeys::Hotkey;
use crate::wm::Layout;
use crate::{calc, fuzzy};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    App,
    Command,
    Calculator,
    Web,
    Snippet,
    Quicklink,
    Window,
    Clip,
    File,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::App => "Application",
            Kind::Command => "Command",
            Kind::Calculator => "Copy",
            Kind::Web => "Search",
            Kind::Snippet => "Snippet",
            Kind::Quicklink => "Quicklink",
            Kind::Window => "Window",
            Kind::Clip => "Clipboard",
            Kind::File => "File",
        }
    }

    /// What ↩ does, for the footer.
    pub fn verb(self) -> &'static str {
        match self {
            Kind::App => "Open Application",
            Kind::Command => "Run Command",
            Kind::Calculator => "Copy Answer",
            Kind::Web => "Search the Web",
            Kind::Snippet => "Paste Snippet",
            Kind::Quicklink => "Open Quicklink",
            Kind::Window => "Move Window",
            Kind::Clip => "Paste",
            Kind::File => "Open File",
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
    /// Copy, then paste into the app that was in front.
    Paste(String),
    /// Opens `url`; one with `{query}` and nothing typed after its keyword asks for the argument.
    Quicklink { url: String, keyword: String },
    Window(Layout),
    ClipboardHistory,
    FileSearch,
    Settings,
    Quit,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub title: String,
    pub subtitle: String,
    pub kind: Kind,
    pub action: Action,
    /// What `ImageCache::app_icon` draws: an `.app` bundle on macOS, a `.desktop` file on Linux.
    pub icon: Option<PathBuf>,
    /// A shortcut shown at the row's end, such as ⌃⌥← for Left Half.
    pub hotkey: Option<String>,
}

impl Item {
    pub fn new(title: impl Into<String>, subtitle: impl Into<String>, kind: Kind, action: Action) -> Self {
        Item { title: title.into(), subtitle: subtitle.into(), kind, action, icon: None, hotkey: None }
    }
}

pub const MAX_RESULTS: usize = 50;
/// File matches shown in the main list; Search Files lists them all.
pub const FILES_IN_ROOT: usize = 3;

/// The rows for `query`: the calculator answer first, then the best fuzzy matches, then `files`,
/// then a search with `engine` (a name and a URL template). `boost` adds to an item's match score
/// for how often and recently it was used. An empty query lists the most used items first so the
/// panel never opens blank.
pub fn search(items: &[Item], query: &str, engine: (&str, &str), boost: impl Fn(&Item) -> i32, files: &[Item]) -> Vec<Item> {
    let query = query.trim();
    if query.is_empty() {
        let mut all: Vec<(i32, &Item)> = items.iter().map(|it| (boost(it), it)).collect();
        all.sort_by_key(|(b, _)| -b);
        return all.into_iter().take(MAX_RESULTS).map(|(_, it)| it.clone()).collect();
    }
    let mut out = Vec::new();
    if let Some(v) = calc::eval(query) {
        let answer = calc::format(v);
        out.push(Item::new(answer.clone(), format!("{query} ="), Kind::Calculator, Action::Copy(answer.replace(',', ""))));
    }
    for it in items {
        if let Action::Quicklink { url, keyword } = &it.action && let Some(arg) = argument(query, keyword) {
            let url = fill(url, arg);
            out.push(Item::new(format!("{}: {arg}", it.title), url.clone(), Kind::Quicklink, Action::Url(url)));
        }
    }
    let mut hits: Vec<(i32, &Item)> = items.iter().filter_map(|it| fuzzy::score(query, &it.title).map(|s| (s + boost(it), it))).collect();
    hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.title.cmp(&b.1.title)));
    out.extend(hits.into_iter().map(|(_, it)| it.clone()));
    out.truncate(MAX_RESULTS - 1 - files.len().min(FILES_IN_ROOT));
    out.extend(files.iter().take(FILES_IN_ROOT).cloned());
    let (name, template) = engine;
    out.push(Item::new(format!("Search {name} for “{query}”"), "Web", Kind::Web, Action::Url(fill(template, query))));
    out.truncate(MAX_RESULTS);
    out
}

/// What's typed after `keyword` and a space, when the query starts with them (ignoring case).
fn argument<'q>(query: &'q str, keyword: &str) -> Option<&'q str> {
    let head = query.get(..keyword.len())?;
    let rest = query[keyword.len()..].strip_prefix(' ')?.trim();
    (head.eq_ignore_ascii_case(keyword) && !rest.is_empty()).then_some(rest)
}

/// `url` with `{query}` replaced by `arg`, encoded for a URL.
pub fn fill(url: &str, arg: &str) -> String {
    url.replace("{query}", &encode(arg))
}

/// Everything searchable on this machine: apps by name, then commands, window layouts,
/// snippets and quicklinks.
pub fn load(config: &Config) -> Vec<Item> {
    let mut items = scan_apps();
    items.sort_by_key(|it| it.title.to_lowercase());
    items.dedup_by(|a, b| a.title == b.title);
    items.extend(commands(config));
    if cfg!(target_os = "macos") {
        items.extend(layouts(config));
    }
    items.extend(extras(config));
    items
}

pub fn layouts(config: &Config) -> impl Iterator<Item = Item> + '_ {
    Layout::ALL.into_iter().map(|l| Item {
        hotkey: config.hotkey_label(Hotkey::Window(l)),
        ..Item::new(l.title(), "Window Management", Kind::Window, Action::Window(l))
    })
}

/// The user's snippets and quicklinks, as rows.
pub fn extras(config: &Config) -> Vec<Item> {
    let snippets = config.snippets.iter().map(|s| {
        let preview = s.text.lines().next().unwrap_or_default();
        Item::new(&s.name, preview, Kind::Snippet, Action::Paste(s.text.clone()))
    });
    let links = config.quicklinks.iter().map(|q| {
        let keyword = q.keyword();
        let hint = if q.url.contains("{query}") { format!("{keyword} …") } else { q.url.clone() };
        Item::new(&q.name, hint, Kind::Quicklink, Action::Quicklink { url: q.url.clone(), keyword })
    });
    snippets.chain(links).collect()
}

/// Presses ⌘V (Ctrl+V on Linux, with xdotool or wtype) in the app in front, a moment after the
/// caller's copy, once that has landed. Pasting on macOS needs Accessibility permission.
pub fn paste_after_copy() {
    let line = if cfg!(target_os = "macos") {
        r#"sleep 0.2; osascript -e 'tell application "System Events" to keystroke "v" using command down'"#
    } else {
        "sleep 0.2; if command -v xdotool >/dev/null; then xdotool key ctrl+v; elif command -v wtype >/dev/null; then wtype -M ctrl v -m ctrl; fi"
    };
    spawn(&Action::Shell(line.into()));
}

/// Runs an item's action that's a process: `OpenPath` and `Shell`. The rest go through Rune's `Tx`.
pub fn spawn(action: &Action) {
    let result = match action {
        Action::OpenPath(path) => Command::new(if cfg!(target_os = "macos") { "open" } else { "xdg-open" }).arg(path).spawn(),
        Action::Shell(line) => Command::new("sh").arg("-c").arg(line).spawn(),
        _ => return,
    };
    if let Err(e) = result {
        eprintln!("runecast: couldn't run {action:?}: {e}");
    }
}

pub fn commands(config: &Config) -> Vec<Item> {
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
    out.push(Item {
        hotkey: config.hotkey_label(Hotkey::Clipboard),
        ..Item::new("Clipboard History", "Runecast", Kind::Command, Action::ClipboardHistory)
    });
    out.push(Item::new("Search Files", "Find files by name", Kind::Command, Action::FileSearch));
    out.push(Item {
        hotkey: Some(if cfg!(target_os = "macos") { "⌘," } else { "Ctrl+," }.into()),
        ..Item::new("Runecast Settings", "Hotkeys, snippets, quicklinks and more", Kind::Command, Action::Settings)
    });
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
        Item { icon: Some(path.to_path_buf()), ..Item::new(name, folder, Kind::App, Action::OpenPath(path.to_path_buf())) }
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
    Some(Item { icon: Some(path.to_path_buf()), ..Item::new(name?, comment, Kind::App, Action::Shell(exec)) })
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
        let rows = search(&items, "12*3", crate::config::ENGINES[0], |_| 0, &[]);
        assert_eq!(rows[0].kind, Kind::Calculator);
        assert_eq!(rows[0].title, "36");
        assert_eq!(rows.last().unwrap().kind, Kind::Web);
        let ddg = search(&items, "rust ui", crate::config::ENGINES[1], |_| 0, &[]);
        assert_eq!(ddg.last().unwrap().title, "Search DuckDuckGo for “rust ui”");
        assert!(matches!(ddg.last().unwrap().action, Action::Url(ref u) if u == "https://duckduckgo.com/?q=rust+ui"));
    }

    #[test]
    fn best_match_ranks_first_and_rows_are_capped() {
        let mut items: Vec<Item> = (0..MAX_RESULTS + 10).map(|i| app(&format!("Shutter {i}"))).collect();
        items.push(app("Terminal"));
        let rows = search(&items, "ter", crate::config::ENGINES[0], |_| 0, &[]);
        assert_eq!(rows[0].title, "Terminal");
        assert_eq!(rows.len(), MAX_RESULTS);
    }

    #[test]
    fn usage_breaks_ties_and_files_go_before_web() {
        let items = [app("Notes"), app("Notion")];
        assert_eq!(search(&items, "no", crate::config::ENGINES[0], |_| 0, &[])[0].title, "Notes");
        let boosted = |it: &Item| if it.title == "Notion" { 40 } else { 0 };
        let rows = search(&items, "no", crate::config::ENGINES[0], boosted, &[]);
        assert_eq!(rows[0].title, "Notion");
        // With no query, the most used comes first.
        assert_eq!(search(&items, "", crate::config::ENGINES[0], boosted, &[])[0].title, "Notion");
        let files: Vec<Item> = (0..5).map(|i| Item::new(format!("notes-{i}.md"), "~", Kind::File, Action::Shell(String::new()))).collect();
        let rows = search(&items, "no", crate::config::ENGINES[0], |_| 0, &files);
        let kinds: Vec<Kind> = rows.iter().map(|r| r.kind).collect();
        assert_eq!(kinds, [Kind::App, Kind::App, Kind::File, Kind::File, Kind::File, Kind::Web]);
    }

    #[test]
    fn quicklink_keyword_takes_an_argument() {
        let items = extras(&crate::config::example());
        let rows = search(&items, "GH rune ui", crate::config::ENGINES[0], |_| 0, &[]);
        assert_eq!(rows[0].title, "Search GitHub: rune ui");
        assert!(matches!(rows[0].action, Action::Url(ref u) if u == "https://github.com/search?q=rune+ui"));
        // Without an argument, or without the space, the keyword is an ordinary search.
        assert!(search(&items, "gh", crate::config::ENGINES[0], |_| 0, &[]).iter().all(|r| !r.title.contains(':')));
        assert!(search(&items, "ghost", crate::config::ENGINES[0], |_| 0, &[]).iter().all(|r| !r.title.contains(':')));
        assert_eq!(fill("https://x.dev/?q={query}", "a&b c"), "https://x.dev/?q=a%26b+c");
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
