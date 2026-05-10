use crate::sources::SymbolEntry;
use colored::Colorize;
use std::collections::BTreeMap;

pub fn render_search_tree(results: &[&SymbolEntry], shown: usize) -> String {
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
}
