//! Pure fuzzy index for the compact settings and diagnostics surfaces.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsDestination {
    General,
    Devices,
    Permissions,
    Advanced,
    Diagnostics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsSearchResult {
    pub title: &'static str,
    pub section: &'static str,
    pub destination: SettingsDestination,
    pub score: i32,
}

#[derive(Clone, Copy)]
struct SearchEntry {
    title: &'static str,
    section: &'static str,
    keywords: &'static str,
    destination: SettingsDestination,
}

const ENTRIES: &[SearchEntry] = &[
    SearchEntry {
        title: "Reverse scrolling",
        section: "General",
        keywords: "enable disable master switch",
        destination: SettingsDestination::General,
    },
    SearchEntry {
        title: "Mouse wheel",
        section: "General",
        keywords: "trackpad magic mouse device type",
        destination: SettingsDestination::General,
    },
    SearchEntry {
        title: "Vertical and horizontal directions",
        section: "General",
        keywords: "axis direction reverse natural",
        destination: SettingsDestination::General,
    },
    SearchEntry {
        title: "Wheel step size",
        section: "General",
        keywords: "lines notch speed discrete",
        destination: SettingsDestination::General,
    },
    SearchEntry {
        title: "Per-device rules and aliases",
        section: "Devices",
        keywords: "connected remembered unavailable inherit profile mouse",
        destination: SettingsDestination::Devices,
    },
    SearchEntry {
        title: "Accessibility permission",
        section: "Permissions",
        keywords: "privacy security input event tap grant",
        destination: SettingsDestination::Permissions,
    },
    SearchEntry {
        title: "Start at login",
        section: "Permissions",
        keywords: "startup launch login item",
        destination: SettingsDestination::Permissions,
    },
    SearchEntry {
        title: "Ignore posted and remote scroll events",
        section: "Advanced",
        keywords: "injected raw input source pid bypass",
        destination: SettingsDestination::Advanced,
    },
    SearchEntry {
        title: "Debug Console",
        section: "Diagnostics",
        keywords: "events resolution decision filter",
        destination: SettingsDestination::Diagnostics,
    },
    SearchEntry {
        title: "Trace and CSV export",
        section: "Diagnostics",
        keywords: "privacy replay save detailed support",
        destination: SettingsDestination::Diagnostics,
    },
    SearchEntry {
        title: "Latency and event-rate metrics",
        section: "Diagnostics",
        keywords: "performance callback p50 p95 histogram",
        destination: SettingsDestination::Diagnostics,
    },
    SearchEntry {
        title: "Scroll benchmark",
        section: "Diagnostics",
        keywords: "curve smooth transfer target test",
        destination: SettingsDestination::Diagnostics,
    },
];

pub fn search_settings(query: &str, limit: usize) -> Vec<SettingsSearchResult> {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() || limit == 0 {
        return Vec::new();
    }
    let tokens: Vec<_> = query.split_whitespace().collect();
    let mut results = ENTRIES
        .iter()
        .filter_map(|entry| {
            let haystack = format!("{} {} {}", entry.title, entry.section, entry.keywords)
                .to_ascii_lowercase();
            let score = tokens.iter().try_fold(0i32, |total, token| {
                fuzzy_token_score(&haystack, token).map(|score| total + score)
            })?;
            Some(SettingsSearchResult {
                title: entry.title,
                section: entry.section,
                destination: entry.destination,
                score,
            })
        })
        .collect::<Vec<_>>();
    results.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.title.cmp(right.title))
    });
    results.truncate(limit);
    results
}

fn fuzzy_token_score(haystack: &str, needle: &str) -> Option<i32> {
    if let Some(position) = haystack.find(needle) {
        return Some(1_000 - i32::try_from(position).unwrap_or(i32::MAX).min(500));
    }

    let mut positions = haystack.char_indices();
    let mut first = None;
    let mut last = 0usize;
    for wanted in needle.chars() {
        let (position, _) = positions.find(|(_, candidate)| *candidate == wanted)?;
        first.get_or_insert(position);
        last = position;
    }
    let span = last.saturating_sub(first.unwrap_or(last));
    Some(500 - i32::try_from(span).unwrap_or(i32::MAX).min(400))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_has_no_results_and_limit_is_honored() {
        assert!(search_settings("  ", 5).is_empty());
        assert_eq!(search_settings("e", 2).len(), 2);
        assert!(search_settings("mouse", 0).is_empty());
    }

    #[test]
    fn exact_and_multiword_queries_rank_expected_destinations() {
        let exact = search_settings("start at login", 3);
        let multiword = search_settings("device alias", 3);

        assert_eq!(exact[0].title, "Start at login");
        assert_eq!(exact[0].destination, SettingsDestination::Permissions);
        assert_eq!(multiword[0].destination, SettingsDestination::Devices);
    }

    #[test]
    fn subsequence_matching_tolerates_a_missing_letter() {
        let results = search_settings("permssion", 3);

        assert!(!results.is_empty());
        assert_eq!(results[0].destination, SettingsDestination::Permissions);
    }

    #[test]
    fn diagnostics_terms_route_out_of_the_main_settings_tabs() {
        for query in ["trace", "latency", "benchmark", "curve"] {
            assert_eq!(
                search_settings(query, 1)[0].destination,
                SettingsDestination::Diagnostics
            );
        }
    }

    #[test]
    fn remote_input_policy_routes_to_advanced() {
        assert_eq!(
            search_settings("remote", 1)[0].destination,
            SettingsDestination::Advanced
        );
    }
}
