# Runecast

A Raycast-style launcher for macOS (and Linux), built with [Rune](https://github.com/anomalyco/rune), Anomaly's native GPU-rendered Rust UI framework.

Press **⌥Space** (Alt+Space on Linux) from anywhere, type a few letters, hit **↩**.

![Searching apps](docs/apps.png)

## What it does

- **Fuzzy app search.** Finds `.app` bundles in `/Applications`, `/System/Applications` and `~/Applications` on macOS, and `.desktop` entries on Linux. Word starts and prefixes rank first, so `vsc` finds Visual Studio Code.
- **Inline calculator.** Type `(1200+340)*1.12` and the answer is the first row; ↩ copies it.
- **System commands.** Lock Screen, Sleep, Empty Trash, Toggle Dark Mode, Open Downloads.
- **Web search fallback.** The last row always searches Google for what you typed.
- **Keyboard first.** ↑/↓ to move, ↩ to run, esc clears the query, then closes. Clicking outside closes the panel too.

| Calculator | Commands |
|---|---|
| ![Calculator](docs/calc.png) | ![Commands](docs/commands.png) |

## Run

Requires Rust (the toolchain is pinned in `rust-toolchain.toml`).

```sh
cargo run --release
```

Runecast starts in the background with no Dock icon. Press ⌥Space to open it. On macOS, the first press may ask for Accessibility permission so the global hotkey works.

On Linux, install `libxkbcommon-dev libwayland-dev` first (see Rune's README).

## Headless screenshots

Rune apps can render without a window, which is how the screenshots above were made:

```sh
target/release/runecast --list-scenes
target/release/runecast --snap docs/apps.png --scene apps --scale 2 --theme dark
target/release/runecast --tree tree.json --scene calc     # dump the element tree
```

## Layout

- `src/main.rs` — the window, hotkey and view
- `src/catalog.rs` — apps, commands, web search and how each runs
- `src/fuzzy.rs` — fuzzy ranking
- `src/calc.rs` — the calculator

## Roadmap

- Clipboard history
- Window management (halves, thirds, maximize)
- Snippets and quicklinks
- Real app icons
- Extensions

Rune is pre-release, so Runecast pins a commit of Rune's `dev` branch.

MIT
