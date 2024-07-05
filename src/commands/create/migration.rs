use std::path::Path;
use anyhow::{Result, Context};

pub fn create_migration(pgm_dir_path: &str) -> Result<()> {
    if !Path::new(pgm_dir_path).exists() {
        return Err(anyhow::anyhow!(
            "Directory '{}' not found. Have you run 'pgm init'?",
            pgm_dir_path
        ));
    }

    let migrations_dir = format!("{}/migrations", pgm_dir_path);
    let migrations_dir = migrations_dir.as_str();
    let last_migration_file = std::fs::read_dir(migrations_dir)?
        .filter_map(|entry| entry.ok())
        .max_by_key(|entry| entry.file_name());
    let last_migration_number = last_migration_file.map_or(0, |entry| {
        entry
            .file_name()
            .to_str()
            .and_then(|s| s.split('.').next())
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(0)
    });
    let next_migration_number = format!("{:05}", last_migration_number + 1);
    let next_migration_file = format!("{}/{}.sql", migrations_dir, next_migration_number);
    std::fs::create_dir_all(migrations_dir).context("Failed to create migrations directory")?;
    std::fs::write(next_migration_file, "").context("Failed to create migration file")?;
    Ok(())
}