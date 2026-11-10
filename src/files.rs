//! File search by name: Spotlight's index (`mdfind`) on macOS, a bounded walk of the home folder
//! elsewhere. Runs on a `rune::Latest` worker, never on the UI thread.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::catalog::{argument, Action, Item, Kind};

/// Shortest query that searches files, so one or two letters don't flood the list.
pub const MIN_QUERY: usize = 2;
pub const MAX_HITS: usize = 40;
/// Folders the walk skips: dependencies, build output and caches nobody searches by name.
const SKIP: [&str; 6] = ["node_modules", "target", "Library", "__pycache__", "venv", "dist"];

/// Which entries a search keeps: "files report" searches only files, "folders report" only folders,
/// and "images", "docs", "videos" or "audio" only files with those extensions.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Scope {
    Any,
    Files,
    Folders,
    Images,
    Docs,
    Videos,
    Audio,
}

const KEYWORDS: [(&str, Scope); 6] = [
    ("files", Scope::Files), ("folders", Scope::Folders), ("images", Scope::Images), ("docs", Scope::Docs), ("videos", Scope::Videos),
    ("audio", Scope::Audio),
];
const IMAGES: [&str; 16] = ["png", "jpg", "jpeg", "gif", "heic", "heif", "webp", "avif", "svg", "bmp", "tif", "tiff", "ico", "psd", "raw", "dng"];
const DOCS: [&str; 17] = ["pdf", "doc", "docx", "txt", "md", "rtf", "odt", "pages", "epub", "xls", "xlsx", "csv", "numbers", "ods", "ppt", "pptx", "key"];
const VIDEOS: [&str; 11] = ["mp4", "m4v", "mov", "mkv", "avi", "webm", "wmv", "flv", "mpg", "mpeg", "3gp"];
const AUDIO: [&str; 9] = ["mp3", "m4a", "wav", "flac", "aac", "ogg", "aiff", "aif", "opus"];

impl Scope {
    /// The scope a leading keyword asks for, and the name to search after it.
    pub fn of(query: &str) -> (Scope, &str) {
        KEYWORDS.iter().find_map(|&(keyword, scope)| argument(query, keyword).map(|rest| (scope, rest))).unwrap_or((Scope::Any, query))
    }

    fn keeps(self, path: &Path, is_dir: bool) -> bool {
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let typed = |list: &[&str]| !is_dir && list.contains(&ext.as_str());
        match self {
            Scope::Any => true,
            Scope::Files => !is_dir,
            Scope::Folders => is_dir,
            Scope::Images => typed(&IMAGES),
            Scope::Docs => typed(&DOCS),
            Scope::Videos => typed(&VIDEOS),
            Scope::Audio => typed(&AUDIO),
        }
    }
}

/// Up to `MAX_HITS` files and folders in the home folder whose names contain the query, scoped by
/// a leading keyword such as `files` or `images`.
pub fn search(query: &str) -> Vec<PathBuf> {
    let (scope, query) = Scope::of(query.trim());
    if query.chars().count() < MIN_QUERY {
        return Vec::new();
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    if cfg!(target_os = "macos") {
        if let Some(hits) = spotlight(&home, query, scope) {
            return hits;
        }
    }
    walk(&home, query, scope, 6, 50_000)
}

/// `mdfind -name`, reading only the first hits and stopping the search there.
fn spotlight(home: &Path, query: &str, scope: Scope) -> Option<Vec<PathBuf>> {
    let mut child = Command::new("mdfind").arg("-onlyin").arg(home).arg("-name").arg(query)
        .stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let lines = BufReader::new(child.stdout.take()?).lines().map_while(Result::ok);
    let mut hits: Vec<PathBuf> = lines.filter(|l| !is_hidden(Path::new(l)) && scope.keeps(Path::new(l), Path::new(l).is_dir())).take(MAX_HITS).map(PathBuf::from).collect();
    let _ = child.kill();
    let _ = child.wait();
    rank(&mut hits, query);
    Some(hits)
}

/// A breadth-first walk of `root`, `depth` folders deep and `budget` entries at most, skipping
/// hidden entries and `SKIP` folders. Names match ignoring case.
pub fn walk(root: &Path, query: &str, scope: Scope, depth: usize, budget: usize) -> Vec<PathBuf> {
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
                let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
                if scope.keeps(&entry.path(), is_dir) && name.to_lowercase().contains(&needle) {
                    hits.push(entry.path());
                }
                if is_dir && !SKIP.contains(&name.as_str()) {
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
        for dir in ["docs/deep", "docs/report-archive", ".secret", "node_modules/pkg"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        for file in ["docs/deep/Report Q3.pdf", "docs/old-report.txt", ".secret/report.key", "node_modules/pkg/report.js", "reports"] {
            std::fs::write(root.join(file), "").unwrap();
        }
        let hits = |scope| walk(&root, "REPORT", scope, 6, 10_000).iter().map(|p| name(p)).collect::<Vec<_>>();
        let (any, files, folders) = (hits(Scope::Any), hits(Scope::Files), hits(Scope::Folders));
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(any, ["reports", "report-archive", "Report Q3.pdf", "old-report.txt"]);
        assert_eq!(files, ["reports", "Report Q3.pdf", "old-report.txt"]);
        assert_eq!(folders, ["report-archive"]);
    }

    #[test]
    fn a_leading_keyword_and_space_sets_the_scope() {
        assert_eq!(Scope::of("Files q3 report"), (Scope::Files, "q3 report"));
        assert_eq!(Scope::of("folders  docs"), (Scope::Folders, "docs"));
        assert_eq!(Scope::of("images beach"), (Scope::Images, "beach"));
        assert_eq!(Scope::of("docs q3"), (Scope::Docs, "q3"));
        assert_eq!(Scope::of("videos trip"), (Scope::Videos, "trip"));
        assert_eq!(Scope::of("audio podcast"), (Scope::Audio, "podcast"));
        // A bare keyword, or one that's only the start of a word, is an ordinary search.
        assert_eq!(Scope::of("files"), (Scope::Any, "files"));
        assert_eq!(Scope::of("filesystem"), (Scope::Any, "filesystem"));
    }

    #[test]
    fn type_scopes_keep_files_by_extension_only() {
        let root = std::env::temp_dir().join(format!("runecast-types-{}", std::process::id()));
        // A folder with an image extension, and a name that says png without the extension.
        std::fs::create_dir_all(root.join("trip.jpg")).unwrap();
        for file in ["trip-photo.PNG", "trip-scan.jpeg", "trip-notes.md", "trip-budget.xlsx", "trip-clip.MOV", "trip-png", "trip-song.mp3", "trip-memo.M4A", "trip-mp3.txt", "trip-voice.opus"] {
            std::fs::write(root.join(file), "").unwrap();
        }
        let hits = |scope| {
            let mut names: Vec<String> = walk(&root, "trip", scope, 6, 10_000).iter().map(|p| name(p)).collect();
            names.sort();
            names
        };
        let (images, docs, videos, audio) = (hits(Scope::Images), hits(Scope::Docs), hits(Scope::Videos), hits(Scope::Audio));
        let folders = hits(Scope::Folders);
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(images, ["trip-photo.PNG", "trip-scan.jpeg"]);
        assert_eq!(docs, ["trip-budget.xlsx", "trip-mp3.txt", "trip-notes.md"]);
        assert_eq!(videos, ["trip-clip.MOV"]);
        assert_eq!(audio, ["trip-memo.M4A", "trip-song.mp3", "trip-voice.opus"]);
        assert_eq!(folders, ["trip.jpg"]);
    }
}
