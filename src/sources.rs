use anyhow::{Context, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const ABYSS_RAW_BASE: &str = "https://github.com/Abyss-W4tcher/volatility3-symbols/raw/master";

#[derive(Debug, Clone, Copy)]
pub enum IndexFormat {
    PlainPaths,
    RemoteIsf,
    GitTree,
}

#[derive(Debug, Clone, Copy)]
pub struct Source {
    pub id: &'static str,
    pub name: &'static str,
    pub index_url: &'static str,
    pub format: IndexFormat,
}

pub const SOURCES: &[Source] = &[
    Source {
        id: "abyss",
        name: "Abyss-W4tcher/volatility3-symbols",
        index_url: "https://raw.githubusercontent.com/Abyss-W4tcher/volatility3-symbols/master/banners/banners_plain.json",
        format: IndexFormat::PlainPaths,
    },
    Source {
        id: "leludo",
        name: "leludo84/vol3-linux-profiles",
        index_url: "https://raw.githubusercontent.com/leludo84/vol3-linux-profiles/main/banners-isf.json",
        format: IndexFormat::RemoteIsf,
    },
    Source {
        id: "p0d",
        name: "p0dalirius/volatility3-symbols",
        index_url: "https://api.github.com/repos/p0dalirius/volatility3-symbols/git/trees/main?recursive=1",
        format: IndexFormat::GitTree,
    },
];

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct SymbolEntry {
    pub source_id: String,
    pub source_name: String,
    pub banner: String,
    pub path: String,
    pub download_url: String,
}

impl SymbolEntry {
    pub fn display_path(&self) -> String {
        format!("{}:{}", self.source_id, self.path)
    }
}

pub fn fetch_all_sources() -> Result<Vec<SymbolEntry>> {
    let mut entries = Vec::new();
    for source in SOURCES {
        entries.extend(fetch_source(source)?);
    }
    Ok(entries)
}

pub fn fetch_source(source: &Source) -> Result<Vec<SymbolEntry>> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("vol3sm/0.1.0")
        .build()?;
    let resp = client
        .get(source.index_url)
        .send()
        .with_context(|| format!("Failed to fetch {}", source.name))?;
    if !resp.status().is_success() {
        anyhow::bail!("HTTP {} fetching index from {}", resp.status(), source.name);
    }

    match source.format {
        IndexFormat::PlainPaths => {
            let index: HashMap<String, Vec<String>> = resp
                .json()
                .with_context(|| format!("Failed to parse index from {}", source.name))?;
            Ok(index
                .into_iter()
                .flat_map(|(banner, paths)| {
                    paths.into_iter().map(move |path| SymbolEntry {
                        source_id: source.id.to_string(),
                        source_name: source.name.to_string(),
                        banner: banner.clone(),
                        download_url: format!("{}/{}", ABYSS_RAW_BASE, path),
                        path,
                    })
                })
                .collect())
        }
        IndexFormat::RemoteIsf => {
            let index: RemoteIsfIndex = resp.json().with_context(|| {
                format!("Failed to parse remote ISF index from {}", source.name)
            })?;
            Ok(index.into_entries(source))
        }
        IndexFormat::GitTree => {
            let tree: GitTree = resp
                .json()
                .with_context(|| format!("Failed to parse Git tree from {}", source.name))?;
            Ok(tree
                .tree
                .into_iter()
                .filter(|item| item.item_type == "blob" && is_symbol_path(&item.path))
                .map(|item| SymbolEntry {
                    source_id: source.id.to_string(),
                    source_name: source.name.to_string(),
                    banner: String::new(),
                    download_url: format!(
                        "https://raw.githubusercontent.com/p0dalirius/volatility3-symbols/main/{}",
                        item.path
                    ),
                    path: item.path,
                })
                .collect())
        }
    }
}

#[derive(Debug, Deserialize)]
struct RemoteIsfIndex {
    #[allow(dead_code)]
    version: Option<u64>,
    #[serde(default)]
    linux: HashMap<String, Vec<String>>,
    #[serde(default)]
    mac: HashMap<String, Vec<String>>,
    #[serde(default)]
    windows: HashMap<String, Vec<String>>,
}

impl RemoteIsfIndex {
    fn into_entries(self, source: &Source) -> Vec<SymbolEntry> {
        let mut entries = Vec::new();
        entries.extend(remote_bucket_entries(source, self.linux));
        entries.extend(remote_bucket_entries(source, self.mac));
        entries.extend(remote_bucket_entries(source, self.windows));
        entries
    }
}

#[derive(Debug, Deserialize)]
struct GitTree {
    tree: Vec<GitTreeItem>,
}

#[derive(Debug, Deserialize)]
struct GitTreeItem {
    path: String,
    #[serde(rename = "type")]
    item_type: String,
}

fn remote_bucket_entries(
    source: &Source,
    bucket: HashMap<String, Vec<String>>,
) -> Vec<SymbolEntry> {
    bucket
        .into_iter()
        .filter_map(|(encoded_banner, urls)| {
            decode_banner(&encoded_banner)
                .ok()
                .map(|banner| (banner, urls))
        })
        .flat_map(|(banner, urls)| {
            urls.into_iter().map(move |url| SymbolEntry {
                source_id: source.id.to_string(),
                source_name: source.name.to_string(),
                path: display_path_from_url(&url),
                download_url: url,
                banner: banner.clone(),
            })
        })
        .collect()
}

fn decode_banner(encoded: &str) -> Result<String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .context("Invalid base64 banner")?;
    let banner = String::from_utf8_lossy(&bytes)
        .trim_end_matches('\0')
        .trim_end()
        .to_string();
    Ok(banner)
}

fn display_path_from_url(url: &str) -> String {
    if let Some((_, path)) = url.split_once("/main/") {
        return path.to_string();
    }
    if let Some((_, path)) = url.split_once("/master/") {
        return path.to_string();
    }
    url.rsplit('/').next().unwrap_or(url).to_string()
}

fn is_symbol_path(path: &str) -> bool {
    path.ends_with(".json") || path.ends_with(".json.xz") || path.ends_with(".json.gz")
}

#[cfg(test)]
mod tests {
    use super::{decode_banner, display_path_from_url, is_symbol_path};

    #[test]
    fn decodes_remote_isf_banner() {
        let encoded = "TGludXggdmVyc2lvbiA2LjEuMAoA";

        assert_eq!(decode_banner(encoded).unwrap(), "Linux version 6.1.0");
    }

    #[test]
    fn derives_readable_path_from_raw_github_url() {
        let url = "https://raw.githubusercontent.com/leludo84/vol3-linux-profiles/main/profiles/ubuntu24/foo.json.xz";

        assert_eq!(display_path_from_url(url), "profiles/ubuntu24/foo.json.xz");
    }

    #[test]
    fn detects_symbol_paths() {
        assert!(is_symbol_path("symbols/Ubuntu/foo.json.xz"));
        assert!(is_symbol_path("windows/foo.json"));
        assert!(!is_symbol_path("symbols/Ubuntu/banner.txt"));
    }
}
