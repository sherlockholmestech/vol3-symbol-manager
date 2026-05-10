use crate::sources::SymbolEntry;

pub fn search_entries<'a>(entries: &'a [SymbolEntry], query: &str) -> Vec<&'a SymbolEntry> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }

    let mut hits = entries
        .iter()
        .filter_map(|entry| score_entry(entry, query).map(|score| (score, entry)))
        .collect::<Vec<_>>();

    hits.sort_by(|(score_a, entry_a), (score_b, entry_b)| {
        score_b
            .cmp(score_a)
            .then_with(|| entry_a.banner.cmp(&entry_b.banner))
            .then_with(|| entry_a.source_id.cmp(&entry_b.source_id))
            .then_with(|| entry_a.path.cmp(&entry_b.path))
    });

    hits.into_iter().map(|(_, entry)| entry).collect()
}

fn score_entry(entry: &SymbolEntry, query: &str) -> Option<i32> {
    let query_lower = query.to_lowercase();
    let query_normalized = normalize_search_text(query);
    let query_tokens = tokenize_search_text(query);
    let basename = entry.path.rsplit('/').next().unwrap_or(&entry.path);
    let display_path = entry.display_path();

    let fields = [
        SearchField::new(&display_path),
        SearchField::new(basename),
        SearchField::new(&entry.banner),
        SearchField::new(&entry.source_name),
    ];

    let mut score = 0;
    let mut whole_query_match = false;

    for (idx, field) in fields.iter().enumerate() {
        if field.lower.contains(&query_lower) {
            score += match idx {
                0 => 280,
                1 => 320,
                2 => 220,
                _ => 80,
            };
            whole_query_match = true;
        }

        if !query_normalized.is_empty() && field.normalized.contains(&query_normalized) {
            score += match idx {
                0 => 160,
                1 => 180,
                2 => 140,
                _ => 50,
            };
            whole_query_match = true;
        }
    }

    let mut matched_tokens = 0;
    for token in &query_tokens {
        let best = fields
            .iter()
            .enumerate()
            .map(|(idx, field)| score_token_against_field(token, field, idx))
            .max()
            .unwrap_or(0);

        if best > 0 {
            matched_tokens += 1;
            score += best;
        }
    }

    if !whole_query_match && matched_tokens < query_tokens.len() {
        return None;
    }

    if query_tokens.len() == 1 {
        let token = &query_tokens[0];
        let best_subsequence = fields
            .iter()
            .enumerate()
            .filter_map(|(idx, field)| {
                subsequence_score(token, &field.compact).map(|subscore| {
                    let base = match idx {
                        0 => 35,
                        1 => 55,
                        2 => 25,
                        _ => 10,
                    };
                    base + subscore.min(30) as i32
                })
            })
            .max()
            .unwrap_or(0);
        score += best_subsequence;
    }

    (score > 0).then_some(score)
}

struct SearchField {
    lower: String,
    normalized: String,
    compact: String,
    tokens: Vec<String>,
}

impl SearchField {
    fn new(value: &str) -> Self {
        let lower = value.to_lowercase();
        let normalized = normalize_search_text(value);
        let compact = normalized.replace(' ', "");
        let tokens = tokenize_search_text(value);
        Self {
            lower,
            normalized,
            compact,
            tokens,
        }
    }
}

fn score_token_against_field(token: &str, field: &SearchField, field_idx: usize) -> i32 {
    let base = match field_idx {
        0 => 70,
        1 => 90,
        2 => 55,
        _ => 25,
    };
    let allow_fuzzy = is_fuzzy_text_token(token);

    let mut best = if field.lower.contains(token) {
        base + 20
    } else {
        0
    };

    for candidate in &field.tokens {
        if candidate == token {
            best = best.max(base + 60);
            continue;
        }
        if candidate.starts_with(token) || (allow_fuzzy && token.starts_with(candidate)) {
            best = best.max(base + 40);
            continue;
        }
        if candidate.contains(token) {
            best = best.max(base + 25);
            continue;
        }

        if allow_fuzzy && token.len() >= 4 && candidate.len() >= 4 {
            let max_distance = if token.len().max(candidate.len()) <= 5 {
                1
            } else {
                2
            };
            if let Some(distance) = bounded_edit_distance(token, candidate, max_distance) {
                let fuzzy = match distance {
                    0 => base + 60,
                    1 => base + 22,
                    2 => base + 12,
                    _ => 0,
                };
                best = best.max(fuzzy);
            }
        }

        if allow_fuzzy && token.len() >= 3
            && let Some(subscore) = subsequence_score(token, candidate) {
                best = best.max(base + subscore.min(18) as i32);
            }
    }

    best
}

fn normalize_search_text(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | '+') {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn tokenize_search_text(input: &str) -> Vec<String> {
    normalize_search_text(input)
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

fn is_fuzzy_text_token(token: &str) -> bool {
    token.chars().all(|ch| ch.is_ascii_alphabetic())
}

fn subsequence_score(needle: &str, haystack: &str) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }

    let mut positions = Vec::with_capacity(needle.len());
    let mut haystack_iter = haystack.char_indices();

    for needle_char in needle.chars() {
        let mut found = None;
        for (idx, haystack_char) in haystack_iter.by_ref() {
            if haystack_char == needle_char {
                found = Some(idx);
                break;
            }
        }

        match found {
            Some(idx) => positions.push(idx),
            None => return None,
        }
    }

    let span = positions.last()? - positions.first()? + 1;
    Some(needle.len().saturating_mul(8).saturating_sub(span))
}

fn bounded_edit_distance(a: &str, b: &str, max_distance: usize) -> Option<usize> {
    let a_chars = a.chars().collect::<Vec<_>>();
    let b_chars = b.chars().collect::<Vec<_>>();
    if a_chars.len().abs_diff(b_chars.len()) > max_distance {
        return None;
    }

    let mut prev = (0..=b_chars.len()).collect::<Vec<_>>();
    let mut curr = vec![0; b_chars.len() + 1];

    for (i, a_char) in a_chars.iter().enumerate() {
        curr[0] = i + 1;
        let mut row_min = curr[0];

        for (j, b_char) in b_chars.iter().enumerate() {
            let cost = usize::from(a_char != b_char);
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
            row_min = row_min.min(curr[j + 1]);
        }

        if row_min > max_distance {
            return None;
        }

        std::mem::swap(&mut prev, &mut curr);
    }

    let distance = prev[b_chars.len()];
    (distance <= max_distance).then_some(distance)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(source_id: &str, banner: &str, path: &str) -> SymbolEntry {
        SymbolEntry {
            source_id: source_id.to_string(),
            source_name: source_id.to_string(),
            banner: banner.to_string(),
            path: path.to_string(),
            download_url: format!("https://example.invalid/{path}"),
        }
    }

    #[test]
    fn fuzzy_search_ranks_best_path_match_first() {
        let entries = vec![
            entry(
                "abyss",
                "Linux version 6.2.0-1007-aws",
                "Ubuntu/amd64/6.2.0/1007/aws/Ubuntu_6.2.0-1007-aws_amd64.json.xz",
            ),
            entry(
                "leludo",
                "Linux version 6.2.0-1007-aws",
                "profiles/ubuntu22/linux-image-unsigned-6.2.0-1007-aws-dbgsym_x86_64.json.xz",
            ),
            entry(
                "p0d",
                "",
                "symbols/Ubuntu/5.4.0-99/5.4.0-99-generic/amd64/foo.json.xz",
            ),
        ];

        let results = search_entries(&entries, "ubuntu aws 1007");

        assert_eq!(results[0].source_id, "abyss");
        assert_eq!(results[1].source_id, "leludo");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn fuzzy_search_tolerates_small_typos() {
        let entries = vec![entry(
            "abyss",
            "Linux version 6.2.0-1007-aws",
            "Ubuntu/amd64/6.2.0/1007/aws/Ubuntu_6.2.0-1007-aws_amd64.json.xz",
        )];

        let results = search_entries(&entries, "ubntu 1007 aws");

        assert_eq!(results.len(), 1);
    }

    #[test]
    fn bounded_edit_distance_respects_limit() {
        assert_eq!(bounded_edit_distance("ubuntu", "ubntu", 2), Some(1));
        assert_eq!(bounded_edit_distance("ubuntu", "debian", 2), None);
    }

    #[test]
    fn subsequence_score_prefers_compact_matches() {
        let compact = subsequence_score("ubt", "ubuntu").unwrap();
        let spread = subsequence_score("ubt", "u_bu_n_tu").unwrap();

        assert!(compact > spread);
    }

    #[test]
    fn tokenization_preserves_kernel_versions() {
        assert_eq!(
            tokenize_search_text("ubntu 6.2.0-1007 aws"),
            vec!["ubntu", "6.2.0-1007", "aws"]
        );
    }

    #[test]
    fn fuzzy_logic_only_applies_to_text_tokens() {
        assert!(is_fuzzy_text_token("ubuntu"));
        assert!(!is_fuzzy_text_token("6.2.0-1007"));
        assert!(!is_fuzzy_text_token("ubuntu22"));
    }
}
