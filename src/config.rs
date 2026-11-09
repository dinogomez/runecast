//! The user's snippets and quicklinks, read from `config.json` in Runecast's folder each time the
//! panel opens, so edits apply without a restart. A first run writes an example file.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub snippets: Vec<Snippet>,
    pub quicklinks: Vec<Quicklink>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Snippet {
    pub name: String,
    pub text: String,
}

/// A URL opened by name. `{query}` in the URL takes what's typed after its keyword:
/// "gh rune" opens GitHub's search for "rune".
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Quicklink {
    pub name: String,
    pub url: String,
    /// What to type before the argument; the name, lowercased, when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyword: Option<String>,
}

impl Quicklink {
    pub fn keyword(&self) -> String {
        self.keyword.clone().unwrap_or_else(|| self.name.to_lowercase())
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
            let _ = std::fs::create_dir_all(dir());
            let _ = std::fs::write(&path, serde_json::to_string_pretty(&config).unwrap());
            config
        }
    }
}

pub fn example() -> Config {
    let snippet = |name: &str, text: &str| Snippet { name: name.into(), text: text.into() };
    let link = |name: &str, keyword: &str, url: &str| Quicklink { name: name.into(), url: url.into(), keyword: Some(keyword.into()) };
    Config {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_files_parse_and_keywords_default_to_the_name() {
        let config: Config = serde_json::from_str(r#"{"quicklinks": [{"name": "Docs", "url": "https://docs.rs/{query}"}]}"#).unwrap();
        assert!(config.snippets.is_empty());
        assert_eq!(config.quicklinks[0].keyword(), "docs");
    }
}
