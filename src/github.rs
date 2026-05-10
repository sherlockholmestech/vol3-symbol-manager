use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::io::{Read, Write};
use std::path::Path;

pub fn download_symbol(url: &str, dest_dir: &Path, symbol_path: &str) -> Result<()> {
    let filename = Path::new(symbol_path)
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine filename from: {}", symbol_path))?;
    let dest_file = dest_dir.join(filename);

    if dest_file.exists() {
        println!("Already installed: {}", dest_file.display());
        return Ok(());
    }

    let client = reqwest::blocking::Client::builder()
        .user_agent("vol3sm/0.1.0")
        .build()?;
    let mut resp = client
        .get(url)
        .send()
        .with_context(|| format!("Failed to download {}", url))?;

    if !resp.status().is_success() {
        anyhow::bail!("HTTP {} downloading symbol", resp.status());
    }

    let pb = match resp.content_length() {
        Some(n) => {
            let pb = ProgressBar::new(n);
            pb.set_style(
                ProgressStyle::default_bar()
                    .template(
                        "  {spinner:.green} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})",
                    )?
                    .progress_chars("=>-"),
            );
            pb
        }
        None => {
            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template("  {spinner:.green} {bytes} downloaded")?,
            );
            pb
        }
    };

    // Write to a .part file, rename on success
    let tmp = dest_dir.join(format!("{}.part", filename.to_string_lossy()));
    {
        let mut file =
            fs::File::create(&tmp).with_context(|| format!("Cannot create {}", tmp.display()))?;
        let mut buf = [0u8; 16384];
        loop {
            let n = resp.read(&mut buf).context("Read error during download")?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])
                .context("Write error during download")?;
            pb.inc(n as u64);
        }
    }
    pb.finish_with_message("done");

    fs::rename(&tmp, &dest_file)
        .with_context(|| format!("Failed to move {} → {}", tmp.display(), dest_file.display()))?;

    println!("Installed: {}", dest_file.display());
    Ok(())
}
