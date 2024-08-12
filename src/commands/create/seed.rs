use anyhow::{Context, Result};
use std::path::Path;
use std::fs;

pub fn create_seed(pgm_dir_path: &str) -> Result<()> {
    if !Path::new(pgm_dir_path).exists() {
        return Err(anyhow::anyhow!(
            "Directory '{}' not found. Have you run 'pgm init'?",
            pgm_dir_path
        ));
    }

    let seeds_dir = format!("{}/seeds", pgm_dir_path);
    let seeds_dir = seeds_dir.as_str();

    // Create seeds directory if it doesn't exist
    fs::create_dir_all(seeds_dir).context("Failed to create seeds directory")?;

    let last_seed_file = fs::read_dir(seeds_dir)?
        .filter_map(|entry| entry.ok())
        .max_by_key(|entry| entry.file_name());
    let last_seed_number = last_seed_file.map_or(0, |entry| {
        entry
            .file_name()
            .to_str()
            .and_then(|s| s.split('.').next())
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(0)
    });
    let next_seed_number = format!("{:05}", last_seed_number + 1);
    let next_seed_file = format!("{}/{}.sql", seeds_dir, next_seed_number);
    std::fs::write(next_seed_file, "").context("Failed to create seed file")?;
    Ok(())
}