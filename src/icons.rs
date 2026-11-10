//! The symbol each kind of row shows when it has no icon of its own: a Lucide icon
//! (https://lucide.dev, ISC licence) drawn white on a tile of one of the theme's accent hues.

use rune::Icon;

use crate::catalog::Kind;

/// Lucide's shared stroke style, which its icons' elements inherit.
macro_rules! lucide {
    ($name:literal, $body:literal) => {
        Icon {
            name: concat!("lucide-", $name),
            view: 24.0,
            body: concat!(
                r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
                $body,
                "</g>"
            ),
        }
    };
}

static GLOBE: Icon = lucide!(
    "globe",
    r#"<circle cx="12" cy="12" r="10"/><path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20"/><path d="M2 12h20"/>"#
);
static CALCULATOR: Icon = lucide!(
    "calculator",
    r#"<rect width="16" height="20" x="4" y="2" rx="2"/><line x1="8" x2="16" y1="6" y2="6"/><line x1="16" x2="16" y1="14" y2="18"/><path d="M16 10h.01"/><path d="M12 10h.01"/><path d="M8 10h.01"/><path d="M12 14h.01"/><path d="M8 14h.01"/><path d="M12 18h.01"/><path d="M8 18h.01"/>"#
);
static COMMAND: Icon = lucide!(
    "command",
    r#"<path d="M15 6v12a3 3 0 1 0 3-3H6a3 3 0 1 0 3 3V6a3 3 0 1 0-3 3h12a3 3 0 1 0-3-3"/>"#
);
static SNIPPET: Icon = lucide!("text-quote", r#"<path d="M17 5H3"/><path d="M21 12H8"/><path d="M21 19H8"/><path d="M3 12v7"/>"#);
static LINK: Icon = lucide!(
    "link",
    r#"<path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/>"#
);
static WINDOW: Icon = lucide!(
    "app-window",
    r#"<rect x="2" y="4" width="20" height="16" rx="2"/><path d="M10 4v4"/><path d="M2 8h20"/><path d="M6 4v4"/>"#
);
static CLIPBOARD: Icon = lucide!(
    "clipboard",
    r#"<rect width="8" height="4" x="8" y="2" rx="1" ry="1"/><path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2"/>"#
);
static FILE: Icon = lucide!(
    "file-text",
    r#"<path d="M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z"/><path d="M14 2v5a1 1 0 0 0 1 1h5"/><path d="M10 9H8"/><path d="M16 13H8"/><path d="M16 17H8"/>"#
);
static CLOCK: Icon = lucide!("clock", r#"<circle cx="12" cy="12" r="10"/><path d="M12 6v6l4 2"/>"#);
static GAUGE: Icon = lucide!("gauge", r#"<path d="m12 14 4-4"/><path d="M3.34 19a10 10 0 1 1 17.32 0"/>"#);

/// The symbol for `kind` and the `tok::accent` hue of its tile; `None` for apps, which show their
/// own icon or initial.
pub fn for_kind(kind: Kind) -> Option<(&'static Icon, usize)> {
    Some(match kind {
        Kind::App => return None,
        Kind::Web => (&GLOBE, 0),
        Kind::File => (&FILE, 0),
        Kind::Window => (&WINDOW, 1),
        Kind::Usage => (&GAUGE, 1),
        Kind::Clock => (&CLOCK, 4),
        Kind::Snippet => (&SNIPPET, 2),
        Kind::Calculator => (&CALCULATOR, 3),
        Kind::Quicklink => (&LINK, 4),
        Kind::Command => (&COMMAND, 5),
        Kind::Clip => (&CLIPBOARD, 5),
    })
}
