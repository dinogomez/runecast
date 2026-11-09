//! Global hotkeys: what each one does, and the registry that binds them to the launcher at
//! startup and rebinds them when the Settings window records a new chord.

use std::collections::{HashMap, HashSet};

use rune::{AppHandle, Shortcut, ShortcutError, ShortcutId};

use crate::State;
use crate::config::Config;
use crate::wm::Layout;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Hotkey {
    Launcher,
    Clipboard,
    Window(Layout),
}

impl Hotkey {
    pub fn all() -> Vec<Hotkey> {
        [Hotkey::Launcher, Hotkey::Clipboard].into_iter().chain(Layout::ALL.map(Hotkey::Window)).collect()
    }

    /// The key in `config.json`.
    pub fn id(self) -> String {
        match self {
            Hotkey::Launcher => "launcher".into(),
            Hotkey::Clipboard => "clipboard".into(),
            Hotkey::Window(l) => format!("window.{}", l.title().to_lowercase().replace(' ', "-")),
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Hotkey::Launcher => "Open Runecast",
            Hotkey::Clipboard => "Clipboard History",
            Hotkey::Window(l) => l.title(),
        }
    }

    pub fn default_accel(self) -> Option<&'static str> {
        match self {
            Hotkey::Launcher => Some("alt+space"),
            Hotkey::Clipboard => Some("ctrl+alt+v"),
            Hotkey::Window(Layout::LeftHalf) => Some("ctrl+alt+left"),
            Hotkey::Window(Layout::RightHalf) => Some("ctrl+alt+right"),
            Hotkey::Window(Layout::Maximize) => Some("ctrl+alt+enter"),
            Hotkey::Window(Layout::Center) => Some("ctrl+alt+c"),
            Hotkey::Window(_) => None,
        }
    }

    /// Window layouts only move windows on macOS, so they're bound only there.
    pub fn available(self) -> bool {
        !matches!(self, Hotkey::Window(_)) || cfg!(target_os = "macos")
    }
}

/// The launcher's live registrations. Shared by the launcher and Settings windows; both run on
/// the UI thread, where registering is immediate.
pub struct Registry {
    launcher: AppHandle<State>,
    ids: HashMap<Hotkey, ShortcutId>,
    /// Hotkeys whose chord couldn't be bound at startup (another app holds it, say).
    pub failed: HashSet<Hotkey>,
}

impl Registry {
    pub fn new(launcher: AppHandle<State>) -> Self {
        Registry { launcher, ids: HashMap::new(), failed: HashSet::new() }
    }

    /// Binds every available hotkey the config sets, reporting the ones that fail.
    pub fn register_all(&mut self, config: &Config) {
        for hotkey in Hotkey::all().into_iter().filter(|h| h.available()) {
            let Some(accel) = config.hotkey(hotkey) else { continue };
            let result = accel.parse::<Shortcut>().and_then(|s| self.set(hotkey, Some(&s)));
            if let Err(e) = result {
                eprintln!("runecast: {} ({accel}) isn't available: {e}", hotkey.title());
                self.failed.insert(hotkey);
            }
        }
    }

    /// Binds `hotkey` to `next` (or unbinds it). The new chord is registered before the old one
    /// is released, so a refused chord leaves the old binding working.
    pub fn set(&mut self, hotkey: Hotkey, next: Option<&Shortcut>) -> Result<(), ShortcutError> {
        let launcher = &self.launcher;
        let new = next.map(|s| launcher.register_shortcut(s.clone(), move |st: &mut State, _| st.fire(hotkey))).transpose()?;
        if let Some(old) = self.ids.remove(&hotkey)
            && let Err(e) = launcher.unregister_shortcut(old)
        {
            if let Some(new) = new {
                let _ = launcher.unregister_shortcut(new);
            }
            self.ids.insert(hotkey, old);
            return Err(e);
        }
        if let Some(new) = new {
            self.ids.insert(hotkey, new);
        }
        self.failed.remove(&hotkey);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Hotkey;

    #[test]
    fn ids_are_unique_and_defaults_parse() {
        let all = Hotkey::all();
        let ids: std::collections::HashSet<String> = all.iter().map(|h| h.id()).collect();
        assert_eq!(ids.len(), all.len());
        assert!(ids.contains("window.left-two-thirds"));
        for h in all {
            if let Some(accel) = h.default_accel() {
                assert!(accel.parse::<rune::Shortcut>().is_ok(), "{accel}");
            }
        }
    }
}
