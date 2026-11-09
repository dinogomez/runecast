//! Launch at login: a LaunchAgent on macOS, an XDG autostart entry on Linux, both starting this
//! executable. Whether the file exists is the setting, so there's nothing to keep in sync.

use std::path::PathBuf;

fn path() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    if cfg!(target_os = "macos") {
        home.join("Library/LaunchAgents/dev.runecast.launcher.plist")
    } else {
        let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or(home.join(".config"));
        config.join("autostart/runecast.desktop")
    }
}

pub fn enabled() -> bool {
    path().exists()
}

pub fn set(on: bool) -> std::io::Result<()> {
    let path = path();
    if !on {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    let exe = std::env::current_exe()?;
    let exe = exe.display();
    let body = if cfg!(target_os = "macos") {
        format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>dev.runecast.launcher</string>
  <key>ProgramArguments</key><array><string>{exe}</string></array>
  <key>RunAtLoad</key><true/>
</dict>
</plist>
"#)
    } else {
        format!("[Desktop Entry]\nType=Application\nName=Runecast\nExec=\"{exe}\"\nX-GNOME-Autostart-enabled=true\n")
    };
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, body)
}
