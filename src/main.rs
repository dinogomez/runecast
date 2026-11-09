//! Runecast: a Raycast-style launcher built with Rune.
//!
//! `cargo run --release` starts it in the background; ⌥Space (Alt+Space elsewhere) shows the panel.
//! Type to search apps, commands, snippets and quicklinks, do arithmetic inline, or fall through to
//! a web search. ⌃⌥V opens clipboard history; ⌃⌥←/→/↩/C move the front window on macOS.
//!
//! Headless, with no window: `runecast --snap out.png --scene apps` renders a scene to PNG.

mod calc;
mod catalog;
mod clipboard;
mod config;
mod fuzzy;
mod wm;

use std::collections::HashMap;
use std::path::PathBuf;

use catalog::{Action, Item, Kind};
use clipboard::History;
use rune::tok::{self, type_};
use rune::{
    Align, App, AppHandle, AppSpec, Color, Cx, DisplayTarget, El, Image, ImageCache, Key, KeyEvent, OverlayPlacement,
    ScreenEdge, ScrollSpec, TextEdit, TextStyle, Tx, Waker, WindowOptions, WindowSpec, col, el, row, spacer, text,
    text_input,
};
use wm::Layout;

const HOTKEY: &str = "alt+space";
const CLIPBOARD_HOTKEY: &str = "ctrl+alt+v";
const WIDTH: f32 = 760.0;
const SEARCH_H: f32 = 60.0;
const ROW_H: f32 = 44.0;
const VISIBLE_ROWS: usize = 8;
const FOOTER_H: f32 = 38.0;
const HEIGHT: f32 = SEARCH_H + 1.0 + 8.0 + ROW_H * VISIBLE_ROWS as f32 + 8.0 + 1.0 + FOOTER_H;
const ICON: f32 = 26.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Root,
    Clipboard,
}

struct State {
    mode: Mode,
    query: TextEdit,
    items: Vec<Item>,
    history: History,
    results: Vec<Item>,
    picked: usize,
    /// Made in `setup`, which has the waker that redraws once an icon decodes.
    icons: Option<ImageCache>,
    /// Live handles keep decoded icons in the cache; one per app shown so far.
    icon_handles: HashMap<PathBuf, Image>,
    watching: bool,
    me: Option<AppHandle<State>>,
}

impl State {
    fn new(items: Vec<Item>, history: History, me: Option<AppHandle<State>>) -> Self {
        let mut query = TextEdit::new("");
        query.focused = true;
        let mut s = State {
            mode: Mode::Root, query, items, history, results: Vec::new(), picked: 0,
            icons: None, icon_handles: HashMap::new(), watching: false, me,
        };
        s.refresh();
        s
    }

    fn refresh(&mut self) {
        self.results = match self.mode {
            Mode::Root => catalog::search(&self.items, &self.query.text),
            Mode::Clipboard => self.history.search(&self.query.text).into_iter().map(|t| clip_item(t)).collect(),
        };
        self.picked = 0;
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
        self.items = catalog::load(&config::load());
        self.enter(mode);
        me.place(DisplayTarget::Primary, OverlayPlacement::Edge {
            edge: ScreenEdge::Top, margin: 180.0, align: Align::Center, inset: 0.0,
        });
        me.show();
    }
}

fn clip_item(text: &str) -> Item {
    let first = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
    Item::new(first, "", Kind::Clip, Action::Paste(text.to_string()))
}

fn setup(s: &mut State, waker: Waker) {
    s.icons = Some(ImageCache::new(waker));
    s.load_icons();
    if rune::is_headless() {
        // Screenshots show icons, not the frame before they decode.
        for icon in s.icon_handles.values() {
            let _ = icon.wait();
        }
    } else if let Some(me) = s.me.clone() && !s.watching {
        s.watching = true;
        clipboard::watch(me, |s: &mut State, text| {
            if s.history.push(text) {
                s.history.save();
                if s.mode == Mode::Clipboard {
                    s.refresh();
                }
            }
        });
    }
}

fn run_picked(s: &mut State, tx: &mut Tx) {
    let Some(item) = s.results.get(s.picked).cloned() else { return };
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
        _ => return false,
    }
    tx.scroll_into_view(format!("row-{}", s.picked));
    true
}

fn view(s: &State, cx: &mut Cx<State>) -> El {
    let dark = cx.system_dark();
    let t = tok::theme(dark);
    let accent = tok::accent(0, dark);

    let placeholder = match s.mode {
        Mode::Root => "Search for apps and commands…",
        Mode::Clipboard => "Search clipboard history…",
    };
    let field = text_input(&s.query, TextStyle::new(19.0, 400.0).lh(28.0)).key("query").single_line().grow(1.0)
        .color(t.text).placeholder(placeholder, t.text_secondary.alpha(0.8))
        .on_edit(|s: &mut State| &mut s.query)
        .on_input_change(|s: &mut State, _| s.refresh())
        .on_input_key(on_key)
        .on_submit(run_picked);
    let mut search = row().h(SEARCH_H).px(20.0).gap(14.0).center_y()
        .child(text("⌕").font(TextStyle::new(22.0, 400.0).lh(28.0)).color(t.text_secondary));
    if s.mode == Mode::Clipboard {
        search = search.child(row().h(26.0).px(10.0).radius(7.0).center().fill(accent.alpha(0.2))
            .child(text("Clipboard History").font(type_::LABEL).color(t.text)));
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
        _ => "No results",
    };
    let list = if s.results.is_empty() {
        col().w_full().h_full().center().child(text(empty).font(type_::BODY).color(t.text_secondary))
    } else {
        list
    };

    let body = match s.mode {
        Mode::Root => row().w_full().grow(1.0).min_h(0.0).child(list),
        Mode::Clipboard => row().w_full().grow(1.0).min_h(0.0)
            .child(col().w(300.0).h_full().min_h(0.0).child(list))
            .child(el().w(1.0).h_full().fill(t.separator))
            .child(preview(s.results.get(s.picked), &t)),
    };

    let action = s.results.get(s.picked).map_or("", |it| it.kind.verb());
    let footer = row().h(FOOTER_H).px(16.0).gap(10.0).center_y().fill(t.bg.alpha(0.5))
        .child(el().size(14.0, 14.0).radius(4.0).fill(accent))
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
    if let Some(hotkey) = item.hotkey {
        r = r.child(keycap(hotkey, t));
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
        Kind::App => item.title.chars().find(|c| c.is_alphanumeric()).unwrap_or('•').to_uppercase().to_string(),
    };
    let hue = item.title.bytes().fold(0usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize));
    let (fill, ink) = match item.kind {
        Kind::App => (tok::accent(hue % 6, dark), Color::WHITE),
        _ => (t.control_off, t.text),
    };
    row().size(ICON, ICON).radius(7.0).center().fill(fill).child(text(glyph).font(type_::TITLE).color(ink))
}

fn keycap(label: &'static str, t: &tok::Theme) -> El {
    row().h(20.0).px(6.0).radius(5.0).center().fill(t.control_off)
        .child(text(label).font(type_::LABEL).color(t.text_secondary))
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
    for title in ["Lock Screen", "Sleep", "Toggle Dark Mode", "Empty Trash"] {
        items.push(Item::new(title, "System", Kind::Command, Action::Shell(String::new())));
    }
    items.push(Item { hotkey: Some("⌃⌥V"), ..Item::new("Clipboard History", "Runecast", Kind::Command, Action::ClipboardHistory) });
    items.extend(catalog::layouts());
    items.extend(catalog::extras(&config::example()));
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
        h.push(clip.into());
    }
    h
}

fn scene(s: &mut State, mode: Mode, query: &str) {
    *s = State::new(demo_items(), demo_history(), None);
    s.mode = mode;
    s.query.replace_text(query);
    s.refresh();
}

/// The machine's own apps, to check real icons headless.
fn installed(s: &mut State) {
    *s = State::new(catalog::load(&config::example()), History::default(), None);
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
    ("installed", installed),
];

fn main() {
    rune::prewarm_gpu();
    rune::run_windows(|_displays| {
        let spec = AppSpec {
            scenes: SCENES,
            setup: Some(setup),
            ..AppSpec::new("Runecast", (WIDTH, HEIGHT), || unreachable!(), view)
        };
        let app = App::new(spec).windowless().accessory();
        let me = app.handle();
        let mut app = app
            .init(move || State::new(catalog::load(&config::load()), History::load(), Some(me)))
            .global_shortcut(HOTKEY, |s, _| match &s.me {
                Some(me) if me.is_visible() && s.mode == Mode::Root => me.close(),
                _ => s.open(Mode::Root),
            })
            .global_shortcut(CLIPBOARD_HOTKEY, |s, _| s.open(Mode::Clipboard))
            .on_focus(|s, focused, tx| if !focused {
                s.reset();
                tx.close_window();
            });
        if cfg!(target_os = "macos") {
            app = app
                .global_shortcut("ctrl+alt+left", |_, _| wm::apply(Layout::LeftHalf))
                .global_shortcut("ctrl+alt+right", |_, _| wm::apply(Layout::RightHalf))
                .global_shortcut("ctrl+alt+enter", |_, _| wm::apply(Layout::Maximize))
                .global_shortcut("ctrl+alt+c", |_, _| wm::apply(Layout::Center));
        }
        let options = WindowOptions::launcher_panel().macos(|native| native.corner_radius(14.0));
        let windows: Vec<rune::AnyWindowSpec> = vec![WindowSpec::from_app("launcher", app).configured_options(options).into()];
        windows
    });
}
