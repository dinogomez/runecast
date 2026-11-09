//! Runecast: a Raycast-style launcher built with Rune.
//!
//! `cargo run --release` starts it in the background; ⌥Space (Alt+Space elsewhere) shows the panel.
//! Type to search apps, commands, snippets and quicklinks, do arithmetic inline, or fall through to
//! a web search. ⌃⌥V opens clipboard history; ⌃⌥←/→/↩/C move the front window on macOS. ⌘, in the
//! panel opens Settings, where every hotkey can be recorded again.
//!
//! Headless, with no window: `runecast --snap out.png --scene apps` renders a scene to PNG, and
//! `runecast --window settings --snap out.png --scene hotkeys` renders the Settings window.

mod calc;
mod catalog;
mod clipboard;
mod config;
mod convert;
mod files;
mod fuzzy;
mod hotkeys;
mod login;
mod menubar;
mod settings;
mod usage;
mod wm;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use catalog::{Action, Item, Kind};
use clipboard::History;
use config::Config;
use hotkeys::{Hotkey, Registry};
use settings::Settings;
use usage::Usage;
use rune::tok::{self, type_};
use rune::{
    Align, App, AppHandle, Latest, AppSpec, Color, Cx, DisplayTarget, El, Image, ImageCache, Key, KeyEvent, OverlayPlacement,
    ScreenEdge, ScrollSpec, TextEdit, TextStyle, Titlebar, Tx, Waker, WindowOptions, WindowSpec, col, el, row, spacer, text,
    text_input,
};
const WIDTH: f32 = 760.0;
const SEARCH_H: f32 = 60.0;
const ROW_H: f32 = 44.0;
const VISIBLE_ROWS: usize = 8;
const FOOTER_H: f32 = 38.0;
const HEIGHT: f32 = SEARCH_H + 1.0 + 8.0 + ROW_H * VISIBLE_ROWS as f32 + 8.0 + 1.0 + FOOTER_H;
const ICON: f32 = 26.0;
/// Runecast's logo, drawn by `mark` in the panel's footer and the Settings sidebar.
const LOGO: &[u8] = include_bytes!("../assets/icon.png");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Root,
    Clipboard,
    Files,
}

struct State {
    mode: Mode,
    query: TextEdit,
    config: Config,
    items: Vec<Item>,
    history: History,
    usage: Usage,
    /// Currency rates for conversions, fetched in the background when stale.
    rates: convert::Rates,
    /// File matches for the current query, from `file_search`.
    files: Vec<Item>,
    /// Made in `setup`; searches files off the UI thread and lands the newest answer in `poll`.
    file_search: Option<Latest<Vec<PathBuf>>>,
    results: Vec<Item>,
    picked: usize,
    /// Made in `setup`, which has the waker that redraws once an icon decodes.
    icons: Option<ImageCache>,
    logo: Option<Image>,
    /// Live handles keep decoded icons in the cache; one per app shown so far.
    icon_handles: HashMap<PathBuf, Image>,
    watching: bool,
    me: Option<AppHandle<State>>,
    registry: Option<Arc<Mutex<Registry>>>,
    settings: Option<AppHandle<Settings>>,
}

impl State {
    fn new(config: Config, items: Vec<Item>, history: History) -> Self {
        let mut query = TextEdit::new("");
        query.focused = true;
        let mut s = State {
            mode: Mode::Root, query, config, items, history, usage: Usage::default(), rates: convert::Rates::default(), files: Vec::new(), file_search: None,
            results: Vec::new(), picked: 0,
            icons: None, logo: None, icon_handles: HashMap::new(), watching: false, me: None, registry: None, settings: None,
        };
        s.refresh();
        s
    }

    /// The query changed: list matches again and start a file search for it.
    fn refresh(&mut self) {
        let query = self.query.text.trim().to_string();
        if let Some(search) = &self.file_search {
            if self.mode != Mode::Clipboard && query.chars().count() >= files::MIN_QUERY {
                search.request(query, |q| files::search(&q));
            } else {
                search.cancel();
                self.files.clear();
            }
        }
        self.relist();
        self.picked = 0;
    }

    /// The rows for the current query, mode and file matches.
    fn relist(&mut self) {
        let (now, clock) = (usage::now(), jiff::Zoned::now());
        let boost = |it: &Item| self.usage.boost(it, now);
        let ctx = catalog::Ctx {
            engine: self.config.engine(), boost: &boost, files: &self.files, rates: &self.rates,
            aliases: &self.config.aliases, now: &clock,
        };
        self.results = match self.mode {
            Mode::Root => catalog::search(&self.items, &self.query.text, &ctx),
            Mode::Clipboard => self.history.search(&self.query.text).into_iter().map(|t| clip_item(t)).collect(),
            Mode::Files => self.files.clone(),
        };
        self.picked = self.picked.min(self.results.len().saturating_sub(1));
        self.load_icons();
    }

    fn load_icons(&mut self) {
        let Some(cache) = &self.icons else { return };
        for path in self.results.iter().filter_map(|it| it.icon.as_ref()) {
            self.icon_handles.entry(path.clone()).or_insert_with(|| cache.app_icon(path, ICON));
        }
    }

    fn enter(&mut self, mode: Mode) {
        self.mode = mode;
        self.query.replace_text("");
        self.refresh();
    }

    /// Back to the root list with an empty query, so the next ⌥Space starts fresh.
    fn reset(&mut self) {
        self.enter(Mode::Root);
    }

    /// Shows the panel in `mode`, with apps and the config read again so changes appear.
    fn open(&mut self, mode: Mode) {
        let Some(me) = self.me.clone() else { return };
        self.config = config::load();
        self.items = catalog::load(&self.config);
        self.enter(mode);
        me.place(DisplayTarget::Primary, OverlayPlacement::Edge {
            edge: ScreenEdge::Top, margin: 180.0, align: Align::Center, inset: 0.0,
        });
        me.show();
    }

    /// What a global hotkey does.
    fn fire(&mut self, hotkey: Hotkey) {
        match hotkey {
            Hotkey::Launcher => match &self.me {
                Some(me) if me.is_visible() && self.mode == Mode::Root => me.close(),
                _ => self.open(Mode::Root),
            },
            Hotkey::Clipboard => self.open(Mode::Clipboard),
            Hotkey::Window(layout) => wm::apply(layout),
        }
    }

    /// Takes the config the Settings window just saved.
    fn apply_config(&mut self, config: Config) {
        self.history.entries.truncate(config.general.clipboard_size);
        self.config = config;
    }

    fn open_settings(&mut self, tx: &mut Tx) {
        tx.close_window();
        self.show_settings();
    }

    /// Hides the panel and brings up Settings, from the panel or the menu bar.
    fn show_settings(&mut self) {
        self.reset();
        if let Some(me) = &self.me {
            me.close();
        }
        if let Some(settings) = &self.settings {
            settings.update(|s, _| s.reload());
            settings.open();
        }
    }
}

fn clip_item(text: &str) -> Item {
    let first = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
    Item::new(first, "", Kind::Clip, Action::Paste(text.to_string()))
}

fn setup(s: &mut State, waker: Waker) {
    s.file_search = Some(Latest::new(waker.clone()).debounce(std::time::Duration::from_millis(120)));
    let cache = ImageCache::new(waker);
    s.logo = Some(cache.load(LOGO));
    s.icons = Some(cache);
    s.load_icons();
    if rune::is_headless() {
        // Screenshots show icons, not the frame before they decode.
        for icon in s.icon_handles.values().chain(&s.logo) {
            let _ = icon.wait();
        }
    } else if let Some(me) = s.me.clone() && !s.watching {
        s.watching = true;
        clipboard::watch(me, |s: &mut State, text| {
            if s.config.general.clipboard_history && s.history.push(text, s.config.general.clipboard_size) {
                s.history.save();
                if s.mode == Mode::Clipboard {
                    s.refresh();
                }
            }
        });
    }
}

/// A file search landed: show it, keeping the selection where it was.
fn poll(s: &mut State, _: f64, _: &mut Tx) -> bool {
    let Some(hits) = s.file_search.as_ref().and_then(Latest::take) else { return false };
    s.files = hits.iter().map(|p| files::item(p)).collect();
    s.relist();
    true
}

fn run_picked(s: &mut State, tx: &mut Tx) {
    let Some(item) = s.results.get(s.picked).cloned() else { return };
    // Rows that are answers to this query rather than things to come back to don't count.
    if !matches!(item.kind, Kind::Calculator | Kind::Web | Kind::Clip) && s.me.is_some() {
        s.usage.record(&item, usage::now());
        s.usage.save();
    }
    match item.action {
        Action::Url(url) => {
            let _ = tx.open_url(&url);
        }
        Action::Copy(value) => tx.copy(value),
        Action::Paste(value) => {
            tx.copy(value);
            catalog::paste_after_copy();
        }
        Action::Quicklink { url, keyword } if url.contains("{query}") => {
            s.query.replace_text(format!("{keyword} "));
            return s.refresh();
        }
        Action::Quicklink { url, .. } => {
            let _ = tx.open_url(&url);
        }
        Action::Window(layout) => wm::apply(layout),
        Action::ClipboardHistory => return s.enter(Mode::Clipboard),
        Action::FileSearch => return s.enter(Mode::Files),
        Action::Settings => return s.open_settings(tx),
        Action::Quit => return tx.quit(),
        action => catalog::spawn(&action),
    }
    s.reset();
    tx.close_window();
}

fn on_key(s: &mut State, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let n = s.results.len();
    match ev.key {
        Key::Down if n > 0 => s.picked = (s.picked + 1) % n,
        Key::Up if n > 0 => s.picked = (s.picked + n - 1) % n,
        Key::Escape if !s.query.text.is_empty() => {
            s.query.replace_text("");
            s.refresh();
        }
        Key::Escape | Key::Backspace if s.mode != Mode::Root && s.query.text.is_empty() => s.reset(),
        Key::Escape => tx.close_window(),
        // ⌘↩ on a file shows it in Finder instead of opening it.
        Key::Enter if ev.mods.cmd => {
            let Some(Item { action: Action::OpenPath(path), kind: Kind::File, .. }) = s.results.get(s.picked) else { return false };
            files::reveal(path);
            s.reset();
            tx.close_window();
            return true;
        }
        _ if ev.is_command(',') => {
            s.open_settings(tx);
            return true;
        }
        _ => return false,
    }
    tx.scroll_into_view(format!("row-{}", s.picked));
    true
}

fn view(s: &State, cx: &mut Cx<State>) -> El {
    let dark = s.config.general.appearance.dark(cx.system_dark());
    let t = tok::theme(dark);
    let accent = tok::accent(0, dark);

    let placeholder = match s.mode {
        Mode::Root => "Search for apps and commands…",
        Mode::Clipboard => "Search clipboard history…",
        Mode::Files => "Search files by name…",
    };
    let field = text_input(&s.query, TextStyle::new(19.0, 400.0).lh(28.0)).key("query").single_line().grow(1.0)
        .color(t.text).placeholder(placeholder, t.text_secondary.alpha(0.8))
        .on_edit(|s: &mut State| &mut s.query)
        .on_input_change(|s: &mut State, _| s.refresh())
        .on_input_key(on_key)
        .on_submit(run_picked);
    let mut search = row().h(SEARCH_H).px(20.0).gap(14.0).center_y()
        .child(text("⌕").font(TextStyle::new(22.0, 400.0).lh(28.0)).color(t.text_secondary));
    let chip = match s.mode {
        Mode::Root => None,
        Mode::Clipboard => Some("Clipboard History"),
        Mode::Files => Some("Search Files"),
    };
    if let Some(chip) = chip {
        search = search.child(row().h(26.0).px(10.0).radius(7.0).center().fill(accent.alpha(0.2))
            .child(text(chip).font(type_::LABEL).color(t.text)));
    }
    let search = search.child(field);

    let compact = s.mode == Mode::Clipboard;
    let mut list = col().key("results").w_full().h_full().padding(8.0).scroll(ScrollSpec { stick_bottom: false });
    for (i, item) in s.results.iter().enumerate() {
        let icon = item.icon.as_ref().and_then(|p| s.icon_handles.get(p));
        list = list.child(result_row(item, icon, i == s.picked, compact, dark, accent, &t)
            .key(format!("row-{i}"))
            .on_hover(move |s: &mut State| s.picked = i)
            .on_press_tx(move |s: &mut State, tx| {
                s.picked = i;
                run_picked(s, tx);
            }));
    }
    let empty = match s.mode {
        Mode::Clipboard if s.history.entries.is_empty() => "Nothing copied yet",
        Mode::Files if s.query.text.trim().chars().count() < files::MIN_QUERY => "Type a file name",
        _ => "No results",
    };
    let list = if s.results.is_empty() {
        col().w_full().h_full().center().child(text(empty).font(type_::BODY).color(t.text_secondary))
    } else {
        list
    };

    let body = match s.mode {
        Mode::Root | Mode::Files => row().w_full().grow(1.0).min_h(0.0).child(list),
        Mode::Clipboard => row().w_full().grow(1.0).min_h(0.0)
            .child(col().w(300.0).h_full().min_h(0.0).child(list))
            .child(el().w(1.0).h_full().fill(t.separator))
            .child(preview(s.results.get(s.picked), &t)),
    };

    let action = s.results.get(s.picked).map_or("", |it| it.kind.verb());
    let footer = row().h(FOOTER_H).px(16.0).gap(10.0).center_y().fill(t.bg.alpha(0.5))
        .child(mark(s.logo.as_ref(), 16.0, accent))
        .child(text("Runecast").font(type_::LABEL).color(t.text_secondary))
        .child(spacer())
        .child(text(action).font(type_::LABEL).color(t.text))
        .child(keycap("↩", &t))
        .child(el().w(1.0).h(16.0).fill(t.separator))
        .child(text(if s.mode == Mode::Root { "Close" } else { "Back" }).font(type_::LABEL).color(t.text_secondary))
        .child(keycap("esc", &t));

    col().key("panel").w_full().h_full().radius(14.0).clip_rounded(14.0)
        .fill(t.card).border(1.0, if dark { Color::WHITE.alpha(0.1) } else { Color::BLACK.alpha(0.08) })
        .child(search)
        .child(el().h(1.0).w_full().fill(t.separator))
        .child(body)
        .child(el().h(1.0).w_full().fill(t.separator))
        .child(footer)
}

/// The selected clipboard entry in full, with its size underneath.
fn preview(item: Option<&Item>, t: &tok::Theme) -> El {
    let Some(Item { action: Action::Paste(clip), .. }) = item else { return col().grow(1.0) };
    let stat = |label: &'static str, value: String| row().w_full().h(24.0).center_y()
        .child(text(label).font(type_::LABEL).color(t.text_secondary))
        .child(spacer())
        .child(text(value).font(type_::LABEL).color(t.text));
    col().key("preview").grow(1.0).h_full().min_w(0.0).padding(18.0).gap(4.0)
        .child(text(clip.clone()).font(type_::BODY.lh(20.0)).color(t.text).max_lines(14).grow(1.0))
        .child(el().h(1.0).w_full().fill(t.separator))
        .child(stat("Characters", clip.chars().count().to_string()))
        .child(stat("Words", clip.split_whitespace().count().to_string()))
        .child(stat("Lines", clip.lines().count().max(1).to_string()))
}

fn result_row(item: &Item, icon: Option<&Image>, on: bool, compact: bool, dark: bool, accent: Color, t: &tok::Theme) -> El {
    let leading = match icon {
        Some(img) if img.error().is_none() => el().size(ICON, ICON).pixels(img.pixels()),
        _ => badge(item, dark, t),
    };
    let title_style = if item.kind == Kind::Calculator { TextStyle::new(15.0, 600.0) } else { TextStyle::new(14.0, 500.0) };
    let mut r = row().h(ROW_H).px(10.0).gap(12.0).radius(9.0).center_y()
        .fill(if on { accent.alpha(if dark { 0.28 } else { 0.16 }) } else { Color::TRANSPARENT })
        .child(leading);
    if compact {
        return r.child(text(item.title.clone()).font(title_style).color(t.text).truncate().grow(1.0));
    }
    r = r.child(text(item.title.clone()).font(title_style).color(t.text).truncate())
        .child(text(item.subtitle.clone()).font(type_::BODY).color(t.text_secondary).truncate().grow(1.0));
    if let Some(hotkey) = &item.hotkey {
        r = r.child(keycap(hotkey.clone(), t));
    }
    r.child(text(item.kind.label()).font(type_::LABEL).color(t.text_secondary))
}

/// The fallback when there's no icon: the app's initial on a colour picked from its name, or the
/// row's kind.
fn badge(item: &Item, dark: bool, t: &tok::Theme) -> El {
    let glyph = match item.kind {
        Kind::Calculator => "=".to_string(),
        Kind::Web => "⌕".to_string(),
        Kind::Command => "⚙".to_string(),
        Kind::Snippet => "¶".to_string(),
        Kind::Quicklink => "↗".to_string(),
        Kind::Window => "◧".to_string(),
        Kind::Clip => "T".to_string(),
        Kind::File => "≡".to_string(),
        Kind::App => item.title.chars().find(|c| c.is_alphanumeric()).unwrap_or('•').to_uppercase().to_string(),
    };
    let hue = item.title.bytes().fold(0usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize));
    let (fill, ink) = match item.kind {
        Kind::App => (tok::accent(hue % 6, dark), Color::WHITE),
        _ => (t.control_off, t.text),
    };
    row().size(ICON, ICON).radius(7.0).center().fill(fill).child(text(glyph).font(type_::TITLE).color(ink))
}

/// The logo at `size` points, or a plain square of `fallback` until it decodes.
fn mark(logo: Option<&Image>, size: f32, fallback: Color) -> El {
    match logo {
        Some(img) if img.is_ready() && img.error().is_none() => el().size(size, size).pixels(img.pixels()),
        _ => el().size(size, size).radius(size * 0.22).fill(fallback),
    }
}

fn keycap(label: impl Into<String>, t: &tok::Theme) -> El {
    row().h(20.0).px(6.0).radius(5.0).center().fill(t.control_off)
        .child(text(label.into()).font(type_::LABEL).color(t.text_secondary))
}

/// A fixed catalog and history, so headless scenes render the same on every machine.
fn demo_items() -> Vec<Item> {
    let app = |name: &str, folder: &str| Item::new(name, folder, Kind::App, Action::Shell(String::new()));
    let mut items = vec![
        app("Arc", "/Applications"), app("Calendar", "/System/Applications"), app("Figma", "/Applications"),
        app("Finder", "/System/Library/CoreServices"), app("Linear", "/Applications"), app("Mail", "/System/Applications"),
        app("Notes", "/System/Applications"), app("Safari", "/Applications"), app("Slack", "/Applications"),
        app("Spotify", "/Applications"), app("System Settings", "/System/Applications"),
        app("Terminal", "/System/Applications/Utilities"), app("Visual Studio Code", "/Applications"),
    ];
    for title in ["Lock Screen", "Sleep", "Toggle Dark Mode", "Empty Trash", "Volume Up", "Volume Down", "Toggle Mute"] {
        items.push(Item::new(title, "System", Kind::Command, Action::Shell(String::new())));
    }
    let config = config::example();
    items.extend(catalog::commands(&config).into_iter().filter(|it| it.subtitle != "System"));
    items.extend(catalog::layouts(&config));
    items.extend(catalog::extras(&config));
    catalog::show_aliases(&mut items, &config);
    items
}

fn demo_history() -> History {
    let mut h = History::default();
    for clip in [
        "cargo run --release -- --snap docs/apps.png --scene apps --scale 2",
        "Ayala Avenue, Makati, Metro Manila",
        "fn score(query: &str, candidate: &str) -> Option<i32> {\n    let query: Vec<char> = query.chars().collect();\n    // every query character must appear in order\n    todo!()\n}",
        "https://github.com/anomalyco/rune",
    ] {
        h.push(clip.into(), 200);
    }
    h
}

fn scene(s: &mut State, mode: Mode, query: &str) {
    *s = State::new(config::example(), demo_items(), demo_history());
    s.rates = convert::Rates::demo();
    s.mode = mode;
    s.query.replace_text(query);
    s.refresh();
}

/// The machine's own apps, to check real icons headless.
fn installed(s: &mut State) {
    let config = config::example();
    *s = State::new(config.clone(), catalog::load(&config), History::default());
}

const SCENES: &[(&str, fn(&mut State))] = &[
    ("empty", |s| scene(s, Mode::Root, "")),
    ("apps", |s| scene(s, Mode::Root, "vsc")),
    ("calc", |s| scene(s, Mode::Root, "(1200+340)*1.12")),
    ("commands", |s| scene(s, Mode::Root, "dark")),
    ("web", |s| scene(s, Mode::Root, "rune rust ui")),
    ("windows", |s| scene(s, Mode::Root, "half")),
    ("snippets", |s| scene(s, Mode::Root, "sig")),
    ("quicklink", |s| scene(s, Mode::Root, "gh rune")),
    ("clipboard", |s| {
        scene(s, Mode::Clipboard, "");
        s.picked = 1;
    }),
    ("files", |s| {
        scene(s, Mode::Root, "report");
        let home = std::env::var("HOME").unwrap_or_default();
        s.files = ["Documents/Q3 Report.pdf", "Desktop/report-draft.docx", "Projects/runecast/docs/report.md"]
            .iter().map(|f| files::item(&std::path::Path::new(&home).join(f))).collect();
        s.relist();
    }),
    ("convert", |s| scene(s, Mode::Root, "5 km in mi")),
    ("currency", |s| scene(s, Mode::Root, "100 usd to php")),
    ("timezone", |s| scene(s, Mode::Root, "3pm pst in manila")),
    ("alias", |s| scene(s, Mode::Root, "vs")),
    ("system", |s| scene(s, Mode::Root, "volume")),
    ("installed", installed),
];

fn main() {
    rune::prewarm_gpu();
    rune::run_windows(|_displays| {
        let spec = AppSpec {
            scenes: SCENES,
            setup: Some(setup),
            poll: Some(poll),
            ..AppSpec::new("Runecast", (WIDTH, HEIGHT), || unreachable!(), view)
        };
        let launcher = App::new(spec).windowless().accessory();
        let settings_spec = AppSpec {
            scenes: settings::SCENES,
            setup: Some(settings::setup),
            min_size: Some((640.0, 460.0)),
            ..AppSpec::new("Runecast Settings", settings::SIZE, || unreachable!(), settings::view)
        };
        let settings = App::new(settings_spec).windowless().keep_running().titlebar(Titlebar::new(settings::TITLEBAR));
        let (me, settings_handle) = (launcher.handle(), settings.handle());
        let registry = Arc::new(Mutex::new(Registry::new(me.clone())));

        let shared = registry.clone();
        let launcher = launcher
            .init(move || {
                let config = config::load();
                let mut s = State::new(config.clone(), catalog::load(&config), History::load());
                s.usage = Usage::load();
                s.rates = convert::Rates::load();
                s.me = Some(me);
                s.relist();
                s.registry = Some(shared);
                s.settings = Some(settings_handle);
                s
            })
            .on_focus(|s, focused, tx| if !focused {
                s.reset();
                tx.close_window();
            });
        // Hotkeys bind once the app runs; Settings rebinds them through the same registry.
        launcher.handle().update(|s, _| {
            if let Some(registry) = &s.registry {
                registry.lock().unwrap().register_all(&s.config);
            }
            if !rune::is_headless() && let Some(me) = s.me.clone() {
                if let Err(e) = menubar::install(me.clone()) {
                    eprintln!("runecast: no menu bar icon: {e}");
                }
                convert::refresh_in_background(s.rates.is_stale(usage::now()), move |rates| me.update(move |s, _| s.rates = rates));
            }
        });
        let launcher_handle = launcher.handle();
        let settings = settings.init(move || Settings::new(config::load(), Some(registry), Some(launcher_handle)));

        let options = WindowOptions::launcher_panel().macos(|native| native.corner_radius(14.0));
        let windows: Vec<rune::AnyWindowSpec> = vec![
            WindowSpec::from_app("launcher", launcher).configured_options(options).into(),
            WindowSpec::from_app("settings", settings).into(),
        ];
        windows
    });
}
