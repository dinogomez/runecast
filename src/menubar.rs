//! The menu bar icon: an `NSStatusItem` on macOS (Rune's tray doesn't cover macOS yet), Rune's
//! tray on Linux and Windows. Its menu opens the launcher, clipboard history, file search and
//! Settings, and quits; once a newer release is out, it offers that at the top.

use rune::AppHandle;

use crate::{Mode, State};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pick {
    Open,
    Clipboard,
    Files,
    Settings,
    Quit,
    /// Only in the menu once `offer_update` adds it.
    Update,
}

/// The menu, top to bottom; `None` is a separator. The last field is the macOS key equivalent,
/// shown beside the item while the menu is open.
const MENU: [Option<(Pick, &str, &str)>; 7] = [
    Some((Pick::Open, "Open Runecast", "")),
    Some((Pick::Clipboard, "Clipboard History", "")),
    Some((Pick::Files, "Search Files", "")),
    None,
    Some((Pick::Settings, "Settings…", ",")),
    None,
    Some((Pick::Quit, "Quit Runecast", "q")),
];

impl Pick {
    /// The tray's menu id (macOS menus use the tag instead).
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    fn id(self) -> &'static str {
        match self {
            Pick::Open => "open",
            Pick::Clipboard => "clipboard",
            Pick::Files => "files",
            Pick::Settings => "settings",
            Pick::Quit => "quit",
            Pick::Update => "update",
        }
    }

    fn all() -> impl Iterator<Item = Pick> {
        MENU.into_iter().flatten().map(|(p, _, _)| p).chain([Pick::Update])
    }
}

/// What a pick does, on the UI thread.
fn run(launcher: &AppHandle<State>, pick: Pick) {
    match pick {
        Pick::Quit => launcher.quit(),
        Pick::Settings => launcher.update(|s, _| s.show_settings()),
        Pick::Open => launcher.update(|s, _| s.open(Mode::Root)),
        Pick::Clipboard => launcher.update(|s, _| s.open(Mode::Clipboard)),
        Pick::Files => launcher.update(|s, _| s.open(Mode::Files)),
        Pick::Update => launcher.update(|s, _| s.install_update()),
    }
}

pub use native::{install, offer_update};

#[cfg(target_os = "macos")]
mod native {
    use std::cell::RefCell;

    use objc2::rc::Retained;
    use objc2::runtime::NSObjectProtocol;
    use objc2::{AnyThread, DefinedClass, MainThreadOnly, msg_send, sel};
    use objc2_app_kit::{NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem};
    use objc2_foundation::{MainThreadMarker, NSData, NSObject, NSSize, NSString};
    use rune::AppHandle;

    use super::{MENU, Pick, run};
    use crate::State;

    /// A template image: macOS tints it to match the menu bar.
    const ICON: &[u8] = include_bytes!("../assets/menubar@2x.png");

    struct Ivars {
        launcher: AppHandle<State>,
    }

    objc2::define_class!(
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        #[ivars = Ivars]
        struct Target;

        unsafe impl NSObjectProtocol for Target {}

        impl Target {
            #[unsafe(method(pick:))]
            fn pick(&self, sender: &NSMenuItem) {
                if let Some(pick) = Pick::all().find(|p| *p as isize == sender.tag()) {
                    run(&self.ivars().launcher, pick);
                }
            }
        }
    );

    thread_local! {
        /// The status item, its menu and the menu's target live as long as the app.
        static ITEM: RefCell<Option<(Retained<NSStatusItem>, Retained<NSMenu>, Retained<Target>)>> = const { RefCell::new(None) };
    }

    /// Adds the icon to the menu bar. Call on the UI thread once the app runs.
    pub fn install(launcher: AppHandle<State>) -> Result<(), String> {
        let mtm = MainThreadMarker::new().ok_or("the menu bar icon needs the main thread")?;
        let this = Target::alloc(mtm).set_ivars(Ivars { launcher });
        let target: Retained<Target> = unsafe { msg_send![super(this), init] };
        let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Runecast"));
        for entry in MENU {
            match entry {
                Some((pick, title, key)) => menu.addItem(&menu_item(mtm, &target, pick, title, key)),
                None => menu.addItem(&NSMenuItem::separatorItem(mtm)),
            }
        }
        let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(ICON)).ok_or("couldn't decode the menu bar icon")?;
        image.setSize(NSSize::new(18.0, 18.0));
        image.setTemplate(true);
        image.setAccessibilityDescription(Some(&NSString::from_str("Runecast")));
        // NSVariableStatusItemLength: as wide as the icon.
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(-1.0);
        item.button(mtm).ok_or("the status item has no button")?.setImage(Some(&image));
        item.setMenu(Some(&menu));
        ITEM.with(|slot| *slot.borrow_mut() = Some((item, menu, target)));
        Ok(())
    }

    /// Puts `label` at the top of the menu for installing an update. Call on the UI thread.
    pub fn offer_update(label: &str) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        ITEM.with(|slot| {
            let Some((_, menu, target)) = &*slot.borrow() else { return };
            if let Some(item) = menu.itemWithTag(Pick::Update as isize) {
                item.setTitle(&NSString::from_str(label));
                return;
            }
            menu.insertItem_atIndex(&NSMenuItem::separatorItem(mtm), 0);
            menu.insertItem_atIndex(&menu_item(mtm, target, Pick::Update, label, ""), 0);
        });
    }

    fn menu_item(mtm: MainThreadMarker, target: &Target, pick: Pick, title: &str, key: &str) -> Retained<NSMenuItem> {
        // SAFETY: `pick:` is a method of `target`, which lives in ITEM as long as the menu.
        let item = unsafe {
            let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm), &NSString::from_str(title), Some(sel!(pick:)), &NSString::from_str(key),
            );
            item.setTarget(Some(target));
            item
        };
        item.setTag(pick as isize);
        item
    }
}

#[cfg(not(target_os = "macos"))]
mod native {
    use std::cell::RefCell;

    use rune::{AppHandle, Tray, TrayEvent, TrayIcon, TrayItem, TraySpec};

    use super::{MENU, Pick, run};
    use crate::State;

    const ICON: &[u8] = include_bytes!("../assets/tray.png");

    thread_local! {
        /// Dropping the tray removes the icon.
        static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
    }

    /// Adds the icon to the system tray. A click opens the launcher; the menu has the rest.
    pub fn install(launcher: AppHandle<State>) -> Result<(), String> {
        let spec = TraySpec::new("dev.runecast.launcher", "Runecast", icon()?).tooltip("Runecast").menu(menu(None));
        let tray = Tray::new(spec, move |event| match event {
            TrayEvent::Activate { .. } => run(&launcher, Pick::Open),
            TrayEvent::Menu(id) => {
                if let Some(pick) = Pick::all().find(|p| p.id() == id) {
                    run(&launcher, pick);
                }
            }
            _ => {}
        }).map_err(|e| e.to_string())?;
        TRAY.with(|slot| *slot.borrow_mut() = Some(tray));
        Ok(())
    }

    /// Puts `label` at the top of the menu for getting an update. Call on the UI thread.
    pub fn offer_update(label: &str) {
        TRAY.with(|slot| if let Some(tray) = &*slot.borrow() {
            tray.set_menu(menu(Some(label)));
        });
    }

    fn menu(update: Option<&str>) -> Vec<TrayItem> {
        let update = update.into_iter().flat_map(|label| [TrayItem::action(Pick::Update.id(), label), TrayItem::Separator]);
        update.chain(MENU.iter().map(|entry| match entry {
            Some((pick, title, _)) => TrayItem::action(pick.id(), title.trim_end_matches('…')),
            None => TrayItem::Separator,
        })).collect()
    }

    fn icon() -> Result<TrayIcon, String> {
        let mut decoder = png::Decoder::new(std::io::Cursor::new(ICON));
        decoder.set_transformations(png::Transformations::EXPAND);
        let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
        let mut rgba = vec![0; reader.output_buffer_size().ok_or("tray icon too large")?];
        let frame = reader.next_frame(&mut rgba).map_err(|e| e.to_string())?;
        rgba.truncate(frame.buffer_size());
        Ok(TrayIcon::rgba(frame.width, frame.height, rgba))
    }
}

#[cfg(test)]
mod tests {
    use super::Pick;

    #[test]
    fn every_pick_is_in_the_menu_once_with_its_own_id() {
        let ids: Vec<&str> = Pick::all().map(Pick::id).collect();
        let unique: std::collections::HashSet<&&str> = ids.iter().collect();
        assert_eq!(ids.len(), 6);
        assert_eq!(unique.len(), ids.len());
    }
}
