# Contributing to Runecast

## Run from source

Requires Rust. `rustup` installs the toolchain pinned in `rust-toolchain.toml`.

```sh
cargo run --release
```

Runecast starts in the background with no Dock icon. Press ⌥Space to open it.

- **macOS:** give Runecast (or the terminal you run it from) **Accessibility** permission in System Settings → Privacy & Security. Pasting and moving windows need it.
- **Linux:** install `libxkbcommon-dev libwayland-dev` first (see Rune's README). Pasting uses `xdotool` or `wtype` if one is installed; otherwise ↩ only copies.

Rune is pre-release, so `Cargo.toml` pins a commit of Rune's `dev` branch.

## Tests

```sh
cargo test --locked
```

CI runs this on macOS and Linux for every push to `main` and every pull request.

## Screenshots

Rune apps can render without a window, which is how the screenshots in `docs/` are made:

```sh
target/release/runecast --list-scenes
target/release/runecast --snap docs/clipboard.png --scene clipboard --scale 2 --theme dark
target/release/runecast --window settings --snap docs/settings-hotkeys.png --scene hotkeys --scale 2 --theme dark
target/release/runecast --snap icons.png --scene installed     # this machine's apps and icons
target/release/runecast --tree tree.json --scene calc           # dump the element tree
```

## Code layout

| File | What it owns |
|---|---|
| `src/main.rs` | The launcher window and view |
| `src/settings.rs` | The Settings window |
| `src/catalog.rs` | Apps, commands, snippets, quicklinks, web search and how each runs |
| `src/hotkeys.rs` | Global hotkeys and their registration |
| `src/clipboard.rs` | Clipboard watcher and history |
| `src/wm.rs` | Window layouts |
| `src/files.rs` | File search |
| `src/usage.rs` | Usage-based ranking |
| `src/fuzzy.rs` | Fuzzy ranking |
| `src/calc.rs` | The calculator |
| `src/convert.rs` | Unit, currency and time zone conversion |
| `src/config.rs` | `config.json` |
| `src/menubar.rs` | The menu bar icon |
| `src/login.rs` | Launch at login |

## Ideas

- Snippet keywords that expand as you type in any app
- Image entries in clipboard history
- Extensions
