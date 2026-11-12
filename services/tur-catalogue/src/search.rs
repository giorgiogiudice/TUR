// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Search by name and alias.
//!
//! Search works on the loaded catalogue only; nothing is fetched. It is
//! tolerant of case, punctuation and spacing, because names are compared
//! through a normalised key, and of small typing mistakes, through an edit
//! distance with a limit that grows with the length of the query.
//!
//! Ranking is deterministic: a fixed score per kind of match, then the
//! application's name, then its ID.

use serde::Serialize;

use crate::model::Entry;

/// The most hits a search returns.
pub const MAX_HITS: usize = 20;

/// How a query matched a name or alias. Better kinds come first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MatchKind {
    /// The query is the name or alias, ignoring case, punctuation and spacing.
    Exact,
    /// The name or alias starts with the query.
    Prefix,
    /// Every word of the query starts a word of the name or alias.
    Words,
    /// The query appears inside the name or alias.
    Substring,
    /// The query is the name or alias, or its words are, but for small typos.
    Typo,
}

/// One application found by a search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SearchHit<'a> {
    /// The application.
    pub entry: &'a Entry,
    /// The name or alias that matched.
    pub matched: &'a str,
    /// How it matched.
    pub kind: MatchKind,
    /// The ranking score. Higher is better.
    pub score: u32,
}

/// The comparison key of a name: its letters and digits in lower case, with
/// everything else removed. `Counter-Strike 2` and `counterstrike2` share a
/// key.
pub fn normalise(text: &str) -> String {
    text.chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The words of a name: maximal runs of letters and digits, in lower case.
pub fn words(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(|word| word.chars().flat_map(char::to_lowercase).collect())
        .collect()
}

/// How many typing mistakes are forgiven in a word of `length` characters:
/// none up to three characters, one up to six, two beyond that.
pub fn allowed_typos(length: usize) -> usize {
    match length {
        0..=3 => 0,
        4..=6 => 1,
        _ => 2,
    }
}

/// The number of single-character insertions, deletions, substitutions and
/// swaps of neighbours that turn `a` into `b`, or `None` when it is more
/// than `limit`.
pub fn edit_distance(a: &[char], b: &[char], limit: usize) -> Option<usize> {
    if a.len().abs_diff(b.len()) > limit {
        return None;
    }
    let width = b.len() + 1;
    let mut before = vec![0usize; width];
    let mut previous: Vec<usize> = (0..width).collect();
    let mut current = vec![0usize; width];
    for (i, a_char) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, b_char) in b.iter().enumerate() {
            let cost = usize::from(a_char != b_char);
            let mut best = (previous[j + 1] + 1)
                .min(current[j] + 1)
                .min(previous[j] + cost);
            if i > 0 && j > 0 && *a_char == b[j - 1] && a[i - 1] == *b_char {
                best = best.min(before[j - 1] + 1);
            }
            current[j + 1] = best;
        }
        std::mem::swap(&mut before, &mut previous);
        std::mem::swap(&mut previous, &mut current);
    }
    let distance = previous[b.len()];
    (distance <= limit).then_some(distance)
}

/// The cost of matching every word of the query to some word of the
/// candidate: zero for an exact word or a word prefix, the edit distance for
/// a typo within the limit, and `None` when a query word matches nothing.
fn words_cost(query_words: &[Vec<char>], candidate_words: &[Vec<char>]) -> Option<usize> {
    if query_words.is_empty() {
        return None;
    }
    let mut total = 0usize;
    for query_word in query_words {
        let mut best: Option<usize> = None;
        for candidate_word in candidate_words {
            let cost = if candidate_word.starts_with(query_word) {
                Some(0)
            } else {
                edit_distance(query_word, candidate_word, allowed_typos(query_word.len()))
            };
            best = match (best, cost) {
                (Some(known), Some(found)) => Some(known.min(found)),
                (known, found) => known.or(found),
            };
        }
        total += best?;
    }
    Some(total)
}

/// Scores one name or alias against the query.
fn score(
    query_key: &[char],
    query_words: &[Vec<char>],
    candidate: &str,
) -> Option<(u32, MatchKind)> {
    let candidate_key: Vec<char> = normalise(candidate).chars().collect();
    if candidate_key.is_empty() || query_key.is_empty() {
        return None;
    }
    if candidate_key == query_key {
        return Some((1000, MatchKind::Exact));
    }
    if candidate_key.starts_with(query_key) {
        // A shorter name is a closer match to the same prefix.
        let extra = candidate_key.len() - query_key.len();
        let closeness = 99 - u32::try_from(extra.min(99)).unwrap_or(99);
        return Some((800 + closeness, MatchKind::Prefix));
    }
    let candidate_words: Vec<Vec<char>> = words(candidate)
        .iter()
        .map(|word| word.chars().collect())
        .collect();
    let cost = words_cost(query_words, &candidate_words);
    if cost == Some(0) {
        return Some((700, MatchKind::Words));
    }
    if query_key.len() >= 3 && contains(&candidate_key, query_key) {
        return Some((600, MatchKind::Substring));
    }
    if let Some(distance) = edit_distance(query_key, &candidate_key, allowed_typos(query_key.len()))
    {
        let penalty = u32::try_from(distance).unwrap_or(4).min(4) * 100;
        return Some((500 - penalty, MatchKind::Typo));
    }
    cost.map(|distance| {
        let penalty = u32::try_from(distance).unwrap_or(6).min(6) * 50;
        (400 - penalty, MatchKind::Typo)
    })
}

/// Whether `needle` appears in `haystack`.
fn contains(haystack: &[char], needle: &[char]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

/// Searches `entries` by name and alias.
///
/// Returns at most [`MAX_HITS`] hits, best first. An application appears
/// once, through its best-matching name or alias; a name is preferred to an
/// alias that matches equally well. Equal scores are ordered by name, then by
/// application ID, so the same catalogue and query always give the same
/// list. A query with no letter or digit matches nothing.
pub fn search<'a>(entries: &'a [Entry], query: &str) -> Vec<SearchHit<'a>> {
    let query_key: Vec<char> = normalise(query).chars().collect();
    if query_key.is_empty() {
        return Vec::new();
    }
    let query_words: Vec<Vec<char>> = words(query)
        .iter()
        .map(|word| word.chars().collect())
        .collect();

    let mut hits: Vec<SearchHit<'a>> = Vec::new();
    for entry in entries {
        let mut best: Option<SearchHit<'a>> = None;
        let names = std::iter::once((entry.name.as_str(), 5u32))
            .chain(entry.aliases.iter().map(|alias| (alias.as_str(), 0u32)));
        for (candidate, bonus) in names {
            let Some((points, kind)) = score(&query_key, &query_words, candidate) else {
                continue;
            };
            let total = points + bonus;
            if best.as_ref().is_none_or(|hit| total > hit.score) {
                best = Some(SearchHit {
                    entry,
                    matched: candidate,
                    kind,
                    score: total,
                });
            }
        }
        hits.extend(best);
    }
    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| normalise(&a.entry.name).cmp(&normalise(&b.entry.name)))
            .then_with(|| a.entry.id.cmp(&b.entry.id))
    });
    hits.truncate(MAX_HITS);
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Category, Edition, EditionKind, EntryKind, SCHEMA_VERSION};

    fn entry(id: &str, name: &str, aliases: &[&str]) -> Entry {
        Entry {
            schema_version: SCHEMA_VERSION,
            id: id.to_owned(),
            entry_kind: EntryKind::Application,
            name: name.to_owned(),
            aliases: aliases.iter().map(|alias| (*alias).to_owned()).collect(),
            publisher: "Example".to_owned(),
            category: Category::Utility,
            summary: None,
            homepage: None,
            editions: vec![Edition::new(EditionKind::Web)],
        }
    }

    fn sample() -> Vec<Entry> {
        vec![
            entry("org.mozilla.firefox", "Firefox", &["Mozilla Firefox"]),
            entry(
                "com.microsoft.vscode",
                "Visual Studio Code",
                &["VS Code", "vscode"],
            ),
            entry("com.microsoft.visualstudio", "Visual Studio", &["VS"]),
            entry("com.adobe.photoshop", "Adobe Photoshop", &["Photoshop"]),
            entry("com.valvesoftware.cs2", "Counter-Strike 2", &["CS2"]),
            entry(
                "org.libreoffice.libreoffice",
                "LibreOffice",
                &["Libre Office"],
            ),
            entry("org.sevenzip.sevenzip", "7-Zip", &["7zip"]),
            entry("com.cdprojektred.cyberpunk2077", "Cyberpunk 2077", &[]),
        ]
    }

    fn ids(hits: &[SearchHit<'_>]) -> Vec<String> {
        hits.iter().map(|hit| hit.entry.id.clone()).collect()
    }

    #[test]
    fn normalises_case_punctuation_and_spacing() {
        assert_eq!(normalise("Counter-Strike 2"), "counterstrike2");
        assert_eq!(normalise("  7-Zip!  "), "7zip");
        assert_eq!(normalise("ÉCLAIR"), "éclair");
        assert_eq!(normalise("-- !! --"), "");
        assert_eq!(words("Counter-Strike 2"), ["counter", "strike", "2"]);
        assert!(words(" - ").is_empty());
    }

    #[test]
    fn edit_distance_counts_single_edits_and_swaps() {
        let chars = |text: &str| text.chars().collect::<Vec<char>>();
        let distance = |a: &str, b: &str, limit| edit_distance(&chars(a), &chars(b), limit);
        assert_eq!(distance("firefox", "firefox", 2), Some(0));
        assert_eq!(distance("firefox", "firefix", 2), Some(1));
        assert_eq!(distance("firefox", "firefx", 2), Some(1));
        assert_eq!(distance("firefox", "fireefox", 2), Some(1));
        assert_eq!(distance("firefox", "fierfox", 2), Some(1));
        assert_eq!(distance("photoshop", "fotoshop", 2), Some(2));
        assert_eq!(distance("firefox", "chrome", 2), None);
        assert_eq!(distance("abc", "abcdef", 2), None);
        assert_eq!(distance("", "ab", 2), Some(2));
        assert_eq!(distance("ab", "", 2), Some(2));
        assert_eq!(distance("", "", 0), Some(0));
        assert_eq!(distance("ab", "ba", 0), None);
    }

    #[test]
    fn finds_names_whatever_the_case_punctuation_or_spacing() {
        let entries = sample();
        for query in ["firefox", "FIREFOX", "Fire fox", " fire-fox ", "FireFox!"] {
            let hits = search(&entries, query);
            assert_eq!(hits[0].entry.id, "org.mozilla.firefox", "{query}");
            assert_eq!(hits[0].kind, MatchKind::Exact, "{query}");
        }
        for query in [
            "counter strike 2",
            "counterstrike2",
            "Counter-Strike  2",
            "cs2",
            "CS 2",
        ] {
            let hits = search(&entries, query);
            assert_eq!(hits[0].entry.id, "com.valvesoftware.cs2", "{query}");
            assert_eq!(hits[0].kind, MatchKind::Exact, "{query}");
        }
        assert_eq!(
            search(&entries, "7 zip")[0].entry.id,
            "org.sevenzip.sevenzip"
        );
    }

    #[test]
    fn finds_applications_by_alias() {
        let entries = sample();
        let hits = search(&entries, "vs code");
        assert_eq!(hits[0].entry.id, "com.microsoft.vscode");
        assert_eq!(hits[0].matched, "VS Code");
        let hits = search(&entries, "libre office");
        assert_eq!(hits[0].entry.id, "org.libreoffice.libreoffice");
    }

    #[test]
    fn forgives_small_typos() {
        let entries = sample();
        for (query, id) in [
            ("firefix", "org.mozilla.firefox"),
            ("fierfox", "org.mozilla.firefox"),
            ("fotoshop", "com.adobe.photoshop"),
            ("photoshp", "com.adobe.photoshop"),
            ("libreofice", "org.libreoffice.libreoffice"),
            ("cyberpnuk", "com.cdprojektred.cyberpunk2077"),
            ("visual studoi code", "com.microsoft.vscode"),
        ] {
            let hits = search(&entries, query);
            assert!(!hits.is_empty(), "{query} found nothing");
            assert_eq!(hits[0].entry.id, id, "{query}");
            assert_eq!(hits[0].kind, MatchKind::Typo, "{query}");
        }
    }

    #[test]
    fn does_not_forgive_typos_in_very_short_queries_or_unrelated_words() {
        let entries = sample();
        assert!(search(&entries, "cs3").is_empty());
        assert!(search(&entries, "zzzzzzzz").is_empty());
        assert!(search(&entries, "").is_empty());
        assert!(search(&entries, " -- ").is_empty());
    }

    #[test]
    fn ranks_exact_before_prefix_before_words_before_substring() {
        let entries = sample();
        // "Visual Studio" is exact for one entry and a prefix of the other.
        let hits = search(&entries, "visual studio");
        assert_eq!(
            ids(&hits),
            ["com.microsoft.visualstudio", "com.microsoft.vscode"]
        );
        assert_eq!(hits[0].kind, MatchKind::Exact);
        assert_eq!(hits[1].kind, MatchKind::Prefix);

        // A word prefix that is not a prefix of the whole name.
        let hits = search(&entries, "studio");
        assert_eq!(hits[0].kind, MatchKind::Words);
        assert_eq!(
            ids(&hits),
            ["com.microsoft.visualstudio", "com.microsoft.vscode"]
        );

        // Inside a word.
        let hits = search(&entries, "office");
        assert_eq!(ids(&hits), ["org.libreoffice.libreoffice"]);
        assert_eq!(hits[0].kind, MatchKind::Words);
        let hits = search(&entries, "refo");
        assert_eq!(ids(&hits), ["org.mozilla.firefox"]);
        assert_eq!(hits[0].kind, MatchKind::Substring);
    }

    #[test]
    fn prefers_the_shorter_name_for_the_same_prefix_and_the_name_to_an_alias() {
        let entries = sample();
        let hits = search(&entries, "vis");
        assert_eq!(
            ids(&hits),
            ["com.microsoft.visualstudio", "com.microsoft.vscode"]
        );
        // "Photoshop" is an alias; the name matches through its second word.
        let hits = search(&entries, "photoshop");
        assert_eq!(hits[0].matched, "Photoshop");
        assert_eq!(hits[0].kind, MatchKind::Exact);
    }

    #[test]
    fn ranking_is_deterministic_and_independent_of_catalogue_order() {
        let entries = sample();
        let mut reversed = sample();
        reversed.reverse();
        for query in ["v", "studio", "o", "2", "co", "visual studoi"] {
            let forward = ids(&search(&entries, query));
            let backward = ids(&search(&reversed, query));
            assert_eq!(forward, backward, "{query}");
            assert_eq!(forward, ids(&search(&entries, query)), "{query}");
        }
    }

    #[test]
    fn equal_scores_are_ordered_by_name_then_id() {
        let entries = vec![
            entry("org.example.b", "Alpha Tool", &[]),
            entry("org.example.a", "Alpha Tool", &[]),
            entry("org.example.c", "Alpha Edit", &[]),
        ];
        let hits = search(&entries, "alpha");
        assert_eq!(
            ids(&hits),
            ["org.example.c", "org.example.a", "org.example.b"]
        );
    }

    #[test]
    fn returns_at_most_the_limit() {
        let entries: Vec<Entry> = (0..50)
            .map(|number| {
                entry(
                    &format!("org.example.app{number}"),
                    &format!("App {number}"),
                    &[],
                )
            })
            .collect();
        assert_eq!(search(&entries, "app").len(), MAX_HITS);
    }
}
