use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::Colorize;
use std::collections::BTreeMap;

mod github;
mod symbols;

use github::{download_symbol, fetch_banner_index};
use symbols::{dest_dir_for, find_symbols_dir, list_local_symbols};

const BANNERS_URL: &str = "https://raw.githubusercontent.com/Abyss-W4tcher/volatility3-symbols/master/banners/banners_plain.json";
const RAW_BASE: &str = "https://github.com/Abyss-W4tcher/volatility3-symbols/raw/master";

#[derive(Parser)]
#[command(name = "vol3sm")]
#[command(
    about = "Volatility3 symbol manager — search and install ISF symbols from Abyss-W4tcher/volatility3-symbols"
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
        /// Repository path  (e.g. "Debian/amd64/5.4.0/1/Debian_5.4.0-1-amd64_...json.xz")
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
    eprintln!("{}", "Fetching symbol index...".dimmed());
    let index = fetch_banner_index(BANNERS_URL)?;

    let q = query.to_lowercase();
    let mut results: Vec<(&String, &Vec<String>)> = index
        .iter()
        .filter(|(banner, paths)| {
            banner.to_lowercase().contains(&q)
                || paths.iter().any(|p| p.to_lowercase().contains(&q))
        })
        .collect();

    results.sort_by_key(|(b, _)| b.as_str());

    if results.is_empty() {
        println!("No symbols found matching '{}'.", query);
        return Ok(());
    }

    let total = results.len();
    let shown = total.min(limit);
    let tree = render_search_tree(&results, shown);

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

fn render_search_tree(results: &[(&String, &Vec<String>)], shown: usize) -> String {
    let mut tree = PathTree::default();
    let mut has_paths = false;

    for (_banner, paths) in results.iter().take(shown) {
        for path in paths.iter() {
            tree.insert(path);
            has_paths = true;
        }
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

    output.trim_end().to_string()
}

#[derive(Default)]
struct PathTree {
    children: BTreeMap<String, PathTree>,
    is_file: bool,
}

impl PathTree {
    fn insert(&mut self, path: &str) {
        let mut current = self;
        let mut segments = path.split('/').filter(|s| !s.is_empty()).peekable();
        while let Some(segment) = segments.next() {
            current = current.children.entry(segment.to_string()).or_default();
            if segments.peek().is_none() {
                current.is_file = true;
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
                    "{}{} {}\n",
                    prefix,
                    branch.dimmed(),
                    style_file_name(name)
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

#[cfg(test)]
mod tests {
    use super::render_search_tree;

    #[test]
    fn render_search_tree_collapses_single_child_directories() {
        let banner = String::from("Linux version 6.1");
        let paths = vec![
            String::from("Debian/amd64/6.1.0/1/debian.json.xz"),
            String::from("Debian/amd64/6.1.0/1/debian-debug.json.xz"),
        ];
        let results = vec![(&banner, &paths)];

        let rendered = render_search_tree(&results, 1);

        assert!(rendered.contains("Matched symbol paths"));
        assert!(rendered.contains("Debian/amd64/6.1.0/1/"));
        assert!(rendered.contains("debian"));
        assert!(rendered.contains("2 files"));
        assert!(!rendered.contains("\n.\n"));
    }

    #[test]
    fn render_search_tree_counts_directories_after_collapsing() {
        let banner = String::from("Linux version 5.4");
        let paths = vec![
            String::from("Ubuntu/amd64/5.4.0/1/ubuntu.json.xz"),
            String::from("Ubuntu/arm64/5.4.0/1/ubuntu-arm.json.xz"),
        ];
        let results = vec![(&banner, &paths)];

        let rendered = render_search_tree(&results, 1);

        assert!(rendered.contains("7 directories"));
        assert!(rendered.contains("2 files"));
        assert!(rendered.contains("Ubuntu/"));
        assert!(rendered.contains("amd64/5.4.0/1/"));
        assert!(rendered.contains("arm64/5.4.0/1/"));
    }
}

fn cmd_install(path: Option<String>, banner: Option<String>) -> Result<()> {
    let symbol_path = match (path, banner) {
        (Some(p), None) => p,

        (None, Some(b)) => {
            eprintln!("{}", "Fetching symbol index...".dimmed());
            let index = fetch_banner_index(BANNERS_URL)?;
            let paths = index
                .get(&b)
                .ok_or_else(|| anyhow::anyhow!("No symbol found for banner:\n  {}", b))?;
            if paths.is_empty() {
                anyhow::bail!("Banner matched but has no associated symbol paths.");
            }
            if paths.len() > 1 {
                println!("Multiple symbols match this banner:");
                for (i, p) in paths.iter().enumerate() {
                    println!("  [{}] {}", i, p.green());
                }
                println!("Installing: {}", paths[0].green());
            }
            paths[0].clone()
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
    let dest = dest_dir_for(&symbols_dir, &symbol_path);
    std::fs::create_dir_all(&dest)?;

    let url = format!("{}/{}", RAW_BASE, symbol_path);

    println!("Symbol : {}", symbol_path.green());
    println!("Dest   : {}", dest.display().to_string().yellow());

    download_symbol(&url, &dest, &symbol_path)?;

    Ok(())
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
