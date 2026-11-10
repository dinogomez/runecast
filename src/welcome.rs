//! The Welcome window: opens on the first launch (Runecast has no Dock icon or window, so a new
//! user would otherwise see only the menu bar icon), and from the menu bar after. It shows the
//! launcher hotkey, on macOS asks for the Accessibility permission pasting and window layouts
//! need, ticking it off once it's granted, and offers launch at login.

use std::time::Duration;

use rune::{AppHandle, Cx, El, Image, ImageCache, Tx, Waker, col, el, row, spacer, text};
use rune_kit::{Button, ButtonVariant, Kit, Switch};

use crate::settings::kit;

use crate::State;
use crate::config::{self, Appearance};
use crate::hotkeys::Hotkey;
use crate::login;

pub const SIZE: (f32, f32) = (520.0, 520.0);
/// The band the window drags by; the traffic lights sit in it on macOS.
pub const TITLEBAR: f32 = 40.0;

pub struct Welcome {
    /// The launcher hotkey as keycaps, such as ["⌥", "Space"].
    keys: Vec<String>,
    /// Whether to ask for Accessibility: macOS only, and scenes.
    macos: bool,
    appearance: Appearance,
    trusted: bool,
    /// From opening until closing, poll checks each second whether Accessibility is allowed.
    watching: bool,
    login: bool,
    logo: Option<(ImageCache, Image)>,
    /// None in headless scenes, which then never touch the disk.
    launcher: Option<AppHandle<State>>,
    me: Option<AppHandle<Welcome>>,
}

impl Welcome {
    pub fn new(launcher: Option<AppHandle<State>>, me: Option<AppHandle<Welcome>>) -> Self {
        let mut s = Welcome { keys: Vec::new(), macos: cfg!(target_os = "macos"), appearance: Appearance::System, trusted: false, watching: false, login: false, logo: None, launcher, me };
        s.reload();
        s
    }

    /// Reads the hotkey, appearance and permissions again, for each time the window opens.
    pub fn reload(&mut self) {
        let config = if self.launcher.is_some() { config::load() } else { config::example() };
        let shortcut = config.hotkey(Hotkey::Launcher).and_then(|a| a.parse::<rune::Shortcut>().ok());
        self.keys = shortcut.map(|s| s.keycaps()).unwrap_or_default();
        self.appearance = config.general.appearance;
        self.trusted = self.launcher.is_none() || accessibility::trusted();
        self.login = self.launcher.is_some() && login::enabled();
        // Starts the poll that ticks the permission off once it's granted.
        if let Some(me) = &self.me {
            self.watching = true;
            me.wake();
        }
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

    /// Closes the window and opens the launcher, so the first try is one click away.
    fn get_started(&mut self) {
        if let Some(me) = &self.me {
            me.close();
        }
        if let Some(launcher) = &self.launcher {
            launcher.update(|s, _| s.open(crate::Mode::Root));
        }
    }
}

/// Whether the Welcome window has opened before: a marker file beside `config.json`, since the
/// Settings window rewrites that file whole.
pub fn seen() -> bool {
    marker().exists()
}

pub fn mark_seen() {
    let _ = std::fs::create_dir_all(config::dir());
    let _ = std::fs::write(marker(), "");
}

fn marker() -> std::path::PathBuf {
    config::dir().join("welcomed")
}

pub fn setup(s: &mut Welcome, waker: Waker) {
    let cache = ImageCache::new(waker);
    let logo = cache.load(crate::LOGO);
    if rune::is_headless() {
        let _ = logo.wait();
    }
    s.logo = Some((cache, logo));
}

/// While the window is open, checks each second whether Accessibility was granted or taken back,
/// so the switch follows System Settings.
pub fn poll(s: &mut Welcome, _: f64, tx: &mut Tx) -> bool {
    if !s.watching {
        return false;
    }
    tx.wake_after(Duration::from_secs(1));
    let trusted = accessibility::trusted();
    std::mem::replace(&mut s.trusted, trusted) != trusted
}

pub fn on_close(s: &mut Welcome, _: &mut Tx) {
    s.watching = false;
}

pub fn view(s: &Welcome, cx: &mut Cx<Welcome>) -> El {
    let k = kit(s.appearance.dark(cx.system_dark()));
    let mut keys = row().gap(4.0).center_y();
    for key in &s.keys {
        keys = keys.child(keycap(&k, key));
    }
    if s.keys.is_empty() {
        keys = keys.child(text("Set one in Settings").font(k.ty.small).color(k.c.text_muted));
    }
    let mut steps = col().w_full().radius(k.radius.lg).border(1.0, k.c.border_muted)
        .child(step(&k, "Open Runecast from anywhere", "Type a few letters, then ↩. Change the hotkey in Settings.", keys));
    if s.macos {
        // Only System Settings can grant or take back the permission, so the switch opens it,
        // and follows what's set there.
        let control = Switch::new(s.trusted).label("Allow Accessibility").hide_label()
            .on_toggle(|_: &mut Welcome| accessibility::request()).build(&k);
        let why = "Lets Runecast paste snippets and clipboard history, and move windows.";
        let detail = if s.trusted { why.to_string() } else { format!("{why} Turn Runecast on in the list that opens.") };
        steps = steps.child(divider(&k)).child(step(&k, "Allow Accessibility", &detail, control));
    }
    let login = Switch::new(s.login).label("Launch at login").hide_label().on_toggle(|s: &mut Welcome| s.toggle_login()).build(&k);
    steps = steps.child(divider(&k)).child(step(&k, "Launch at login", "Start Runecast in the background when you log in.", login));
    let logo = crate::mark(s.logo.as_ref().map(|(_, img)| img), 72.0, k.c.bg_accent);
    let mut body = col().w_full().grow(1.0).px(32.0).pb(28.0).gap(20.0).center_y()
        .child(logo)
        .child(col().gap(6.0).center_y()
            .child(text("Welcome to Runecast").font(rune::TextStyle::new(22.0, 650.0)).color(k.c.text_base))
            .child(text("A launcher that lives in your menu bar.").font(k.ty.ui).color(k.c.text_muted)))
        .child(steps);
    if s.macos {
        body = body.child(text("The first window layout also asks to control System Events. Click OK to allow it.")
            .font(k.ty.small).color(k.c.text_faint).wrap().w_full());
    }
    body = body.child(spacer())
        .child(Button::new("Get Started").variant(ButtonVariant::Submit).on_press(|s: &mut Welcome| s.get_started()).build(&k));
    rune_kit::root(col().w_full().h_full().fill(k.c.page)
        .child(el().key("welcome-drag").w_full().h(TITLEBAR).window_drag())
        .child(body), Vec::new())
}

/// One step: what it is and a line about it on the left, its control on the right.
fn step(k: &Kit, title: &str, detail: &str, control: El) -> El {
    row().w_full().min_h(64.0).px(16.0).py(12.0).gap(16.0).center_y()
        .child(col().grow(1.0).min_w(0.0).gap(3.0)
            .child(text(title.to_string()).font(k.ty.ui_medium).color(k.c.text_base))
            .child(text(detail.to_string()).font(k.ty.small).color(k.c.text_muted).wrap().w_full()))
        .child(control)
}

fn divider(k: &Kit) -> El {
    el().h(1.0).w_full().fill(k.c.border_muted)
}

fn keycap(k: &Kit, label: &str) -> El {
    row().min_w(26.0).h(26.0).px(7.0).radius(6.0).center().fill(k.c.layer_02).border(1.0, k.c.border_muted)
        .child(text(label.to_string()).font(k.ty.ui_medium).color(k.c.text_base))
}

pub const SCENES: &[(&str, fn(&mut Welcome))] = &[
    ("welcome", |s| *s = Welcome::new(None, None)),
    ("welcome-needs-access", |s| {
        *s = Welcome::new(None, None);
        (s.macos, s.trusted, s.keys) = (true, false, vec!["⌥".into(), "Space".into()]);
    }),
    ("welcome-allowed", |s| {
        *s = Welcome::new(None, None);
        (s.macos, s.trusted, s.keys) = (true, true, vec!["⌥".into(), "Space".into()]);
    }),
];

#[cfg(target_os = "macos")]
mod accessibility {
    use std::ffi::c_void;

    use objc2_foundation::{NSDictionary, NSNumber, NSString};

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> u8;
        fn AXIsProcessTrustedWithOptions(options: *const c_void) -> u8;
    }

    /// Whether Runecast may use Accessibility: post keystrokes and move other apps' windows.
    pub fn trusted() -> bool {
        // SAFETY: takes nothing, only reads the process's TCC state.
        unsafe { AXIsProcessTrusted() != 0 }
    }

    /// Opens the Accessibility list in System Settings, first adding Runecast to it (the list
    /// shows only apps that asked) with macOS's own prompt when it isn't allowed yet.
    pub fn request() {
        if trusted() {
            open_list();
            return;
        }
        let key = NSString::from_str("AXTrustedCheckOptionPrompt");
        let options = NSDictionary::from_slices(&[&*key], &[&*NSNumber::new_bool(true)]);
        // SAFETY: NSDictionary is toll-free bridged to the CFDictionary the call reads.
        unsafe { AXIsProcessTrustedWithOptions(&*options as *const _ as *const c_void) };
        open_list();
    }

    fn open_list() {
        let _ = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility").spawn();
    }
}

#[cfg(not(target_os = "macos"))]
mod accessibility {
    /// Only macOS gates keystrokes and window moves behind a permission.
    pub fn trusted() -> bool {
        true
    }

    pub fn request() {}
}
