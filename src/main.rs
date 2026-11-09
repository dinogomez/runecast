//! Runecast: a Raycast-style launcher built with Rune.
//!
//! `cargo run --release` starts it in the background; ⌥Space (Alt+Space elsewhere) shows the panel.
//! Type to search apps and commands, do arithmetic inline, or fall through to a web search.
//!
//! Headless, with no window: `runecast --snap out.png --scene apps` renders a scene to PNG.

mod calc;
mod catalog;
mod fuzzy;

use catalog::{Action, Item, Kind, MAX_RESULTS};
use rune::tok::{self, type_};
use rune::{
    Align, App, AppHandle, AppSpec, Color, Cx, DisplayTarget, El, Key, KeyEvent, OverlayPlacement, ScreenEdge,
    TextEdit, TextStyle, Tx, WindowOptions, WindowSpec, col, el, row, spacer, text, text_input,
};

const HOTKEY: &str = "alt+space";
const WIDTH: f32 = 720.0;
const SEARCH_H: f32 = 60.0;
const ROW_H: f32 = 44.0;
const FOOTER_H: f32 = 38.0;
const HEIGHT: f32 = SEARCH_H + 1.0 + 8.0 + ROW_H * MAX_RESULTS as f32 + 8.0 + 1.0 + FOOTER_H;

struct State {
    query: TextEdit,
    items: Vec<Item>,
    results: Vec<Item>,
    picked: usize,
    me: Option<AppHandle<State>>,
}

impl State {
    fn new(items: Vec<Item>, me: Option<AppHandle<State>>) -> Self {
        let mut query = TextEdit::new("");
        query.focused = true;
        let results = catalog::search(&items, "");
        State { query, items, results, picked: 0, me }
    }

    fn refresh(&mut self) {
        self.results = catalog::search(&self.items, &self.query.text);
        self.picked = 0;
    }

    /// Back to an empty query, so the next ⌥Space starts fresh.
    fn reset(&mut self) {
        self.query.clear();
        self.refresh();
    }
}

fn run_picked(s: &mut State, tx: &mut Tx) {
    let Some(item) = s.results.get(s.picked).cloned() else { return };
    match &item.action {
        Action::Url(url) => {
            let _ = tx.open_url(url);
        }
        Action::Copy(value) => tx.copy(value.clone()),
        Action::Quit => return tx.quit(),
        action => catalog::spawn(action),
    }
    s.reset();
    tx.close_window();
}

fn view(s: &State, cx: &mut Cx<State>) -> El {
    let dark = cx.system_dark();
    let t = tok::theme(dark);
    let accent = tok::accent(0, dark);

    let field = text_input(&s.query, TextStyle::new(19.0, 400.0).lh(28.0)).key("query").single_line().grow(1.0)
        .color(t.text).placeholder("Search for apps and commands…", t.text_secondary.alpha(0.8))
        .on_edit(|s: &mut State| &mut s.query)
        .on_input_change(|s: &mut State, _| s.refresh())
        .on_input_key(|s: &mut State, ev: &KeyEvent, tx: &mut Tx| {
            let n = s.results.len();
            match ev.key {
                Key::Down if n > 0 => s.picked = (s.picked + 1) % n,
                Key::Up if n > 0 => s.picked = (s.picked + n - 1) % n,
                Key::Escape if !s.query.text.is_empty() => s.reset(),
                Key::Escape => tx.close_window(),
                _ => return false,
            }
            true
        })
        .on_submit(run_picked);
    let search = row().h(SEARCH_H).px(20.0).gap(14.0).center_y()
        .child(text("⌕").font(TextStyle::new(22.0, 400.0).lh(28.0)).color(t.text_secondary))
        .child(field);

    let mut list = col().key("results").w_full().grow(1.0).padding(8.0).gap(0.0);
    if s.results.is_empty() {
        list = list.center().child(text("No results").font(type_::BODY).color(t.text_secondary));
    }
    for (i, item) in s.results.iter().enumerate() {
        let on = i == s.picked;
        list = list.child(result_row(item, on, dark, accent, &t)
            .key(format!("row-{i}"))
            .on_hover(move |s: &mut State| s.picked = i)
            .on_press_tx(move |s: &mut State, tx| {
                s.picked = i;
                run_picked(s, tx);
            }));
    }

    let action = s.results.get(s.picked).map_or("", |it| match it.kind {
        Kind::App => "Open Application",
        Kind::Command => "Run Command",
        Kind::Calculator => "Copy Answer",
        Kind::Web => "Search the Web",
    });
    let footer = row().h(FOOTER_H).px(16.0).gap(10.0).center_y().fill(t.bg.alpha(0.5))
        .child(el().size(14.0, 14.0).radius(4.0).fill(accent))
        .child(text("Runecast").font(type_::LABEL).color(t.text_secondary))
        .child(spacer())
        .child(text(action).font(type_::LABEL).color(t.text))
        .child(keycap("↩", &t))
        .child(el().w(1.0).h(16.0).fill(t.separator))
        .child(text("Close").font(type_::LABEL).color(t.text_secondary))
        .child(keycap("esc", &t));

    col().key("panel").w_full().h_full().radius(14.0).clip_rounded(14.0)
        .fill(t.card).border(1.0, if dark { Color::WHITE.alpha(0.1) } else { Color::BLACK.alpha(0.08) })
        .child(search)
        .child(el().h(1.0).w_full().fill(t.separator))
        .child(list)
        .child(el().h(1.0).w_full().fill(t.separator))
        .child(footer)
}

fn result_row(item: &Item, on: bool, dark: bool, accent: Color, t: &tok::Theme) -> El {
    let initial = item.title.chars().find(|c| c.is_alphanumeric()).unwrap_or('•').to_uppercase().to_string();
    let glyph = match item.kind {
        Kind::Calculator => "=".to_string(),
        Kind::Web => "⌕".to_string(),
        Kind::Command => "⚙".to_string(),
        Kind::App => initial,
    };
    let hue = item.title.bytes().fold(0usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize));
    let badge_fill = match item.kind {
        Kind::App => tok::accent(hue % 6, dark),
        _ => t.control_off,
    };
    let badge_ink = if item.kind == Kind::App { Color::WHITE } else { t.text };
    let badge = row().size(26.0, 26.0).radius(7.0).center().fill(badge_fill)
        .child(text(glyph).font(type_::TITLE).color(badge_ink));
    let title_style = if item.kind == Kind::Calculator { TextStyle::new(15.0, 600.0) } else { TextStyle::new(14.0, 500.0) };
    row().h(ROW_H).px(10.0).gap(12.0).radius(9.0).center_y()
        .fill(if on { accent.alpha(if dark { 0.28 } else { 0.16 }) } else { Color::TRANSPARENT })
        .child(badge)
        .child(text(item.title.clone()).font(title_style).color(t.text).truncate())
        .child(text(item.subtitle.clone()).font(type_::BODY).color(t.text_secondary).truncate().grow(1.0))
        .child(text(item.kind.label()).font(type_::LABEL).color(t.text_secondary))
}

fn keycap(label: &'static str, t: &tok::Theme) -> El {
    row().h(20.0).px(6.0).radius(5.0).center().fill(t.control_off)
        .child(text(label).font(type_::LABEL).color(t.text_secondary))
}

/// A fixed catalog, so headless scenes render the same on every machine.
fn demo_items() -> Vec<Item> {
    let app = |name: &str, folder: &str| Item {
        title: name.into(), subtitle: folder.into(), kind: Kind::App, action: Action::Shell(String::new()),
    };
    let mut items = vec![
        app("Arc", "/Applications"), app("Calendar", "/System/Applications"), app("Figma", "/Applications"),
        app("Finder", "/System/Library/CoreServices"), app("Linear", "/Applications"), app("Mail", "/System/Applications"),
        app("Notes", "/System/Applications"), app("Safari", "/Applications"), app("Slack", "/Applications"),
        app("Spotify", "/Applications"), app("System Settings", "/System/Applications"),
        app("Terminal", "/System/Applications/Utilities"), app("Visual Studio Code", "/Applications"),
    ];
    for (title, sub) in [("Lock Screen", "System"), ("Sleep", "System"), ("Toggle Dark Mode", "System"), ("Empty Trash", "System")] {
        items.push(Item { title: title.into(), subtitle: sub.into(), kind: Kind::Command, action: Action::Shell(String::new()) });
    }
    items
}

fn scene(query: &str, picked: usize) -> impl Fn(&mut State) {
    move |s| {
        *s = State::new(demo_items(), None);
        s.query = TextEdit::new(query);
        s.query.focused = true;
        s.refresh();
        s.picked = picked;
    }
}

const SCENES: &[(&str, fn(&mut State))] = &[
    ("empty", |s| scene("", 0)(s)),
    ("apps", |s| scene("vsc", 0)(s)),
    ("calc", |s| scene("(1200+340)*1.12", 0)(s)),
    ("commands", |s| scene("dark", 0)(s)),
    ("web", |s| scene("rune rust ui", 0)(s)),
];

fn main() {
    rune::prewarm_gpu();
    rune::run_windows(|_displays| {
        let spec = AppSpec { scenes: SCENES, ..AppSpec::new("Runecast", (WIDTH, HEIGHT), || unreachable!(), view) };
        let app = App::new(spec).windowless().accessory();
        let me = app.handle();
        let app = app
            .init(move || State::new(catalog::load(), Some(me)))
            .global_shortcut(HOTKEY, |s, _| {
                let Some(me) = &s.me else { return };
                if me.is_visible() {
                    me.close();
                    return;
                }
                me.place(DisplayTarget::Primary, OverlayPlacement::Edge {
                    edge: ScreenEdge::Top, margin: 180.0, align: Align::Center, inset: 0.0,
                });
                me.show();
            })
            .on_focus(|s, focused, tx| if !focused {
                s.reset();
                tx.close_window();
            });
        let options = WindowOptions::launcher_panel().macos(|native| native.corner_radius(14.0));
        let windows: Vec<rune::AnyWindowSpec> = vec![WindowSpec::from_app("launcher", app).configured_options(options).into()];
        windows
    });
}
