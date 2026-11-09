//! File search by name: Spotlight's index (`mdfind`) on macOS, a bounded walk of the home folder
//! elsewhere. Runs on a `rune::Latest` worker, never on the UI thread.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::catalog::{Action, Item, Kind};

/// Shortest query that searches files, so one or two letters don't flood the list.
pub const MIN_QUERY: usize = 2;
pub const MAX_HITS: usize = 40;
/// Folders the walk skips: dependencies, build output and caches nobody searches by name.
const SKIP: [&str; 6] = ["node_modules", "target", "Library", "__pycache__", "venv", "dist"];

/// Up to `MAX_HITS` files and folders in the home folder whose names contain `query`.
pub fn search(query: &str) -> Vec<PathBuf> {
    let query = query.trim();
    if query.chars().count() < MIN_QUERY {
        return Vec::new();
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    if cfg!(target_os = "macos") {
        if let Some(hits) = spotlight(&home, query) {
            return hits;
        }
    }
    walk(&home, query, 6, 50_000)
}

/// `mdfind -name`, reading only the first hits and stopping the search there.
fn spotlight(home: &Path, query: &str) -> Option<Vec<PathBuf>> {
    let mut child = Command::new("mdfind").arg("-onlyin").arg(home).arg("-name").arg(query)
        .stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let lines = BufReader::new(child.stdout.take()?).lines().map_while(Result::ok);
    let mut hits: Vec<PathBuf> = lines.filter(|l| !is_hidden(Path::new(l))).take(MAX_HITS).map(PathBuf::from).collect();
    let _ = child.kill();
    let _ = child.wait();
    rank(&mut hits, query);
    Some(hits)
}

/// A breadth-first walk of `root`, `depth` folders deep and `budget` entries at most, skipping
/// hidden entries and `SKIP` folders. Names match ignoring case.
pub fn walk(root: &Path, query: &str, depth: usize, budget: usize) -> Vec<PathBuf> {
    let needle = query.to_lowercase();
    let (mut hits, mut seen) = (Vec::new(), 0);
    let mut level = vec![root.to_path_buf()];
    for _ in 0..depth {
        let mut next = Vec::new();
        for dir in level {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for entry in entries.flatten() {
                seen += 1;
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                if name.to_lowercase().contains(&needle) {
                    hits.push(entry.path());
                }
                if entry.file_type().is_ok_and(|t| t.is_dir()) && !SKIP.contains(&name.as_str()) {
                    next.push(entry.path());
                }
                if hits.len() >= MAX_HITS * 4 || seen >= budget {
                    rank(&mut hits, query);
                    return hits;
                }
            }
        }
        level = next;
    }
    rank(&mut hits, query);
    hits
}

/// Names that start with the query first, then shorter paths.
fn rank(hits: &mut Vec<PathBuf>, query: &str) {
    let needle = query.to_lowercase();
    let starts = |p: &PathBuf| !name(p).to_lowercase().starts_with(&needle);
    hits.sort_by_key(|p| (starts(p), p.components().count(), p.as_os_str().len()));
    hits.truncate(MAX_HITS);
}

fn is_hidden(path: &Path) -> bool {
    path.components().any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
}

fn name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// A row for `path`: its name, and its folder with the home folder as `~`. macOS draws
/// Finder's icon for it.
pub fn item(path: &Path) -> Item {
    let home = std::env::var("HOME").unwrap_or_default();
    let folder = path.parent().map(|p| p.display().to_string()).unwrap_or_default();
    let folder = match folder.strip_prefix(&home) {
        Some(rest) if !home.is_empty() => format!("~{rest}"),
        _ => folder,
    };
    let icon = cfg!(target_os = "macos").then(|| path.to_path_buf());
    Item { icon, ..Item::new(name(path), folder, Kind::File, Action::OpenPath(path.to_path_buf())) }
}

/// Shows `path` in Finder (or opens its folder elsewhere).
pub fn reveal(path: &Path) {
    let result = if cfg!(target_os = "macos") {
        Command::new("open").arg("-R").arg(path).spawn()
    } else {
        Command::new("xdg-open").arg(path.parent().unwrap_or(path)).spawn()
    };
    if let Err(e) = result {
        eprintln!("runecast: couldn't reveal {}: {e}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_finds_names_skips_hidden_and_build_folders_and_ranks_prefixes_first() {
        let root = std::env::temp_dir().join(format!("runecast-files-{}", std::process::id()));
        for dir in ["docs/deep", ".secret", "node_modules/pkg"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        for file in ["docs/deep/Report Q3.pdf", "docs/old-report.txt", ".secret/report.key", "node_modules/pkg/report.js", "reports"] {
            std::fs::write(root.join(file), "").unwrap();
        }
        let hits: Vec<String> = walk(&root, "REPORT", 6, 10_000).iter().map(|p| name(p)).collect();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(hits, ["reports", "Report Q3.pdf", "old-report.txt"]);
    }
}
