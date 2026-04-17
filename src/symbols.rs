use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Find the volatility3 symbols directory.
/// Checks VOL3_SYMBOLS_DIR env var first, then common installation paths.
pub fn find_symbols_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("VOL3_SYMBOLS_DIR") {
        let path = PathBuf::from(&dir);
        if path.is_dir() {
            return Ok(path);
        }
        anyhow::bail!(
            "VOL3_SYMBOLS_DIR='{}' does not exist or is not a directory",
            dir
        );
    }

    let system_patterns = [
        "/opt/volatility3/lib/python3.*/site-packages/volatility3/symbols",
        "/usr/local/lib/python3.*/site-packages/volatility3/symbols",
        "/usr/lib/python3/dist-packages/volatility3/symbols",
        "/usr/lib/python3.*/dist-packages/volatility3/symbols",
    ];

    for pattern in &system_patterns {
        if let Some(path) = best_glob_match(pattern)? {
            return Ok(path);
        }
    }

    // User pip install
    if let Some(home) = home_dir() {
        let pattern = format!(
            "{}/.local/lib/python3.*/site-packages/volatility3/symbols",
            home.display()
        );
        if let Some(path) = best_glob_match(&pattern)? {
            return Ok(path);
        }
    }

    anyhow::bail!(
        "Could not find volatility3 symbols directory.\n\
         Set the VOL3_SYMBOLS_DIR environment variable to point to it.\n\
         Example: export VOL3_SYMBOLS_DIR=/opt/volatility3/lib/python3.12/site-packages/volatility3/symbols"
    )
}

/// Resolve glob pattern and return the lexicographically last match (highest python3.x).
fn best_glob_match(pattern: &str) -> Result<Option<PathBuf>> {
    let matches: Vec<PathBuf> = glob::glob(pattern)
        .with_context(|| format!("Invalid glob: {}", pattern))?
        .filter_map(|e| e.ok())
        .filter(|p| p.is_dir())
        .collect();

    Ok(matches.into_iter().max_by(|a, b| {
        python_version_key(a)
            .cmp(&python_version_key(b))
            .then_with(|| a.cmp(b))
    }))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Return the destination subdirectory (linux/ or mac/) for a given repo path.
pub fn dest_dir_for(symbols_dir: &Path, repo_path: &str) -> PathBuf {
    let sub = symbol_subdir_for_repo_path(repo_path);
    symbols_dir.join(sub)
}

/// Recursively collect all .json.xz files under the symbols directory.
pub fn list_local_symbols(symbols_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut results = Vec::new();
    collect_xz(symbols_dir, &mut results)?;
    results.sort();
    Ok(results)
}

fn collect_xz(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_xz(&path, out)?;
        } else if is_symbol_file(&path) {
            out.push(path);
        }
    }
    Ok(())
}

fn python_version_key(path: &Path) -> (u32, u32) {
    path.components()
        .filter_map(|c| c.as_os_str().to_str())
        .filter_map(parse_python_component)
        .max()
        .unwrap_or((0, 0))
}

fn parse_python_component(component: &str) -> Option<(u32, u32)> {
    let version = component.strip_prefix("python")?;
    let (major, rest) = parse_leading_u32(version)?;
    let minor = if let Some(after_dot) = rest.strip_prefix('.') {
        parse_leading_u32(after_dot).map(|(n, _)| n).unwrap_or(0)
    } else {
        0
    };
    Some((major, minor))
}

fn parse_leading_u32(input: &str) -> Option<(u32, &str)> {
    let digits_len = input.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits_len == 0 {
        return None;
    }
    let value = input[..digits_len].parse::<u32>().ok()?;
    Some((value, &input[digits_len..]))
}

fn symbol_subdir_for_repo_path(repo_path: &str) -> &'static str {
    let top_level = repo_path
        .split('/')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();

    match top_level.as_str() {
        "mac" | "macos" | "darwin" => "mac",
        "windows" | "win" => "windows",
        _ => "linux",
    }
}

fn is_symbol_file(path: &Path) -> bool {
    let filename = match path.file_name().and_then(|f| f.to_str()) {
        Some(name) => name,
        None => return false,
    };

    filename.ends_with(".json") || filename.ends_with(".json.xz") || filename.ends_with(".json.gz")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_python_component_versions() {
        assert_eq!(parse_python_component("python3"), Some((3, 0)));
        assert_eq!(parse_python_component("python3.12"), Some((3, 12)));
        assert_eq!(parse_python_component("python3.13t"), Some((3, 13)));
        assert_eq!(parse_python_component("python"), None);
        assert_eq!(parse_python_component("py3.12"), None);
    }

    #[test]
    fn extracts_highest_python_version_from_path() {
        let p39 = Path::new("/usr/lib/python3.9/site-packages/volatility3/symbols");
        let p310 = Path::new("/usr/lib/python3.10/site-packages/volatility3/symbols");
        let p312 = Path::new("/usr/lib/python3.12/site-packages/volatility3/symbols");

        assert!(python_version_key(p310) > python_version_key(p39));
        assert!(python_version_key(p312) > python_version_key(p310));
    }

    #[test]
    fn maps_repo_paths_to_symbol_subdirs() {
        assert_eq!(symbol_subdir_for_repo_path("macOS/13/foo.json.xz"), "mac");
        assert_eq!(symbol_subdir_for_repo_path("darwin/22/foo.json.xz"), "mac");
        assert_eq!(
            symbol_subdir_for_repo_path("Windows/ntkrnlmp.json.xz"),
            "windows"
        );
        assert_eq!(
            symbol_subdir_for_repo_path("Debian/amd64/6.1/foo.json.xz"),
            "linux"
        );
    }

    #[test]
    fn detects_supported_symbol_file_extensions() {
        assert!(is_symbol_file(Path::new("foo.json")));
        assert!(is_symbol_file(Path::new("foo.json.xz")));
        assert!(is_symbol_file(Path::new("foo.json.gz")));
        assert!(!is_symbol_file(Path::new("foo.txt")));
    }
}
