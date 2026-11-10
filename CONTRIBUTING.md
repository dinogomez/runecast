# Contributing to Runecast

## Run from source

Requires Rust. `rustup` installs the toolchain pinned in `rust-toolchain.toml`.

```sh
cargo run --release
```

Runecast starts in the background with no Dock icon. Press <kbd>⌥ Option</kbd> + <kbd>Space</kbd> to open it.

- **macOS:** give Runecast (or the terminal you run it from) **Accessibility** permission in System Settings → Privacy & Security. Pasting and moving windows need it.
- **Linux:** install `libxkbcommon-dev libwayland-dev` first (see Rune's README). Pasting uses `xdotool` or `wtype` if one is installed; otherwise <kbd>Return</kbd> only copies.

Rune is pre-release, so `Cargo.toml` pins a commit of Rune's `dev` branch.

## Tests

```sh
cargo test --locked
```

CI runs this on macOS and Linux for every push to `dev` and `main` and every pull request.

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
| `src/welcome.rs` | The Welcome window shown on first launch |
| `src/catalog.rs` | Apps, commands, snippets, quicklinks, web search and how each runs |
| `src/hotkeys.rs` | Global hotkeys and their registration |
| `src/clipboard.rs` | Clipboard watcher and history |
| `src/wm.rs` | Window layouts |
| `src/files.rs` | File search |
| `src/usage.rs` | Usage-based ranking |
| `src/fuzzy.rs` | Fuzzy ranking |
| `src/calc.rs` | The calculator |
| `src/convert.rs` | Unit, currency and time zone conversion |
| `src/ai.rs` | Claude and ChatGPT plan usage |
| `src/config.rs` | `config.json` |
| `src/menubar.rs` | The menu bar icon |
| `src/login.rs` | Launch at login |
| `src/update.rs` | Checking for and installing new releases |

## Branches and releases

- **`dev`** is the default branch. Work lands here, through pull requests or straight pushes.
- **`main`** is staging. Open a pull request from `dev` to `main` when `dev` is ready to ship, and merge it once CI passes.
- **Releases** come from `main`. Run the **Promote** workflow on `main` from the Actions tab. It picks the next version from the commits since the last release, commits it to `Cargo.toml`, tags `vX.Y.Z`, merges `main` back into `dev` and runs **Release**.
- **Release** builds the Mac app and Linux tarball, sends the Mac app to Apple for notarization and publishes right away. Until Apple accepts the build, the release notes say macOS will warn on first open.
- **Staple** checks Apple every 30 minutes. Once a build is accepted, it staples the ticket, replaces the zip and DMG, and updates the notes. A rejection fails the workflow and says so in the notes.

## Commit messages

Commits follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/): `<type>(<optional scope>): <description>`, such as `feat(ai): show Gemini usage` or `fix: keep the panel on the active screen`. CI checks new commits and pull request titles.

| Type | Use for | Release |
|---|---|---|
| `feat` | A new feature | Minor |
| `fix`, `perf` | A bug fix or speedup | Patch |
| `refactor`, `docs`, `test`, `build`, `ci`, `chore`, `style`, `revert` | Everything else | None |

Add `!` before the colon (`feat!: …`), or a `BREAKING CHANGE:` line in the body, for a breaking change. Before 1.0 that bumps the minor version, not the major. Promote's release notes list features, fixes and breaking changes from these subjects.

## Ideas

- Snippet keywords that expand as you type in any app
- Image entries in clipboard history
- Extensions
