//! Fuzzy matching: every query character must appear in order. Matches score higher at the start of
//! the title, at word starts and in runs, so "vsc" ranks "Visual Studio Code" above "Services".

/// The match score, or `None` when `query` isn't a subsequence of `candidate` (ignoring case).
pub fn score(query: &str, candidate: &str) -> Option<i32> {
    let query: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect();
    if query.is_empty() {
        return Some(0);
    }
    let chars: Vec<char> = candidate.chars().collect();
    let mut qi = 0;
    let mut score = 0;
    let mut prev_match: Option<usize> = None;
    for (i, &c) in chars.iter().enumerate() {
        if qi == query.len() {
            break;
        }
        if !c.to_lowercase().eq(std::iter::once(query[qi])) {
            continue;
        }
        let word_start = i == 0 || !chars[i - 1].is_alphanumeric() || (c.is_uppercase() && chars[i - 1].is_lowercase());
        score += 1;
        if i == 0 {
            score += 8;
        }
        if word_start {
            score += 6;
        }
        match prev_match {
            Some(p) if p + 1 == i => score += 5,
            Some(p) => score -= (i - p - 1).min(4) as i32,
            None => score -= i.min(6) as i32,
        }
        prev_match = Some(i);
        qi += 1;
    }
    // Shorter titles win ties: "Notes" before "Notes Helper".
    (qi == query.len()).then(|| score * 4 - chars.len().min(40) as i32 / 4)
}

#[cfg(test)]
mod tests {
    use super::score;

    #[test]
    fn requires_every_character_in_order() {
        assert!(score("sfr", "Safari").is_some());
        assert!(score("rfs", "Safari").is_none());
        assert!(score("safarix", "Safari").is_none());
    }

    #[test]
    fn word_starts_beat_scattered_letters() {
        let initials = score("vsc", "Visual Studio Code").unwrap();
        let scattered = score("vsc", "Devices and Services").unwrap();
        assert!(initials > scattered, "{initials} vs {scattered}");
    }

    #[test]
    fn prefix_beats_a_match_in_the_middle() {
        assert!(score("term", "Terminal").unwrap() > score("term", "Determine").unwrap());
    }

    #[test]
    fn shorter_title_wins_a_tie() {
        assert!(score("notes", "Notes").unwrap() > score("notes", "Notes Helper").unwrap());
    }
}
