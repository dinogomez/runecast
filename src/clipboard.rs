//! Clipboard history: a worker polls the clipboard's text and hands each new copy to the app,
//! which keeps the latest entries, newest first, and saves them beside the config.

use std::time::Duration;

use rune::AppHandle;

/// Longer copies (a whole log file, say) aren't kept.
const MAX_CHARS: usize = 100_000;

#[derive(Default)]
pub struct History {
    pub entries: Vec<String>,
}

impl History {
    pub fn load() -> Self {
        let entries = std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        History { entries }
    }

    /// Puts `text` first, keeping at most `max` entries. A copy already in the history moves to
    /// the top instead of repeating. Returns whether anything changed.
    pub fn push(&mut self, text: String, max: usize) -> bool {
        if text.trim().is_empty() || text.chars().count() > MAX_CHARS || self.entries.first() == Some(&text) {
            return false;
        }
        self.entries.retain(|e| *e != text);
        self.entries.insert(0, text);
        self.entries.truncate(max);
        true
    }

    /// Entries containing `query` (ignoring case), newest first.
    pub fn search(&self, query: &str) -> Vec<&String> {
        let q = query.trim().to_lowercase();
        self.entries.iter().filter(|e| q.is_empty() || e.to_lowercase().contains(&q)).collect()
    }

    /// Saved readable only by this user: the history can hold anything that was copied.
    pub fn save(&self) {
        let path = path();
        let _ = std::fs::create_dir_all(crate::config::dir());
        if std::fs::write(&path, serde_json::to_string(&self.entries).unwrap()).is_ok() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
        }
    }
}

fn path() -> std::path::PathBuf {
    crate::config::dir().join("clipboard.json")
}

/// Polls the clipboard twice a second on its own thread and calls `on_copy` on the UI thread
/// with each new text.
pub fn watch<S: 'static>(app: AppHandle<S>, on_copy: fn(&mut S, String)) {
    std::thread::Builder::new().name("runecast-clipboard".into()).spawn(move || {
        let mut board = match arboard::Clipboard::new() {
            Ok(board) => board,
            Err(e) => return eprintln!("runecast: clipboard history is off: {e}"),
        };
        let mut last = None;
        loop {
            if let Ok(text) = board.get_text() && last.as_ref() != Some(&text) {
                last = Some(text.clone());
                app.update(move |s, _| on_copy(s, text));
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }).expect("clipboard thread");
}

#[cfg(test)]
mod tests {
    use super::History;

    #[test]
    fn a_repeated_copy_moves_to_the_top_without_duplicating() {
        let mut h = History::default();
        for t in ["a", "b", "c"] {
            h.push(t.into(), 10);
        }
        assert!(h.push("a".into(), 10));
        assert_eq!(h.entries, ["a", "c", "b"]);
        assert!(!h.push("a".into(), 10), "copying the top entry again changes nothing");
        assert!(!h.push("  \n".into(), 10), "blank copies aren't kept");
    }

    #[test]
    fn history_is_capped_dropping_the_oldest() {
        let mut h = History::default();
        for i in 0..55 {
            h.push(i.to_string(), 50);
        }
        assert_eq!(h.entries.len(), 50);
        assert_eq!(h.entries.last().unwrap(), "5");
    }
}
