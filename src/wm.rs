//! Window management on macOS: moves the frontmost app's front window to a part of the screen
//! it's on (the visible frame, so the menu bar and Dock stay clear) through System Events.
//! Needs Accessibility permission for Runecast.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Layout {
    LeftHalf,
    RightHalf,
    TopHalf,
    BottomHalf,
    LeftThird,
    CenterThird,
    RightThird,
    LeftTwoThirds,
    RightTwoThirds,
    Maximize,
    Center,
}

impl Layout {
    pub const ALL: [Layout; 11] = [
        Layout::LeftHalf, Layout::RightHalf, Layout::TopHalf, Layout::BottomHalf, Layout::LeftThird, Layout::CenterThird,
        Layout::RightThird, Layout::LeftTwoThirds, Layout::RightTwoThirds, Layout::Maximize, Layout::Center,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Layout::LeftHalf => "Left Half",
            Layout::RightHalf => "Right Half",
            Layout::TopHalf => "Top Half",
            Layout::BottomHalf => "Bottom Half",
            Layout::LeftThird => "Left Third",
            Layout::CenterThird => "Center Third",
            Layout::RightThird => "Right Third",
            Layout::LeftTwoThirds => "Left Two Thirds",
            Layout::RightTwoThirds => "Right Two Thirds",
            Layout::Maximize => "Maximize",
            Layout::Center => "Center",
        }
    }

    /// x, y, width and height as fractions of the visible frame; `None` keeps the size (Center).
    fn frame(self) -> Option<(f64, f64, f64, f64)> {
        let t = 1.0 / 3.0;
        Some(match self {
            Layout::LeftHalf => (0.0, 0.0, 0.5, 1.0),
            Layout::RightHalf => (0.5, 0.0, 0.5, 1.0),
            Layout::TopHalf => (0.0, 0.0, 1.0, 0.5),
            Layout::BottomHalf => (0.0, 0.5, 1.0, 0.5),
            Layout::LeftThird => (0.0, 0.0, t, 1.0),
            Layout::CenterThird => (t, 0.0, t, 1.0),
            Layout::RightThird => (2.0 * t, 0.0, t, 1.0),
            Layout::LeftTwoThirds => (0.0, 0.0, 2.0 * t, 1.0),
            Layout::RightTwoThirds => (t, 0.0, 2.0 * t, 1.0),
            Layout::Maximize => (0.0, 0.0, 1.0, 1.0),
            Layout::Center => return None,
        })
    }
}

/// Runs the move in the background; does nothing off macOS.
pub fn apply(layout: Layout) {
    if !cfg!(target_os = "macos") {
        return;
    }
    if let Err(e) = std::process::Command::new("osascript").args(["-l", "JavaScript", "-e", &script(layout)]).spawn() {
        eprintln!("runecast: couldn't move the window: {e}");
    }
}

/// JXA: System Events measures from the top-left of the primary screen, AppKit from its
/// bottom-left, so the visible frame is flipped before it's used.
fn script(layout: Layout) -> String {
    let place = match layout.frame() {
        Some((fx, fy, fw, fh)) => format!(
            "const x = X + {fx} * W, y = Y + {fy} * H, w = {fw} * W, h = {fh} * H;"
        ),
        None => "const w = pw, h = ph, x = X + (W - pw) / 2, y = Y + (H - ph) / 2;".into(),
    };
    format!(r#"ObjC.import('AppKit');
const win = Application('System Events').processes.whose({{frontmost: true}})[0].windows[0];
const [px, py] = win.position(), [pw, ph] = win.size();
const screens = $.NSScreen.screens, top = screens.objectAtIndex(0).frame.size.height;
const cx = px + pw / 2, cy = top - (py + ph / 2);
let vf = screens.objectAtIndex(0).visibleFrame;
for (let i = 0; i < screens.count; i++) {{
  const f = screens.objectAtIndex(i).frame;
  if (cx >= f.origin.x && cx < f.origin.x + f.size.width && cy >= f.origin.y && cy < f.origin.y + f.size.height) vf = screens.objectAtIndex(i).visibleFrame;
}}
const X = vf.origin.x, Y = top - vf.origin.y - vf.size.height, W = vf.size.width, H = vf.size.height;
{place}
win.position = [Math.round(x), Math.round(y)];
win.size = [Math.round(w), Math.round(h)];
win.position = [Math.round(x), Math.round(y)];
"#)
}
