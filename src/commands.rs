use crate::cache::{read_last_search_results, write_last_search_results};
use crate::github::download_symbol;
use crate::search::search_entries;
use crate::sources::{SymbolEntry, fetch_all_sources};
use crate::symbols::{dest_dir_for, find_symbols_dir, list_local_symbols};
use crate::tree::render_search_tree;
use anyhow::Result;
use colored::Colorize;

pub fn cmd_search(query: &str, limit: usize) -> Result<()> {
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

pub fn cmd_install(path: Option<String>, banner: Option<String>) -> Result<()> {
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

pub fn cmd_local() -> Result<()> {
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

pub fn cmd_symbols_dir() -> Result<()> {
    let dir = find_symbols_dir()?;
    println!("{}", dir.display());
    Ok(())
}
