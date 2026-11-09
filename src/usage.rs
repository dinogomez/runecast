//! Smarter ranking: how often and how recently each item was run, saved beside the config.
//! Frequent, recent picks float up; a week without use halves an item's boost.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::catalog::Item;

const HALF_LIFE_SECS: f64 = 7.0 * 24.0 * 3600.0;
/// The most a boost adds to a fuzzy score, so a much better text match still wins.
const MAX_BOOST: f64 = 60.0;
const MAX_ENTRIES: usize = 500;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
pub struct Use {
    pub count: u32,
    /// Seconds since the Unix epoch.
    pub last: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Usage {
    uses: HashMap<String, Use>,
}

impl Usage {
    pub fn load() -> Self {
        let uses = std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        Usage { uses }
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(crate::config::dir());
        let _ = std::fs::write(path(), serde_json::to_string(&self.uses).unwrap());
    }

    /// Records a run of `item` at `now` (seconds since the epoch).
    pub fn record(&mut self, item: &Item, now: u64) {
        let entry = self.uses.entry(key(item)).or_default();
        entry.count = entry.count.saturating_add(1);
        entry.last = now;
        if self.uses.len() > MAX_ENTRIES {
            // Forget the stalest.
            let mut all: Vec<(String, f64)> = self.uses.iter().map(|(k, u)| (k.clone(), frecency(*u, now))).collect();
            all.sort_by(|a, b| b.1.total_cmp(&a.1));
            self.uses = all.into_iter().take(MAX_ENTRIES).map(|(k, _)| (k.clone(), self.uses[&k])).collect();
        }
    }

    /// What to add to `item`'s score: 0 for an item never run.
    pub fn boost(&self, item: &Item, now: u64) -> i32 {
        self.uses.get(&key(item)).map_or(0, |u| (MAX_BOOST * (1.0 - (-frecency(*u, now) / 4.0).exp())).round() as i32)
    }
}

/// Uses, each worth less the longer ago the last one was.
fn frecency(u: Use, now: u64) -> f64 {
    let age = now.saturating_sub(u.last) as f64;
    u.count as f64 * 0.5f64.powf(age / HALF_LIFE_SECS)
}

/// Items with the same title and kind count as one, so a rescanned app keeps its history.
fn key(item: &Item) -> String {
    format!("{}:{}", item.kind.label(), item.title)
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn path() -> std::path::PathBuf {
    crate::config::dir().join("usage.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Action, Kind};

    fn app(name: &str) -> Item {
        Item::new(name, "", Kind::App, Action::Shell(String::new()))
    }

    #[test]
    fn more_and_more_recent_use_boosts_more_and_stays_bounded() {
        let (mut u, now) = (Usage::default(), 1_000_000_000);
        let (a, b) = (app("Arc"), app("Bear"));
        assert_eq!(u.boost(&a, now), 0);
        u.record(&a, now);
        for _ in 0..3 {
            u.record(&b, now);
        }
        assert!(u.boost(&b, now) > u.boost(&a, now));
        // Four weeks later, three old uses count for less than one fresh one.
        let later = now + 28 * 24 * 3600;
        u.record(&a, later);
        assert!(u.boost(&a, later) > u.boost(&b, later));
        for _ in 0..1000 {
            u.record(&a, later);
        }
        assert!(u.boost(&a, later) <= MAX_BOOST as i32);
    }
}
