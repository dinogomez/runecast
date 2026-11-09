# Runecast

A Raycast-style launcher for macOS (and Linux), built with [Rune](https://github.com/anomalyco/rune), Anomaly's native GPU-rendered Rust UI framework.

Press **⌥Space** (Alt+Space on Linux) from anywhere, type a few letters, hit **↩**.

![Searching apps](docs/apps.png)

## What it does

- **Fuzzy app search with real icons.** Finds `.app` bundles in `/Applications`, `/System/Applications` and `~/Applications` on macOS (drawn with their Finder icons), and `.desktop` entries on Linux (with their theme icons). Word starts and prefixes rank first, so `vsc` finds Visual Studio Code. Apps are rescanned each time the panel opens.
- **Clipboard history.** **⌃⌥V**, or the Clipboard History command, lists the last 200 texts you copied, newest first, with a preview. Type to filter; ↩ pastes into the app you were in.
- **Window management** (macOS). Halves, thirds, two thirds, Maximize and Center for the front window, on the screen it's on. Search for them, or use **⌃⌥←**, **⌃⌥→**, **⌃⌥↩** and **⌃⌥C**.
- **Snippets.** Saved text you search by name; ↩ pastes it.
- **Quicklinks.** Saved URLs. A URL with `{query}` takes an argument after its keyword: `gh rune` searches GitHub for "rune". Picking one with nothing typed fills in its keyword for you.
- **Inline calculator.** Type `(1200+340)*1.12` and the answer is the first row; ↩ copies it.
- **System commands.** Lock Screen, Sleep, Empty Trash, Toggle Dark Mode, Open Downloads.
- **Web search fallback.** The last row always searches Google for what you typed.
- **Keyboard first.** ↑/↓ to move, ↩ to run, esc clears the query, then goes back or closes.

| Clipboard history | Window management |
|---|---|
| ![Clipboard history](docs/clipboard.png) | ![Window management](docs/windows.png) |
| **Quicklinks** | **Snippets** |
| ![Quicklink](docs/quicklink.png) | ![Snippets](docs/snippets.png) |
| **Calculator** | **Commands** |
| ![Calculator](docs/calc.png) | ![Commands](docs/commands.png) |

## Snippets and quicklinks

They live in `config.json`, which the first run writes with examples:

- macOS: `~/Library/Application Support/Runecast/config.json`
- Linux: `~/.config/runecast/config.json`

The "Edit Snippets and Quicklinks" command opens it. Changes apply the next time the panel opens.

```json
{
  "snippets": [
    { "name": "Email Signature", "text": "Best,\nDino" }
  ],
  "quicklinks": [
    { "name": "Search GitHub", "keyword": "gh", "url": "https://github.com/search?q={query}" },
    { "name": "Runecast Repo", "url": "https://github.com/dinogomez/runecast" }
  ]
}
```

Clipboard history is saved beside it in `clipboard.json`, readable only by you. It records every text you copy, passwords included, so clear the file if that matters to you.

## Run

Requires Rust (the toolchain is pinned in `rust-toolchain.toml`).

```sh
cargo run --release
```

Runecast starts in the background with no Dock icon. Press ⌥Space to open it.

On macOS, give Runecast (or the terminal you run it from) **Accessibility** permission in System Settings → Privacy & Security. Pasting from clipboard history and snippets, and moving windows, need it.

On Linux, install `libxkbcommon-dev libwayland-dev` first (see Rune's README). Pasting uses `xdotool` or `wtype` if one is installed; otherwise ↩ only copies.

## Headless screenshots

Rune apps can render without a window, which is how the screenshots above were made:

```sh
target/release/runecast --list-scenes
target/release/runecast --snap docs/clipboard.png --scene clipboard --scale 2 --theme dark
target/release/runecast --snap icons.png --scene installed     # this machine's apps and icons
target/release/runecast --tree tree.json --scene calc           # dump the element tree
```

## Layout

- `src/main.rs`: the window, hotkeys and view
- `src/catalog.rs`: apps, commands, snippets, quicklinks, web search and how each runs
- `src/clipboard.rs`: clipboard watcher and history
- `src/wm.rs`: window layouts
- `src/config.rs`: `config.json`
- `src/fuzzy.rs`: fuzzy ranking
- `src/calc.rs`: the calculator

## Roadmap

- Snippet keywords that expand as you type in any app
- Image entries in clipboard history
- Configurable hotkeys
- Extensions

Rune is pre-release, so Runecast pins a commit of Rune's `dev` branch.

MIT
