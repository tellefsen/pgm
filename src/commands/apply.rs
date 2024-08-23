use anyhow::Result;
use md5;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use tempfile::NamedTempFile;

use crate::INITIAL_MIGRATION_FILE_NAME;

pub fn apply(pgm_dir_path: &str, dry_run: bool, fake: bool) -> Result<()> {
    // Compile the SQL
    let sql = if fake {
        build_fake(pgm_dir_path).expect("Failed to compile fake SQL")
    } else {
        build(pgm_dir_path, !dry_run).expect("Failed to compile SQL")
    };

    // Print the SQL and exit if dry-run
    if dry_run {
        println!("{}", sql);
        return Ok(());
    } else {
        execute_sql(&sql)
    }
}

fn execute_sql(sql: &str) -> Result<()> {
    // Check if psql exists
    if !Command::new("psql").arg("--version").output().is_ok() {
        return Err(anyhow::anyhow!(
            "psql not found. Please ensure it is installed and in your PATH."
        ));
    }

    // Create a temporary file
    let mut temp_file = NamedTempFile::new().expect("Failed to create temporary file");
    temp_file
        .write_all(sql.as_bytes())
        .expect("Failed to write SQL to temporary file");

    // Construct the psql command
    let mut command = Command::new("psql");
    command.args(&[
        "-f",
        temp_file.path().to_str().unwrap(),
        "-v",
        "ON_ERROR_STOP=1",
    ]);

    let output = command.output().expect("Failed to execute psql command");
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

fn pgm_tables_create_sql() -> String {
    String::from(
        r#"
-- Create tables if they don't exist
CREATE TABLE IF NOT EXISTS pgm_migration (
    name TEXT PRIMARY KEY,
    applied_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS pgm_function (
    name TEXT PRIMARY KEY,
    hash TEXT NOT NULL,
    applied_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS pgm_trigger (
    name TEXT PRIMARY KEY,
    hash TEXT NOT NULL,
    applied_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS pgm_view (
    name TEXT PRIMARY KEY,
    hash TEXT NOT NULL,
    applied_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);
"#,
    )
}

fn build(pgm_dir_path: &str, minify: bool) -> Result<String> {
    // Check if the postgres directory exists
    if !Path::new(pgm_dir_path).is_dir() {
        return Err(anyhow::anyhow!(
            "Directory '{}' not found. Have you run 'pgm init'?",
            pgm_dir_path
        ));
    }

    let mut compiled_content = String::new();

    // Start the main DO block
    compiled_content.push_str("DO $pgm$ BEGIN\n");
    compiled_content.push_str("SET LOCAL check_function_bodies = false;\n");
    compiled_content.push_str("SET LOCAL client_min_messages = notice;\n");

    // Add schema creation with existence check
    compiled_content.push_str(&pgm_tables_create_sql());

    let functions_dir = format!("{}/functions", pgm_dir_path);
    let triggers_dir = format!("{}/triggers", pgm_dir_path);
    let views_dir = format!("{}/views", pgm_dir_path);
    let migrations_dir = format!("{}/migrations", pgm_dir_path);

    // Process initial migration if it exists
    let initial_migration_file = Path::new(&migrations_dir).join(INITIAL_MIGRATION_FILE_NAME);
    if initial_migration_file.exists() {
        compiled_content.push_str(&process_migration(&initial_migration_file)?);
    }

    // Process functions if directory exists
    if Path::new(&functions_dir).is_dir() {
        compiled_content.push_str(&process_directory(&functions_dir, "pgm_function", false)?);
    }
    // Process triggers if directory exists
    if Path::new(&triggers_dir).is_dir() {
        compiled_content.push_str(&process_directory(&triggers_dir, "pgm_trigger", false)?);
    }

    // Process migrations if directory exists
    if Path::new(&migrations_dir).is_dir() {
        let mut migration_files: Vec<_> = std::fs::read_dir(&migrations_dir)?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry.path().is_file() && entry.path().extension().map_or(false, |ext| ext == "sql")
            })
            // filter out initial migration file
            .filter(|entry| {
                entry.path().file_name().expect("Filename must exist")
                    != INITIAL_MIGRATION_FILE_NAME
            })
            .collect();
        migration_files.sort_by_key(|entry| entry.file_name());

        for file in migration_files {
            compiled_content
                .push_str(&process_migration(&file.path()).expect("Failed to process migration"));
        }
    }

    // Process views if directory exists
    if Path::new(&views_dir).is_dir() {
        compiled_content.push_str(
            &process_directory(&views_dir, "pgm_view", true).expect("Failed to process views"),
        );
    }

    // Check function bodies
    compiled_content.push_str("SET LOCAL check_function_bodies = true;\n");
    if Path::new(&functions_dir).is_dir() {
        compiled_content.push_str(
            &process_directory(&functions_dir, "pgm_function", true)
                .expect("Failed to process functions"),
        );
    }
    if Path::new(&triggers_dir).is_dir() {
        compiled_content.push_str(
            &process_directory(&triggers_dir, "pgm_trigger", true)
                .expect("Failed to process triggers"),
        );
    }

    // End the main DO block
    compiled_content.push_str("END $pgm$;\n");

    // Remove empty lines
    compiled_content = compiled_content
        .lines()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    // Remove comments
    if minify {
        compiled_content = compiled_content
            .lines()
            .filter(|line| !line.starts_with("--"))
            .collect::<Vec<_>>()
            .join("\n");
    }

    Ok(compiled_content)
}

fn process_directory(full_dir_path: &str, table: &str, update_table_hash: bool) -> Result<String> {
    let mut compiled_content = String::new();
    for entry in std::fs::read_dir(full_dir_path)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension().map_or(false, |ext| ext == "sql") {
            let content = std::fs::read_to_string(&path)?;

            let hash = format!("{:x}", md5::compute(&content));
            let file_name = path.file_stem().unwrap().to_str().unwrap();

            let file_path = format!("{}/{}", full_dir_path, file_name);

            let update_hash_query = if update_table_hash {
                format!(
                    "
    INSERT INTO {table} (name, hash) VALUES ('{file_name}', '{hash}') ON CONFLICT (name) DO UPDATE SET hash = EXCLUDED.hash, applied_at = CURRENT_TIMESTAMP;
    RAISE NOTICE '✅ Applied {file_path}';
ELSE
    RAISE NOTICE '- Skipped {file_path} (no changes)';"
                )
            } else {
                String::new()
            };

            compiled_content.push_str(&format!(
                "-- RUN {file_path} --
IF (SELECT hash FROM {table} WHERE name = '{file_name}') IS DISTINCT FROM '{hash}' THEN
{content}
{update_hash_query}
END IF;
-- DONE {file_path} --
"
            ));
        }
    }
    Ok(compiled_content)
}

fn process_migration(path: &Path) -> Result<String> {
    let mut compiled_content = String::new();

    let content = std::fs::read_to_string(path)?;

    let file_name = path.file_stem().unwrap().to_str().unwrap();
    let path_with_extension = path
        .file_name()
        .expect("File name should exist")
        .to_str()
        .expect("Should be a string");

    compiled_content.push_str(&format!(
        "-- RUN {path_with_extension} --
IF NOT EXISTS (SELECT 1 FROM pgm_migration WHERE name = '{file_name}') THEN
{content}
INSERT INTO pgm_migration (name) VALUES ('{file_name}');
RAISE NOTICE '✅ Applied migration: {file_name}';
ELSE
RAISE NOTICE '- Skipped migration: {file_name} (already applied)';
END IF;
-- DONE {path_with_extension} --
"
    ));

    Ok(compiled_content)
}

fn build_fake(pgm_dir_path: &str) -> Result<String> {
    // Check if the postgres directory exists
    if !Path::new(pgm_dir_path).is_dir() {
        return Err(anyhow::anyhow!(
            "Directory '{}' not found. Have you run 'pgm init'?",
            pgm_dir_path
        ));
    }

    let mut compiled_content = String::new();

    // Start the main DO block
    compiled_content.push_str("DO $pgm$ BEGIN\n");

    compiled_content.push_str(&pgm_tables_create_sql());

    // Process functions if directory exists
    if Path::new(&format!("{}/functions", pgm_dir_path)).is_dir() {
        let functions_content =
            process_directory_fake(&format!("{}/functions", pgm_dir_path), "pgm_function")
                .expect("Failed to process functions");
        compiled_content.push_str(&functions_content);
    }

    // Process triggers if directory exists
    if Path::new(&format!("{}/triggers", pgm_dir_path)).is_dir() {
        let triggers_content =
            process_directory_fake(&format!("{}/triggers", pgm_dir_path), "pgm_trigger")
                .expect("Failed to process triggers");
        compiled_content.push_str(&triggers_content);
    }

    // Process views if directory exists
    if Path::new(&format!("{}/views", pgm_dir_path)).is_dir() {
        let views_content = process_directory_fake(&format!("{}/views", pgm_dir_path), "pgm_view")
            .expect("Failed to process views");
        compiled_content.push_str(&views_content);
    }

    // Process migrations if directory exists
    if Path::new(&format!("{}/migrations", pgm_dir_path)).is_dir() {
        let migrations_content =
            process_migrations_fake(pgm_dir_path).expect("Failed to process migrations");
        compiled_content.push_str(&migrations_content);
    }

    // End the main DO block
    compiled_content.push_str("END $pgm$;\n");

    Ok(compiled_content)
}

fn process_directory_fake(full_dir_path: &str, table: &str) -> Result<String> {
    let mut compiled_content = String::new();
    for entry in std::fs::read_dir(full_dir_path)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension().map_or(false, |ext| ext == "sql") {
            let content = std::fs::read_to_string(&path)?;
            let hash = format!("{:x}", md5::compute(&content));
            let file_name = path.file_stem().unwrap().to_str().unwrap();

            compiled_content.push_str(&format!(
                "-- Fake apply {table} '{file_name}'
INSERT INTO {table} (name, hash) VALUES ('{file_name}', '{hash}') 
                ON CONFLICT (name) DO UPDATE SET hash = EXCLUDED.hash, applied_at = CURRENT_TIMESTAMP;
                RAISE NOTICE '✅ Fake applied: {table} - {file_name}';\n"
            ));
        }
    }
    Ok(compiled_content)
}

fn process_migrations_fake(pgm_dir_path: &str) -> Result<String> {
    let migrations_dir = format!("{}/migrations", pgm_dir_path);
    let migrations_dir = migrations_dir.as_str();
    let mut migration_files: Vec<_> = std::fs::read_dir(migrations_dir)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.path().is_file() && entry.path().extension().map_or(false, |ext| ext == "sql")
        })
        .collect();

    // Sort the migration files
    migration_files.sort_by_key(|entry| entry.file_name());

    let mut compiled_content = String::new();
    for entry in migration_files {
        let path = entry.path();
        let file_name = path.file_stem().unwrap().to_str().unwrap();
        compiled_content.push_str(&format!(
            "-- Fake apply migration '{file_name}'
INSERT INTO pgm_migration (name) VALUES ('{file_name}') ON CONFLICT (name) DO NOTHING;
            RAISE NOTICE '✅ Fake applied migration: {file_name}';\n"
        ));
    }
    Ok(compiled_content)
}
