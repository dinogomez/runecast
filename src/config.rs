//! Runecast's settings: general options, hotkeys, snippets and quicklinks, in `config.json` in
//! Runecast's folder. The Settings window writes it on every change; the launcher reads it again
//! each time the panel opens, so hand edits apply without a restart. A first run writes defaults.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::hotkeys::Hotkey;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub general: General,
    /// Accelerators by hotkey id ("launcher", "window.left-half"), as `rune::Shortcut` writes
    /// them. An empty string turns a hotkey off; a missing id takes its default.
    pub hotkeys: BTreeMap<String, String>,
    pub snippets: Vec<Snippet>,
    pub quicklinks: Vec<Quicklink>,
    pub aliases: Vec<Alias>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct General {
    pub appearance: Appearance,
    /// A name from `ENGINES`.
    pub search_engine: String,
    pub clipboard_history: bool,
    pub clipboard_size: usize,
}

impl Default for General {
    fn default() -> Self {
        General { appearance: Appearance::System, search_engine: ENGINES[0].0.into(), clipboard_history: true, clipboard_size: 200 }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

impl Appearance {
    pub const ALL: [Appearance; 3] = [Appearance::System, Appearance::Light, Appearance::Dark];

    pub fn dark(self, system_dark: bool) -> bool {
        match self {
            Appearance::System => system_dark,
            Appearance::Light => false,
            Appearance::Dark => true,
        }
    }
}

/// Web search engines by name, with `{query}` where the search goes.
pub const ENGINES: [(&str, &str); 5] = [
    ("Google", "https://www.google.com/search?q={query}"),
    ("DuckDuckGo", "https://duckduckgo.com/?q={query}"),
    ("Bing", "https://www.bing.com/search?q={query}"),
    ("Kagi", "https://kagi.com/search?q={query}"),
    ("Perplexity", "https://www.perplexity.ai/search?q={query}"),
];

pub const CLIPBOARD_SIZES: [usize; 4] = [50, 100, 200, 500];

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Snippet {
    pub name: String,
    pub text: String,
}

/// A short name for an app or command: typing `alias` puts the item titled `target` first.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Alias {
    pub alias: String,
    pub target: String,
}

/// A URL opened by name. `{query}` in the URL takes what's typed after its keyword:
/// "gh rune" opens GitHub's search for "rune".
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Quicklink {
    pub name: String,
    pub url: String,
    /// What to type before the argument; the name, lowercased, when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyword: Option<String>,
}

impl Quicklink {
    pub fn keyword(&self) -> String {
        self.keyword.clone().filter(|k| !k.trim().is_empty()).unwrap_or_else(|| self.name.to_lowercase())
    }
}

impl Config {
    /// The engine's name and URL template; the first engine when the name is unknown.
    pub fn engine(&self) -> (&'static str, &'static str) {
        ENGINES.iter().copied().find(|(name, _)| *name == self.general.search_engine).unwrap_or(ENGINES[0])
    }

    /// The hotkey's accelerator, or `None` when it's off.
    pub fn hotkey(&self, hotkey: Hotkey) -> Option<String> {
        match self.hotkeys.get(&hotkey.id()) {
            Some(accel) => Some(accel.clone()).filter(|a| !a.is_empty()),
            None => hotkey.default_accel().map(Into::into),
        }
    }

    pub fn set_hotkey(&mut self, hotkey: Hotkey, accel: Option<String>) {
        self.hotkeys.insert(hotkey.id(), accel.unwrap_or_default());
    }

    /// The hotkey as keycaps for a row's hint: "⌃⌥V" on macOS, "Ctrl+Alt+V" elsewhere.
    pub fn hotkey_label(&self, hotkey: Hotkey) -> Option<String> {
        let shortcut: rune::Shortcut = self.hotkey(hotkey)?.parse().ok()?;
        Some(shortcut.keycaps().join(if cfg!(target_os = "macos") { "" } else { "+" }))
    }
}

/// `~/Library/Application Support/Runecast` on macOS, `$XDG_CONFIG_HOME/runecast` elsewhere.
pub fn dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Runecast")
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or(home.join(".config")).join("runecast")
    }
}

pub fn path() -> PathBuf {
    dir().join("config.json")
}

/// The config on disk; the example config, written out, when there's none; the example config
/// with an error on stderr when the file doesn't parse, so a typo never empties the launcher.
pub fn load() -> Config {
    let path = path();
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            eprintln!("runecast: {} doesn't parse: {e}", path.display());
            example()
        }),
        Err(_) => {
            let config = example();
            // Headless screenshots and scripts leave the disk alone.
            if !rune::is_headless() {
                save(&config);
            }
            config
        }
    }
}

pub fn save(config: &Config) {
    let _ = std::fs::create_dir_all(dir());
    if let Err(e) = std::fs::write(path(), serde_json::to_string_pretty(config).unwrap()) {
        eprintln!("runecast: couldn't save settings: {e}");
    }
}

pub fn example() -> Config {
    let snippet = |name: &str, text: &str| Snippet { name: name.into(), text: text.into() };
    let link = |name: &str, keyword: &str, url: &str| Quicklink { name: name.into(), url: url.into(), keyword: Some(keyword.into()) };
    let mut config = Config {
        snippets: vec![
            snippet("Email Signature", "Best,\nDino"),
            snippet("Shrug", "¯\\_(ツ)_/¯"),
            snippet("Meeting Link", "https://meet.google.com/"),
        ],
        quicklinks: vec![
            link("Search GitHub", "gh", "https://github.com/search?q={query}"),
            link("Search YouTube", "yt", "https://www.youtube.com/results?search_query={query}"),
            link("Search Maps", "map", "https://www.google.com/maps/search/{query}"),
            link("Translate", "tr", "https://translate.google.com/?sl=auto&tl=en&text={query}"),
        ],
        aliases: vec![
            Alias { alias: "vs".into(), target: "Visual Studio Code".into() },
            Alias { alias: "term".into(), target: "Terminal".into() },
        ],
        ..Config::default()
    };
    // Written out so the file shows every hotkey there is.
    for hotkey in Hotkey::all() {
        let accel = config.hotkey(hotkey);
        config.set_hotkey(hotkey, accel);
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wm::Layout;

    #[test]
    fn partial_files_parse_and_keywords_default_to_the_name() {
        let config: Config = serde_json::from_str(r#"{"quicklinks": [{"name": "Docs", "url": "https://docs.rs/{query}"}]}"#).unwrap();
        assert!(config.snippets.is_empty());
        assert_eq!(config.quicklinks[0].keyword(), "docs");
        assert!(config.general.clipboard_history);
        assert_eq!(config.engine().0, "Google");
    }

    #[test]
    fn missing_hotkeys_take_defaults_and_empty_ones_are_off() {
        let mut config: Config = serde_json::from_str(r#"{"hotkeys": {"clipboard": ""}}"#).unwrap();
        assert_eq!(config.hotkey(Hotkey::Launcher).as_deref(), Some("alt+space"));
        assert_eq!(config.hotkey(Hotkey::Clipboard), None);
        assert_eq!(config.hotkey(Hotkey::Window(Layout::TopHalf)), None, "no default");
        config.set_hotkey(Hotkey::Window(Layout::TopHalf), Some("ctrl+alt+up".into()));
        let back: Config = serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
        assert_eq!(back.hotkey(Hotkey::Window(Layout::TopHalf)).as_deref(), Some("ctrl+alt+up"));
        assert_eq!(back.hotkey(Hotkey::Clipboard), None, "turning one off survives a save");
    }
}
