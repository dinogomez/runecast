//! The Settings window: general options, hotkeys, favorites, aliases, snippets and quicklinks. Every change saves
//! `config.json` and hands the new config to the launcher; a recorded hotkey is rebound at once.

use std::sync::{Arc, Mutex};

use rune::{AppHandle, Cx, Image, ImageCache, Latest, Waker, El, ScrollSpec, Shortcut, ShortcutError, TextEdit, TextStyle, Tx, col, el, row, spacer, text};
use rune_kit::{Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit, SegmentedControl, Select, ShortcutRecorder, ShortcutRecorderState, Switch, TextArea, TextInput};

use crate::State;
use crate::catalog::{self, Action, Item, Kind};
use crate::config::{self, Appearance, CLIPBOARD_SIZES, Config, ENGINES, Favorite, Quicklink, Snippet};
use crate::hotkeys::{Hotkey, Registry};
use crate::login;
use crate::update::{self, Release};

pub const SIZE: (f32, f32) = (760.0, 540.0);
/// The band the window drags by. On macOS the traffic lights sit in it, over the sidebar.
pub const TITLEBAR: f32 = 52.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    General,
    Hotkeys,
    Snippets,
    Quicklinks,
    Aliases,
    Favorites,
}

impl Tab {
    const ALL: [Tab; 6] = [Tab::General, Tab::Hotkeys, Tab::Favorites, Tab::Aliases, Tab::Snippets, Tab::Quicklinks];

    fn title(self) -> &'static str {
        match self {
            Tab::General => "General",
            Tab::Hotkeys => "Hotkeys",
            Tab::Snippets => "Snippets",
            Tab::Quicklinks => "Quicklinks",
            Tab::Aliases => "Aliases",
            Tab::Favorites => "Favorites",
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            Tab::General => "⚙",
            Tab::Hotkeys => "⌘",
            Tab::Snippets => "¶",
            Tab::Quicklinks => "↗",
            Tab::Aliases => "@",
            Tab::Favorites => "★",
        }
    }
}

pub struct Settings {
    pub tab: Tab,
    pub config: Config,
    recorders: Vec<(Hotkey, ShortcutRecorderState)>,
    open_select: Option<&'static str>,
    login: bool,
    cleared: bool,
    /// The selected snippet and quicklink, and the fields editing them.
    snippet: usize,
    link: usize,
    alias: usize,
    /// What the launcher can find, to warn about an alias or favorite for something that isn't there.
    /// Empty in headless scenes, where nothing is scanned.
    known: Vec<Item>,
    name: TextEdit,
    body: TextEdit,
    keyword: TextEdit,
    url: TextEdit,
    registry: Option<Arc<Mutex<Registry>>>,
    /// Made in `setup`, which has the waker that redraws once the logo decodes.
    logo: Option<(ImageCache, Image)>,
    /// None in headless scenes, which then never touch the disk.
    launcher: Option<AppHandle<State>>,
    /// Check for Updates: the check in flight, made in `setup`, and what the last one found.
    update_check: Option<Latest<Result<Option<Release>, String>>>,
    update: Option<Result<Option<Release>, String>>,
}

impl Settings {
    pub fn new(config: Config, registry: Option<Arc<Mutex<Registry>>>, launcher: Option<AppHandle<State>>) -> Self {
        let mut s = Settings {
            tab: Tab::General, config, recorders: Vec::new(), open_select: None, login: false, cleared: false,
            snippet: 0, link: 0, alias: 0, known: Vec::new(), name: TextEdit::new(""), body: TextEdit::new(""), keyword: TextEdit::new(""),
            url: TextEdit::new(""), registry, logo: None, launcher, update_check: None, update: None,
        };
        s.refresh();
        s
    }

    /// Reads the config again, for when the window opens after hand edits.
    pub fn reload(&mut self) {
        self.config = config::load();
        self.cleared = false;
        self.refresh();
    }

    fn refresh(&mut self) {
        let failed = self.registry.as_ref().map(|r| r.lock().unwrap().failed.clone()).unwrap_or_default();
        self.recorders = Hotkey::all().into_iter().map(|h| {
            let mut state = ShortcutRecorderState::new(self.config.hotkey(h).and_then(|a| a.parse().ok()));
            if failed.contains(&h) {
                state.mark_unregistered();
            }
            (h, state)
        }).collect();
        self.login = self.launcher.is_some() && login::enabled();
        self.snippet = self.snippet.min(self.config.snippets.len().saturating_sub(1));
        self.link = self.link.min(self.config.quicklinks.len().saturating_sub(1));
        self.alias = self.alias.min(self.config.aliases.len().saturating_sub(1));
        if self.launcher.is_some() {
            self.known = catalog::load(&self.config);
        }
        self.load_fields();
    }

    /// Check for Updates, or once a newer release turned up, install it.
    fn update_pressed(&mut self) {
        let Some(launcher) = &self.launcher else { return };
        if let Some(Ok(Some(_))) = self.update {
            launcher.update(|s, _| s.install_update());
        } else if let Some(check) = &self.update_check {
            check.request((), |()| update::latest());
        }
    }

    /// Saves and tells the launcher.
    fn changed(&mut self) {
        // Snippets and quicklinks may have been added or renamed; apps are scanned once, on open.
        if !self.known.is_empty() {
            self.known.retain(|it| !matches!(it.kind, Kind::Snippet | Kind::Quicklink));
            self.known.extend(catalog::extras(&self.config));
        }
        let Some(launcher) = &self.launcher else { return };
        config::save(&self.config);
        let config = self.config.clone();
        launcher.update(move |s, _| s.apply_config(config));
    }

    fn select_tab(&mut self, tab: Tab) {
        self.tab = tab;
        self.open_select = None;
        self.load_fields();
    }

    fn toggle_select(&mut self, key: &'static str) {
        self.open_select = if self.open_select == Some(key) { None } else { Some(key) };
    }

    fn rebind(&mut self, hotkey: Hotkey, next: Option<&Shortcut>) -> Result<(), ShortcutError> {
        if let Some(registry) = &self.registry {
            registry.lock().unwrap().set(hotkey, next)?;
        }
        self.config.set_hotkey(hotkey, next.map(ToString::to_string));
        self.changed();
        Ok(())
    }

    fn toggle_login(&mut self) {
        if self.launcher.is_none() {
            self.login = !self.login;
            return;
        }
        match login::set(!self.login) {
            Ok(()) => self.login = !self.login,
            Err(e) => eprintln!("runecast: couldn't change launch at login: {e}"),
        }
    }

    fn clear_history(&mut self) {
        if let Some(launcher) = &self.launcher {
            launcher.update(|s, _| {
                s.history.entries.clear();
                s.history.save();
            });
        }
        self.cleared = true;
    }

    /// Puts the selected snippet or quicklink in the fields.
    fn load_fields(&mut self) {
        let snippet = self.config.snippets.get(self.snippet).cloned().unwrap_or_default();
        let link = self.config.quicklinks.get(self.link).cloned().unwrap_or_default();
        let alias = self.config.aliases.get(self.alias).cloned().unwrap_or_default();
        // Aliases edit their alias in `name` and their target in `keyword`.
        let (name, body, keyword) = match self.tab {
            Tab::Quicklinks => (link.name, String::new(), link.keyword.unwrap_or_default()),
            Tab::Aliases => (alias.alias, String::new(), alias.target),
            // Favorites take the name of one to add in `name`.
            Tab::Favorites => Default::default(),
            _ => (snippet.name, snippet.text, String::new()),
        };
        self.name.replace_text(name);
        self.body.replace_text(body);
        self.keyword.replace_text(keyword);
        self.url.replace_text(link.url);
    }

    /// Copies the fields back into the selected snippet or quicklink.
    fn store_fields(&mut self) {
        match self.tab {
            Tab::Snippets => if let Some(sn) = self.config.snippets.get_mut(self.snippet) {
                sn.name = self.name.text.clone();
                sn.text = self.body.text.clone();
            },
            Tab::Quicklinks => if let Some(q) = self.config.quicklinks.get_mut(self.link) {
                q.name = self.name.text.clone();
                q.url = self.url.text.trim().to_string();
                q.keyword = Some(self.keyword.text.trim().to_string()).filter(|k| !k.is_empty());
            },
            Tab::Aliases => if let Some(a) = self.config.aliases.get_mut(self.alias) {
                a.alias = self.name.text.trim().to_string();
                a.target = self.keyword.text.trim().to_string();
            },
            _ => return,
        }
        self.changed();
    }

    fn pick(&mut self, i: usize) {
        match self.tab {
            Tab::Quicklinks => self.link = i,
            Tab::Aliases => self.alias = i,
            _ => self.snippet = i,
        }
        self.load_fields();
    }

    fn add(&mut self) {
        match self.tab {
            Tab::Quicklinks => {
                self.config.quicklinks.push(Quicklink { name: "New Quicklink".into(), url: "https://".into(), keyword: None });
                self.link = self.config.quicklinks.len() - 1;
            }
            Tab::Aliases => {
                self.config.aliases.push(crate::config::Alias::default());
                self.alias = self.config.aliases.len() - 1;
            }
            _ => {
                self.config.snippets.push(Snippet { name: "New Snippet".into(), text: String::new() });
                self.snippet = self.config.snippets.len() - 1;
            }
        }
        self.load_fields();
        self.changed();
    }

    fn delete(&mut self) {
        match self.tab {
            Tab::Quicklinks if self.link < self.config.quicklinks.len() => {
                self.config.quicklinks.remove(self.link);
            }
            Tab::Aliases if self.alias < self.config.aliases.len() => {
                self.config.aliases.remove(self.alias);
            }
            Tab::Snippets if self.snippet < self.config.snippets.len() => {
                self.config.snippets.remove(self.snippet);
            }
            _ => return,
        }
        self.refresh();
        self.changed();
    }

    /// Swaps favorite `i` with the one above it, or below it.
    fn move_favorite(&mut self, i: usize, up: bool) {
        let j = if up { i.checked_sub(1) } else { Some(i + 1) };
        let Some(j) = j.filter(|j| *j < self.config.favorites.len()) else { return };
        self.config.favorites.swap(i, j);
        self.changed();
    }

    /// What the Add field would add: the first item the launcher has by the typed title that
    /// isn't a favorite yet, or `None` when there's none.
    fn new_favorite(&self) -> Option<Favorite> {
        let typed = self.name.text.trim();
        if typed.is_empty() {
            return None;
        }
        // Headless scenes scan nothing, so anything typed counts there, as an app.
        if self.known.is_empty() {
            let app = Favorite { title: typed.into(), kind: Kind::App.label().into() };
            return (!self.config.favorites.contains(&app)).then_some(app);
        }
        self.known.iter().find(|it| it.title.eq_ignore_ascii_case(typed) && !self.config.is_favorite(it)).map(Favorite::of)
    }

    fn add_favorite(&mut self) {
        let Some(title) = self.new_favorite() else { return };
        self.config.favorites.push(title);
        self.name.replace_text("");
        self.changed();
    }

    fn remove_favorite(&mut self, i: usize) {
        if i < self.config.favorites.len() {
            self.config.favorites.remove(i);
            self.changed();
        }
    }
}

pub fn view(s: &Settings, cx: &mut Cx<Settings>) -> El {
    let dark = s.config.general.appearance.dark(cx.system_dark());
    let k = Kit::neutral(dark);
    let mut layer = Vec::new();
    // What the window controls take: the traffic lights on macOS, nothing where the native
    // titlebar stays.
    let chrome = cx.chrome_insets();

    let mut sidebar = col().key("tabs").w(200.0).h_full().px(12.0).pb(12.0).gap(2.0).fill(k.c.bg_base)
        .child(el().key("sidebar-drag").w_full().h(chrome.height.max(12.0)).window_drag())
        .child(row().h(40.0).px(8.0).gap(10.0).center_y()
            .child(crate::mark(s.logo.as_ref().map(|(_, img)| img), 18.0, k.c.bg_accent))
            .child(text("Runecast").font(k.ty.ui_medium).color(k.c.text_base)));
    for tab in Tab::ALL {
        let on = tab == s.tab;
        sidebar = sidebar.child(row().key(format!("tab-{}", tab.title().to_lowercase())).h(32.0).px(10.0).gap(10.0)
            .radius(k.radius.md).center_y().fill(if on { k.c.layer_02 } else { rune::Color::TRANSPARENT })
            .role("tab").label(tab.title()).checked(on)
            .child(text(tab.glyph()).font(k.ty.ui).color(if on { k.c.text_base } else { k.c.text_muted }).w(16.0))
            .child(text(tab.title()).font(k.ty.ui_medium).color(if on { k.c.text_base } else { k.c.text_muted }))
            .on_press(move |s: &mut Settings| s.select_tab(tab)));
    }

    let body = match s.tab {
        Tab::General => scrolling(general(s, &k, &mut layer)),
        Tab::Hotkeys => scrolling(hotkeys(s, &k)),
        Tab::Favorites => scrolling(favorites(s, &k)),
        Tab::Snippets | Tab::Quicklinks | Tab::Aliases => editor(s, &k),
    };
    let content = col().grow(1.0).min_w(0.0).h_full().fill(k.c.page)
        .child(row().key("header").w_full().h(56.0f32.max(chrome.height)).px(28.0).center_y().window_drag()
            .child(text(s.tab.title()).font(TextStyle::new(18.0, 600.0)).color(k.c.text_base)))
        .child(el().h(1.0).w_full().fill(k.c.border_muted))
        .child(body);
    rune_kit::root(row().w_full().h_full().fill(k.c.page).child(sidebar).child(el().w(1.0).h_full().fill(k.c.border_muted)).child(content), layer)
}

fn scrolling(inner: El) -> El {
    col().key("content").w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false })
        .child(inner.w_full().px(28.0).py(12.0))
}

/// One setting: its title and a line about it on the left, its control on the right.
fn setting(k: &Kit, title: &str, detail: &str, control: El) -> El {
    row().w_full().min_h(60.0).py(10.0).gap(20.0).center_y()
        .child(col().grow(1.0).min_w(0.0).gap(3.0)
            .child(text(title.to_string()).font(k.ty.ui_medium).color(k.c.text_base))
            .child(text(detail.to_string()).font(k.ty.small).color(k.c.text_muted).wrap().w_full()))
        .child(control)
}

fn section(k: &Kit, title: &str) -> El {
    row().w_full().pt(14.0).pb(4.0).child(text(title.to_uppercase()).font(k.ty.text_xsmall_medium).color(k.c.text_faint))
}

fn divider(k: &Kit) -> El {
    el().h(1.0).w_full().fill(k.c.border_muted)
}

fn general(s: &Settings, k: &Kit, layer: &mut Vec<El>) -> El {
    let g = &s.config.general;
    let appearance = SegmentedControl::new(&["System", "Light", "Dark"], Appearance::ALL.iter().position(|a| *a == g.appearance))
        .label("Appearance").width(Some(240.0))
        .on_pick(|s: &mut Settings, i, _| {
            s.config.general.appearance = Appearance::ALL[i];
            s.changed();
        })
        .build(k);
    let engines: Vec<&str> = ENGINES.iter().map(|(n, _)| *n).collect();
    let engine = Select::new("engine", &engines).label("Search engine").width(180.0)
        .current(Some(s.config.engine().0)).open(s.open_select == Some("engine"))
        .on_toggle(|s: &mut Settings| s.toggle_select("engine"))
        .on_close(|s: &mut Settings| s.open_select = None)
        .on_choose(|s: &mut Settings, v: &str, _: &mut Tx| {
            s.config.general.search_engine = v.into();
            s.open_select = None;
            s.changed();
        })
        .build(k, layer);
    let login = Switch::new(s.login).label("Launch at login").hide_label().on_toggle(|s: &mut Settings| s.toggle_login()).build(k);
    let history = Switch::new(g.clipboard_history).label("Clipboard history").hide_label()
        .on_toggle(|s: &mut Settings| {
            s.config.general.clipboard_history = !s.config.general.clipboard_history;
            s.changed();
        })
        .build(k);
    let sizes: Vec<String> = CLIPBOARD_SIZES.iter().map(|n| format!("{n} items")).collect();
    let current = format!("{} items", g.clipboard_size);
    let size = Select::new("history-size", &sizes).label("History size").width(180.0)
        .current(Some(&current)).open(s.open_select == Some("history-size")).disabled(!g.clipboard_history)
        .on_toggle(|s: &mut Settings| s.toggle_select("history-size"))
        .on_close(|s: &mut Settings| s.open_select = None)
        .on_choose(|s: &mut Settings, v: &str, _: &mut Tx| {
            if let Some(n) = v.split(' ').next().and_then(|n| n.parse().ok()) {
                s.config.general.clipboard_size = n;
            }
            s.open_select = None;
            s.changed();
        })
        .build(k, layer);
    let clear = Button::new(if s.cleared { "Cleared" } else { "Clear History" }).variant(ButtonVariant::Outline)
        .disabled(s.cleared).on_press(|s: &mut Settings| s.clear_history()).build(k);
    let file = Button::new("Open config.json").variant(ButtonVariant::Outline)
        .on_press(|_: &mut Settings| catalog::spawn(&Action::OpenPath(config::path()))).build(k);
    let checking = s.update_check.as_ref().is_some_and(Latest::is_pending);
    let (label, detail) = match &s.update {
        _ if checking => ("Checking…".to_string(), "Asking GitHub for the latest release.".to_string()),
        Some(Ok(Some(r))) => (format!("Update to v{}", r.version), format!("v{} is out. Runecast relaunches once it's installed.", r.version)),
        Some(Ok(None)) => ("Check for Updates".into(), "You're on the latest version.".into()),
        Some(Err(e)) => ("Check for Updates".into(), format!("Couldn't check: {e}.")),
        None => ("Check for Updates".into(), "Checked daily; new versions show up in the menu bar.".into()),
    };
    let available = matches!(s.update, Some(Ok(Some(_))));
    let check = Button::new(&label).variant(if available { ButtonVariant::Submit } else { ButtonVariant::Outline })
        .disabled(checking).on_press(|s: &mut Settings| s.update_pressed()).build(k);
    let version = row().gap(14.0).center_y()
        .child(text(concat!("v", env!("CARGO_PKG_VERSION"))).font(k.ty.ui).color(k.c.text_muted))
        .child(check);
    let engine_detail = format!("The last row searches {} for what you typed.", s.config.engine().0);
    col()
        .child(section(k, "Look and feel"))
        .child(setting(k, "Appearance", "Follow the system, or always light or dark.", appearance))
        .child(divider(k))
        .child(setting(k, "Search engine", &engine_detail, engine))
        .child(divider(k))
        .child(setting(k, "Launch at login", "Start Runecast in the background when you log in.", login))
        .child(section(k, "Clipboard history"))
        .child(setting(k, "Keep clipboard history", "Remember the texts you copy. Includes anything secret you copy.", history))
        .child(divider(k))
        .child(setting(k, "History size", "Older entries are dropped past this.", size))
        .child(divider(k))
        .child(setting(k, "Clear history", "Forget everything copied so far.", clear))
        .child(section(k, "Advanced"))
        .child(setting(k, "Config file", "Every setting, as JSON, for editing by hand.", file))
        .child(section(k, "About"))
        .child(setting(k, "Version", &detail, version))
}

fn hotkeys(s: &Settings, k: &Kit) -> El {
    let mut out = col().child(section(k, "Runecast"));
    for (i, (hotkey, state)) in s.recorders.iter().enumerate() {
        let hotkey = *hotkey;
        if hotkey == Hotkey::Window(crate::wm::Layout::ALL[0]) {
            out = out.child(section(k, "Window management"));
        } else if i > 0 {
            out = out.child(divider(k));
        }
        let detail = match hotkey {
            Hotkey::Launcher => "Show or hide the launcher from anywhere.",
            Hotkey::Clipboard => "Open the launcher on clipboard history.",
            Hotkey::Window(_) if hotkey.available() => "Move the front window.",
            Hotkey::Window(_) => "Window management works on macOS only.",
        };
        let recorder = ShortcutRecorder::new(format!("hotkey-{}", hotkey.id()), state, move |s: &mut Settings| &mut s.recorders[i].1)
            .label(hotkey.title()).width(200.0).disabled(!hotkey.available())
            .on_change(move |s: &mut Settings, next, _| s.rebind(hotkey, next))
            .build(k);
        out = out.child(setting(k, hotkey.title(), detail, recorder));
    }
    out.child(row().w_full().pt(16.0)
        .child(text("Click a shortcut to record a new one. Backspace clears it; Escape cancels.").font(k.ty.small).color(k.c.text_faint)))
}

/// The favorites in the order the launcher lists them, each with buttons to move or remove it.
fn favorites(s: &Settings, k: &Kit) -> El {
    let keys = if cfg!(target_os = "macos") { ("⌘⇧F", "⌘⌥↑/↓") } else { ("Ctrl+Shift+F", "Ctrl+Alt+↑/↓") };
    let how = format!("Select an app or command in the launcher and press {} to add it here, or {} to move it.", keys.0, keys.1);
    let list = &s.config.favorites;
    let typed = s.name.text.trim();
    let taken = s.config.favorites.iter().any(|f| f.title.eq_ignore_ascii_case(typed));
    let (hint, color) = match s.new_favorite() {
        None if taken => (format!("“{typed}” is already a favorite."), k.c.text_faint),
        None if !typed.is_empty() => (format!("Nothing in the launcher is named “{typed}”. Use the name as it shows in search."), k.c.danger.fg),
        _ => ("Type the name of an app or command as it shows in search.".to_string(), k.c.text_faint),
    };
    let input = TextInput::new(&s.name).width(None).placeholder("Visual Studio Code").label("Add favorite")
        .on_edit(name_edit)
        .on_key(|s: &mut Settings, ev, _| {
            let enter = ev.key == rune::Key::Enter;
            if enter {
                s.add_favorite();
            }
            enter
        })
        .build(k);
    let add = Button::new("Add").variant(ButtonVariant::Outline).disabled(s.new_favorite().is_none())
        .on_press(|s: &mut Settings| s.add_favorite()).build(k);
    let adder = col().w_full().gap(6.0).pb(8.0)
        .child(row().w_full().gap(8.0).center_y().child(el().key("favorite-name").grow(1.0).min_w(0.0).child(input)).child(add))
        .child(text(hint).font(k.ty.small).color(color).wrap().w_full());
    let mut out = col().child(section(k, "Add favorite")).child(adder).child(section(k, "Listed first when the search is empty"));
    if list.is_empty() {
        return out.child(setting(k, "No favorites yet", &how, el()));
    }
    let button = |glyph: &'static rune::Icon, label: String, disabled: bool| IconButton::new(glyph)
        .variant(IconButtonVariant::GhostMuted).control_size(ControlSize::Small).label(label).disabled(disabled);
    for (i, favorite) in list.iter().enumerate() {
        if i > 0 {
            out = out.child(divider(k));
        }
        let title = &favorite.title;
        let missing = !s.known.is_empty() && !s.known.iter().any(|it| favorite.is(it));
        let mut name = row().grow(1.0).min_w(0.0).gap(10.0).center_y()
            .child(text(format!("{}", i + 1)).font(k.ty.small).color(k.c.text_faint).w(16.0))
            .child(text(title.clone()).font(k.ty.ui_medium).color(k.c.text_base).truncate())
            .child(text(favorite.kind.clone()).font(k.ty.small).color(k.c.text_faint));
        if missing {
            name = name.child(text("Not found in the launcher").font(k.ty.small).color(k.c.danger.fg));
        }
        out = out.child(row().key(format!("favorite-{i}")).w_full().h(48.0).gap(4.0).center_y()
            .child(name)
            .child(button(&rune_kit::glyphs::CHEVRON_UP, format!("Move {title} up"), i == 0)
                .on_press(move |s: &mut Settings| s.move_favorite(i, true)).build(k))
            .child(button(&rune_kit::glyphs::CHEVRON_DOWN, format!("Move {title} down"), i + 1 == list.len())
                .on_press(move |s: &mut Settings| s.move_favorite(i, false)).build(k))
            .child(button(&rune_kit::glyphs::CLOSE, format!("Remove {title}"), false)
                .on_press(move |s: &mut Settings| s.remove_favorite(i)).build(k)));
    }
    out.child(row().w_full().pt(16.0).child(text(how).font(k.ty.small).color(k.c.text_faint).wrap().w_full()))
}

/// Snippets, quicklinks and aliases: a list on the left, the selected one's fields on the right.
fn editor(s: &Settings, k: &Kit) -> El {
    let links = s.tab == Tab::Quicklinks;
    let aliases = s.tab == Tab::Aliases;
    let (names, selected): (Vec<String>, usize) = match s.tab {
        Tab::Quicklinks => (s.config.quicklinks.iter().map(|q| q.name.clone()).collect(), s.link),
        Tab::Aliases => (s.config.aliases.iter().map(|a| match (a.alias.is_empty(), a.target.is_empty()) {
            (true, _) => String::new(),
            (false, true) => a.alias.clone(),
            (false, false) => format!("{} → {}", a.alias, a.target),
        }).collect(), s.alias),
        _ => (s.config.snippets.iter().map(|q| q.name.clone()).collect(), s.snippet),
    };
    let mut list = col().key("list").w_full().grow(1.0).min_h(0.0).gap(2.0).scroll(ScrollSpec { stick_bottom: false });
    for (i, name) in names.iter().enumerate() {
        let on = i == selected;
        let name = if name.trim().is_empty() { "Untitled" } else { name.as_str() };
        list = list.child(row().key(format!("item-{i}")).h(32.0).px(10.0).radius(k.radius.md).center_y()
            .fill(if on { k.c.layer_02 } else { rune::Color::TRANSPARENT })
            .child(text(name.to_string()).font(k.ty.ui).color(if on { k.c.text_base } else { k.c.text_muted }).truncate())
            .on_press(move |s: &mut Settings| s.pick(i)));
    }
    let add = Button::new(match s.tab { Tab::Quicklinks => "Add Quicklink", Tab::Aliases => "Add Alias", _ => "Add Snippet" }).variant(ButtonVariant::Outline)
        .on_press(|s: &mut Settings| s.add()).build(k);
    let left = col().w(220.0).h_full().padding(12.0).gap(8.0).child(list).child(add);

    let label = |t: &'static str| text(t).font(k.ty.small_medium).color(k.c.text_muted);
    let field = |key: &'static str, title: &str, edit: &TextEdit, placeholder: &str, access: fn(&mut Settings) -> &mut TextEdit| {
        el().key(key).w_full().child(TextInput::new(edit).width(None).placeholder(placeholder).label(title)
            .on_edit(access).on_change(|s: &mut Settings, _| s.store_fields()).build(k))
    };
    let right = if names.is_empty() {
        col().grow(1.0).h_full().center().gap(6.0)
            .child(text(match s.tab { Tab::Quicklinks => "No quicklinks yet", Tab::Aliases => "No aliases yet", _ => "No snippets yet" })
                .font(k.ty.ui_medium).color(k.c.text_base))
            .child(text("Add one to search it from the launcher.").font(k.ty.small).color(k.c.text_muted))
    } else {
        let mut form = col().grow(1.0).min_w(0.0).h_full().padding(20.0).gap(8.0);
        if aliases {
            let target = s.keyword.text.trim();
            let missing = !s.known.is_empty() && !target.is_empty() && !s.known.iter().any(|it| it.title.eq_ignore_ascii_case(target));
            let (hint, color) = if missing {
                (format!("Nothing in the launcher is named “{target}”. Use the name as it shows in search."), k.c.danger.fg)
            } else {
                ("Type the alias in the launcher to put this app or command first.".to_string(), k.c.text_faint)
            };
            form = form
                .child(label("Alias")).child(field("name", "Alias", &s.name, "vs", name_edit))
                .child(label("Opens")).child(field("keyword", "Opens", &s.keyword, "Visual Studio Code", keyword_edit))
                .child(text(hint).font(k.ty.small).color(color).wrap().w_full());
        } else {
            form = form.child(label("Name")).child(field("name", "Name", &s.name, "Name", name_edit));
        }
        if links {
            form = form
                .child(label("Keyword")).child(field("keyword", "Keyword", &s.keyword, &s.name.text.to_lowercase(), keyword_edit))
                .child(label("URL")).child(field("url", "URL", &s.url, "https://example.com/search?q={query}", url_edit))
                .child(text("Put {query} in the URL to take what's typed after the keyword: “gh rune” searches GitHub for “rune”.")
                    .font(k.ty.small).color(k.c.text_faint).wrap().w_full());
        } else if !aliases {
            form = form.child(label("Text")).child(el().key("text").w_full()
                .child(TextArea::new(&s.body).width(None).rows(9).placeholder("What to paste").label("Text")
                    .on_edit(|s: &mut Settings| &mut s.body).on_change(|s: &mut Settings, _| s.store_fields()).build(k)))
                .child(text("↩ on this snippet in the launcher pastes this text into the app you were in.").font(k.ty.small).color(k.c.text_faint).wrap().w_full());
        }
        form.child(spacer()).child(row().w_full().child(spacer())
            .child(Button::new("Delete").variant(ButtonVariant::Danger).on_press(|s: &mut Settings| s.delete()).build(k)))
    };
    row().w_full().grow(1.0).min_h(0.0)
        .child(left)
        .child(el().w(1.0).h_full().fill(k.c.border_muted))
        .child(right)
}

fn name_edit(s: &mut Settings) -> &mut TextEdit {
    &mut s.name
}

fn keyword_edit(s: &mut Settings) -> &mut TextEdit {
    &mut s.keyword
}

fn url_edit(s: &mut Settings) -> &mut TextEdit {
    &mut s.url
}

pub fn setup(s: &mut Settings, waker: Waker) {
    let cache = ImageCache::new(waker.clone());
    let logo = cache.load(crate::LOGO);
    if rune::is_headless() {
        let _ = logo.wait();
    }
    s.logo = Some((cache, logo));
    s.update_check = Some(Latest::new(waker));
}

/// A Check for Updates landed: show it, and offer a newer release in the menu bar too.
pub fn poll(s: &mut Settings, _: f64, _: &mut Tx) -> bool {
    let Some(found) = s.update_check.as_ref().and_then(Latest::take) else { return false };
    if let (Ok(Some(release)), Some(launcher)) = (&found, &s.launcher) {
        let release = release.clone();
        launcher.update(move |s, _| s.offer_update(release));
    }
    s.update = Some(found);
    true
}

pub const SCENES: &[(&str, fn(&mut Settings))] = &[
    ("general", |s| scene(s, Tab::General)),
    ("hotkeys", |s| scene(s, Tab::Hotkeys)),
    ("snippets", |s| scene(s, Tab::Snippets)),
    ("quicklinks", |s| scene(s, Tab::Quicklinks)),
    ("aliases", |s| scene(s, Tab::Aliases)),
    ("favorites", |s| {
        scene(s, Tab::Favorites);
        let fav = |title: &str, kind: Kind| Favorite { title: title.into(), kind: kind.label().into() };
        s.config.favorites = vec![fav("Visual Studio Code", Kind::App), fav("Slack", Kind::App), fav("Clipboard History", Kind::Command)];
    }),
    ("favorites-add", |s| {
        scene(s, Tab::Favorites);
        let app = |title: &str| Item::new(title, "", Kind::App, Action::Shell(String::new()));
        s.config.favorites = vec![Favorite::of(&app("Slack"))];
        s.known = vec![app("Slack"), app("Visual Studio Code")];
        s.name.replace_text("Visal Studio");
    }),
];

fn scene(s: &mut Settings, tab: Tab) {
    *s = Settings::new(config::example(), None, None);
    s.select_tab(tab);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_favorite_takes_the_launchers_spelling_once() {
        let mut s = Settings::new(config::example(), None, None);
        let app = |title: &str| Item::new(title, "", Kind::App, Action::Shell(String::new()));
        s.known = vec![app("Visual Studio Code"), app("Slack")];
        s.name.replace_text("  visual studio code ");
        s.add_favorite();
        let vsc = Favorite::of(&app("Visual Studio Code"));
        assert_eq!(s.config.favorites, std::slice::from_ref(&vsc));
        assert_eq!(s.name.text, "", "the field clears for the next one");
        // Again in other case, or a name the launcher doesn't have: nothing changes.
        for typed in ["VISUAL STUDIO CODE", "Visal Studio", "   "] {
            s.name.replace_text(typed);
            s.add_favorite();
            assert_eq!(s.config.favorites, std::slice::from_ref(&vsc), "{typed:?}");
        }
    }

    #[test]
    fn a_snippet_added_in_settings_can_be_a_favorite_right_away() {
        let mut s = Settings::new(config::example(), None, None);
        s.known = vec![Item::new("Slack", "", Kind::App, Action::Shell(String::new()))];
        s.select_tab(Tab::Snippets);
        s.add();
        s.name.replace_text("Slack");
        s.store_fields();
        s.select_tab(Tab::Favorites);
        // The app comes first; once it's a favorite, the snippet by the same name can be added too.
        for kind in [Kind::App, Kind::Snippet] {
            s.name.replace_text("slack");
            s.add_favorite();
            assert_eq!(s.config.favorites.last().map(|f| f.kind.as_str()), Some(kind.label()));
        }
    }
}
