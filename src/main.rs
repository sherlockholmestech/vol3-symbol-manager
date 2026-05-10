use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::Colorize;
use std::collections::BTreeMap;
use std::path::PathBuf;

mod github;
mod sources;
mod symbols;

use github::download_symbol;
use sources::{SymbolEntry, fetch_all_sources};
use symbols::{dest_dir_for, find_symbols_dir, list_local_symbols};

#[derive(Parser)]
#[command(name = "vol3sm")]
#[command(
    about = "Volatility3 symbol manager — search and install ISF symbols from public symbol indexes"
)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Search for symbols by kernel banner or filename pattern
    Search {
        /// Query string: banner snippet, distro name, kernel version, arch, etc.
        query: String,
        /// Maximum results to display
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },

    /// Download and install a symbol into volatility3
    Install {
        /// Repository path or numeric id from the last search results
        path: Option<String>,
        /// Exact kernel banner string (from `vol.py banners` output)
        #[arg(short, long, value_name = "BANNER")]
        banner: Option<String>,
    },

    /// List locally installed symbol files
    Local,

    /// Print the detected volatility3 symbols directory
    SymbolsDir,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Search { query, limit } => cmd_search(&query, limit),
        Commands::Install { path, banner } => cmd_install(path, banner),
        Commands::Local => cmd_local(),
        Commands::SymbolsDir => cmd_symbols_dir(),
    }
}

fn cmd_search(query: &str, limit: usize) -> Result<()> {
    eprintln!("{}", "Fetching symbol indexes...".dimmed());
    let entries = fetch_all_sources()?;
    let results = search_entries(&entries, query);

    if results.is_empty() {
        println!("No symbols found matching '{}'.", query);
        return Ok(());
    }

    let total = results.len();
    let shown = total.min(limit);
    let tree = render_search_tree(&results, shown);
    write_last_search_results(query, &results[..shown])?;

    println!(
        "Found {} match{} for {} (showing {}):\n",
        total.to_string().bold(),
        if total == 1 { "" } else { "es" },
        query.cyan().bold(),
        shown
    );

    println!("{}", tree);

    if total > limit {
        println!(
            "{} more result{}. Use --limit {} to see all.",
            (total - limit).to_string().yellow(),
            if total - limit == 1 { "" } else { "s" },
            total
        );
    }

    Ok(())
}

fn search_entries<'a>(entries: &'a [SymbolEntry], query: &str) -> Vec<&'a SymbolEntry> {
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

        if allow_fuzzy && token.len() >= 3 {
            if let Some(subscore) = subsequence_score(token, candidate) {
                best = best.max(base + subscore.min(18) as i32);
            }
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

fn render_search_tree(results: &[&SymbolEntry], shown: usize) -> String {
    let mut tree = PathTree::default();
    let mut has_paths = false;

    for (id, entry) in results.iter().take(shown).enumerate() {
        tree.insert(&entry.display_path(), id);
        has_paths = true;
    }

    let mut output = String::new();
    output.push_str(&format!("{}\n", "Matched symbol paths".bold()));

    if has_paths {
        tree.render_into(&mut output, "");
        let (dirs, files) = tree.counts();
        output.push_str(
            &format!(
                "\n{} {} • {} {}",
                dirs,
                if dirs == 1 {
                    "directory"
                } else {
                    "directories"
                },
                files,
                if files == 1 { "file" } else { "files" }
            )
            .dimmed()
            .to_string(),
        );
    } else {
        output.push_str("└── (no symbol path)\n\n0 directories, 0 files");
    }

    output.push_str(&format!(
        "\n\n{}",
        "Use: vol3sm install <id>  or  vol3sm install \"<source:path>\"".dimmed()
    ));

    output.trim_end().to_string()
}

#[derive(serde::Serialize, serde::Deserialize)]
struct LastSearchResults {
    query: String,
    entries: Vec<SymbolEntry>,
}

fn write_last_search_results(query: &str, results: &[&SymbolEntry]) -> Result<()> {
    let cache = LastSearchResults {
        query: query.to_string(),
        entries: results.iter().map(|entry| (*entry).clone()).collect(),
    };
    let path = search_cache_path();
    let data = serde_json::to_vec_pretty(&cache)?;
    std::fs::write(&path, data)?;
    Ok(())
}

fn read_last_search_results() -> Result<LastSearchResults> {
    let path = search_cache_path();
    let data = std::fs::read(&path).map_err(|err| {
        anyhow::anyhow!(
            "Could not read last search results from {}: {}",
            path.display(),
            err
        )
    })?;
    Ok(serde_json::from_slice(&data)?)
}

fn search_cache_path() -> PathBuf {
    if let Ok(path) = std::env::var("VOL3SM_SEARCH_CACHE") {
        return PathBuf::from(path);
    }

    std::env::temp_dir().join("vol3sm-last-search.json")
}

#[derive(Default)]
struct PathTree {
    children: BTreeMap<String, PathTree>,
    is_file: bool,
    file_id: Option<usize>,
}

impl PathTree {
    fn insert(&mut self, path: &str, file_id: usize) {
        let mut current = self;
        let mut segments = path.split('/').filter(|s| !s.is_empty()).peekable();
        while let Some(segment) = segments.next() {
            current = current.children.entry(segment.to_string()).or_default();
            if segments.peek().is_none() {
                current.is_file = true;
                current.file_id = Some(file_id);
            }
        }
    }

    fn render_into(&self, output: &mut String, prefix: &str) {
        let count = self.children.len();
        for (index, (name, child)) in self.children.iter().enumerate() {
            let is_last = index + 1 == count;
            let branch = if is_last { "└──" } else { "├──" };

            if child.is_file {
                output.push_str(&format!(
                    "{}{} {}{}\n",
                    prefix,
                    branch.dimmed(),
                    style_file_name(name),
                    style_file_id(child.file_id)
                ));
            } else {
                let (collapsed_name, collapsed_child) = child.collapsed_dir_name(name);
                output.push_str(&format!(
                    "{}{} {}\n",
                    prefix,
                    branch.dimmed(),
                    style_dir_name(&collapsed_name)
                ));
                let cont = if is_last {
                    "    ".to_string()
                } else {
                    "│   ".dimmed().to_string()
                };
                let child_prefix = format!("{prefix}{cont}");
                collapsed_child.render_into(output, &child_prefix);
            }
        }
    }

    fn counts(&self) -> (usize, usize) {
        let mut dirs = 0usize;
        let mut files = 0usize;

        for child in self.children.values() {
            if child.is_file {
                files += 1;
                continue;
            }

            dirs += 1;
            let (subdirs, subfiles) = child.counts();
            dirs += subdirs;
            files += subfiles;
        }

        (dirs, files)
    }

    fn collapsed_dir_name<'a>(&'a self, name: &str) -> (String, &'a PathTree) {
        let mut parts = vec![name.to_string()];
        let mut current = self;

        loop {
            if current.is_file || current.children.len() != 1 {
                break;
            }

            let Some((child_name, child)) = current.children.iter().next() else {
                break;
            };

            if child.is_file {
                break;
            }

            parts.push(child_name.clone());
            current = child;
        }

        (format!("{}/", parts.join("/")), current)
    }
}

fn style_dir_name(name: &str) -> String {
    name.blue().bold().to_string()
}

fn style_file_name(name: &str) -> String {
    for ext in [".json.xz", ".json.gz", ".json"] {
        if let Some(base) = name.strip_suffix(ext) {
            return format!("{}{}", base.cyan().bold(), ext.dimmed());
        }
    }

    name.cyan().bold().to_string()
}

fn style_file_id(file_id: Option<usize>) -> String {
    file_id
        .map(|id| format!(" {}", format!("[{}]", id).dimmed()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{
        SymbolEntry, bounded_edit_distance, is_fuzzy_text_token, read_last_search_results,
        render_search_tree, resolve_search_id, search_cache_path, search_entries,
        subsequence_score, tokenize_search_text, write_last_search_results,
    };
    use std::sync::{Mutex, OnceLock};

    fn entry(source_id: &str, banner: &str, path: &str) -> SymbolEntry {
        SymbolEntry {
            source_id: source_id.to_string(),
            source_name: source_id.to_string(),
            banner: banner.to_string(),
            path: path.to_string(),
            download_url: format!("https://example.invalid/{path}"),
        }
    }

    fn test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn render_search_tree_collapses_single_child_directories() {
        let results = vec![
            entry(
                "abyss",
                "Linux version 6.1",
                "Debian/amd64/6.1.0/1/debian.json.xz",
            ),
            entry(
                "abyss",
                "Linux version 6.1",
                "Debian/amd64/6.1.0/1/debian-debug.json.xz",
            ),
        ];
        let result_refs = results.iter().collect::<Vec<_>>();

        let rendered = render_search_tree(&result_refs, 2);

        assert!(rendered.contains("Matched symbol paths"));
        assert!(rendered.contains("abyss:Debian/amd64/6.1.0/1/"));
        assert!(rendered.contains("debian"));
        assert!(rendered.contains("[0]"));
        assert!(rendered.contains("[1]"));
        assert!(rendered.contains("2 files"));
        assert!(!rendered.contains("\n.\n"));
    }

    #[test]
    fn render_search_tree_counts_directories_after_collapsing() {
        let results = vec![
            entry(
                "abyss",
                "Linux version 5.4",
                "Ubuntu/amd64/5.4.0/1/ubuntu.json.xz",
            ),
            entry(
                "abyss",
                "Linux version 5.4",
                "Ubuntu/arm64/5.4.0/1/ubuntu-arm.json.xz",
            ),
        ];
        let result_refs = results.iter().collect::<Vec<_>>();

        let rendered = render_search_tree(&result_refs, 2);

        assert!(rendered.contains("7 directories"));
        assert!(rendered.contains("2 files"));
        assert!(rendered.contains("abyss:Ubuntu/"));
        assert!(rendered.contains("amd64/5.4.0/1/"));
        assert!(rendered.contains("arm64/5.4.0/1/"));
    }

    #[test]
    fn render_search_tree_shows_ids_next_to_files() {
        let results = vec![
            entry(
                "abyss",
                "Linux version 6.1",
                "Debian/amd64/6.1.0/1/debian.json.xz",
            ),
            entry(
                "leludo",
                "Linux version 6.1",
                "profiles/debian/debian.json.xz",
            ),
        ];
        let result_refs = results.iter().collect::<Vec<_>>();

        let rendered = render_search_tree(&result_refs, 2);

        assert!(rendered.contains("[0]"));
        assert!(rendered.contains("[1]"));
        assert!(rendered.contains("vol3sm install <id>"));
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

    #[test]
    fn search_results_are_cached_and_resolvable_by_id() {
        let _guard = test_lock().lock().unwrap();
        let cache_path =
            std::env::temp_dir().join(format!("vol3sm-test-cache-{}", std::process::id()));
        unsafe {
            std::env::set_var("VOL3SM_SEARCH_CACHE", &cache_path);
        }

        let entries = vec![
            entry(
                "abyss",
                "Linux version 6.1",
                "Debian/amd64/6.1.0/1/debian.json.xz",
            ),
            entry(
                "leludo",
                "Linux version 6.1",
                "profiles/debian/debian.json.xz",
            ),
        ];
        let entry_refs = entries.iter().collect::<Vec<_>>();

        write_last_search_results("debian 6.1", &entry_refs).unwrap();

        let cache = read_last_search_results().unwrap();
        assert_eq!(cache.query, "debian 6.1");
        assert_eq!(cache.entries.len(), 2);
        assert_eq!(resolve_search_id(1).unwrap().source_id, "leludo");

        let _ = std::fs::remove_file(search_cache_path());
        unsafe {
            std::env::remove_var("VOL3SM_SEARCH_CACHE");
        }
    }
}

fn cmd_install(path: Option<String>, banner: Option<String>) -> Result<()> {
    let entry = match (path, banner) {
        (Some(p), None) => resolve_path(&p)?,

        (None, Some(b)) => {
            eprintln!("{}", "Fetching symbol indexes...".dimmed());
            let matches = fetch_all_sources()?
                .into_iter()
                .filter(|entry| entry.banner == b)
                .collect::<Vec<_>>();
            if matches.is_empty() {
                anyhow::bail!("No symbol found for banner:\n  {}", b);
            }
            if matches.len() > 1 {
                println!("Multiple symbols match this banner:");
                for (i, entry) in matches.iter().enumerate() {
                    println!("  [{}] {}", i, entry.display_path().green());
                }
                println!("Installing: {}", matches[0].display_path().green());
            }
            matches[0].clone()
        }

        (None, None) => {
            anyhow::bail!(
                "Provide a repository path or use --banner \"<banner string>\".\n\
                 Tip: get the banner by running:  vol.py -r pretty -f <dump> banners"
            );
        }

        (Some(_), Some(_)) => {
            anyhow::bail!("Provide either a path or --banner, not both.");
        }
    };

    let symbols_dir = find_symbols_dir()?;
    let dest = dest_dir_for(&symbols_dir, &entry.path);
    std::fs::create_dir_all(&dest)?;

    println!("Source : {}", entry.source_name.green());
    println!("Symbol : {}", entry.display_path().green());
    println!("Dest   : {}", dest.display().to_string().yellow());

    download_symbol(&entry.download_url, &dest, &entry.path)?;

    Ok(())
}

fn resolve_path(path: &str) -> Result<SymbolEntry> {
    if let Ok(id) = path.parse::<usize>() {
        return resolve_search_id(id);
    }

    eprintln!("{}", "Fetching symbol indexes...".dimmed());
    let entries = fetch_all_sources()?;
    let matches = entries
        .into_iter()
        .filter(|entry| {
            entry.display_path() == path
                || (!path.contains(':') && entry.source_id == "abyss" && entry.path == path)
        })
        .collect::<Vec<_>>();

    match matches.as_slice() {
        [entry] => Ok(entry.clone()),
        [] => anyhow::bail!(
            "No symbol found for path '{}'. Use `vol3sm search <query>` and install the source-qualified path.",
            path
        ),
        _ => {
            println!("Multiple symbols match this path:");
            for (i, entry) in matches.iter().enumerate() {
                println!("  [{}] {}", i, entry.display_path().green());
            }
            println!("Installing: {}", matches[0].display_path().green());
            Ok(matches[0].clone())
        }
    }
}

fn resolve_search_id(id: usize) -> Result<SymbolEntry> {
    let cache = read_last_search_results()?;
    cache.entries.get(id).cloned().ok_or_else(|| {
        anyhow::anyhow!(
            "Search id {} is out of range for the last search '{}'. Run `vol3sm search ...` again.",
            id,
            cache.query
        )
    })
}

fn cmd_local() -> Result<()> {
    let symbols_dir = find_symbols_dir()?;
    println!(
        "Symbols dir: {}\n",
        symbols_dir.display().to_string().yellow()
    );

    let files = list_local_symbols(&symbols_dir)?;
    if files.is_empty() {
        println!("No symbols installed.");
    } else {
        println!("Installed ({}):", files.len().to_string().bold());
        for f in &files {
            let rel = f
                .strip_prefix(&symbols_dir)
                .unwrap_or(f)
                .display()
                .to_string();
            println!("  {}", rel.green());
        }
    }

    Ok(())
}

fn cmd_symbols_dir() -> Result<()> {
    let dir = find_symbols_dir()?;
    println!("{}", dir.display());
    Ok(())
}
