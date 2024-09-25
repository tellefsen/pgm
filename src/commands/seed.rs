use std::{io::Write, path::Path, process::Command};

use anyhow::{Context, Result};
use tempfile::NamedTempFile;

fn process_seed_directory(full_dir_path: &str) -> Result<String> {
    let mut entries: Vec<_> = std::fs::read_dir(full_dir_path)?
        .filter_map(|entry| entry.ok())
        .collect();
    
    entries.sort_by_key(|entry| entry.path());

    let mut compiled_content = String::new();
    for entry in entries {
        let path = entry.path();
        if path.is_file() && path.extension().map_or(false, |ext| ext == "sql") {
            let content = std::fs::read_to_string(&path)?;

            let file_name = path.file_stem().unwrap().to_str().unwrap();

            let file_path = format!("{}/{}", full_dir_path, file_name);
            compiled_content.push_str(&format!(
                "-- RUN {file_path} --
{content}
RAISE NOTICE '✅ Applied seed: {file_name}';
-- DONE {file_path} --
"
            ));
        }
    }
    Ok(compiled_content)
}

fn execute_sql(sql: &str) -> Result<()> {
    // Check if psql exists
    if !Command::new("psql").arg("--version").output().is_ok() {
        return Err(anyhow::anyhow!(
            "psql not found. Please ensure it is installed and in your PATH."
        ));
    }

    // Create a temporary file
    let mut temp_file = NamedTempFile::new().context("Failed to create temporary file")?;
    temp_file
        .write_all(sql.as_bytes())
        .context("Failed to write SQL to temporary file")?;

    // Construct the psql command
    let mut command = Command::new("psql");
    command.args(&[
        "-f",
        temp_file.path().to_str().unwrap(),
        "-v",
        "ON_ERROR_STOP=1",
    ]);

    let output = command.output().context("Failed to execute psql command")?;
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Process stderr to remove prefix 'psql:/path/to/temp/file:1234: '
    stderr.lines().for_each(|line| {
        println!("{}", line.split_once(": ").map_or(line, |(_, rest)| rest));
    });

    if output.status.success() {
        Ok(())
    } else {
        let exit_code = output.status.code().unwrap_or(-1);
        Err(anyhow::anyhow!(
            "psql command failed with exit code: {}",
            exit_code
        ))
    }
}

pub fn seed(pgm_dir_path: &str) -> Result<()> {
    if !Path::new(pgm_dir_path).is_dir() {
        return Err(anyhow::anyhow!(
            "Directory '{}' not found. Have you run 'pgm init'?",
            pgm_dir_path
        ));
    }
    let seeds_dir = format!("{}/seeds", pgm_dir_path);
    let seeds_dir = seeds_dir.as_str();
    let mut compiled_content = String::new();
    compiled_content.push_str("DO $pgm_seed$ BEGIN ");
    compiled_content.push_str("SET LOCAL client_min_messages=NOTICE;");
    compiled_content
        .push_str(&process_seed_directory(seeds_dir).context("Failed to process seed directory")?);
    compiled_content.push_str("END $pgm_seed$;");

    execute_sql(&compiled_content).context("Failed to execute seed SQL")?;
    Ok(())
}
