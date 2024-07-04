use anyhow::{Context, Result};
use clap::{Arg, Command};
use md5;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command as ProcessCommand;
use tempfile::NamedTempFile;

const DEFAULT_PGM_PATH: &str = "postgres";
const INITIAL_MIGRATION_FILE_NAME: &str = "00000.sql";

fn get_initial_migration_from_db() -> Result<NamedTempFile> {
    // Create temporary file for schema dump
    let schema_dump_file =
        NamedTempFile::new().context("Failed to create temporary file for schema dump")?;

    let mut child = match ProcessCommand::new("pg_dump")
        .args(&[
            "-f",
            schema_dump_file.path().to_str().unwrap(),
            "--no-owner",
            "--schema-only",
        ])
        .spawn()
    {
        Ok(child) => child,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(anyhow::anyhow!(
                "pg_dump not found. Please ensure it is installed and in your PATH."
            ));
        }
        Err(e) => return Err(anyhow::anyhow!("Failed to spawn pg_dump: {}", e)),
    };

    let exit_status = child.wait().context("Failed to wait for pg_dump")?;
    if !exit_status.success() {
        return Err(anyhow::anyhow!("pg_dump failed"));
    }

    // HACK: replace SELECT pg_catalog.set_config('search_path', '', false);
    let schema_dump_file_content = std::fs::read_to_string(schema_dump_file.path())?;
    let modified_content = schema_dump_file_content
        .replace("SELECT pg_catalog.set_config('search_path', '', false);", "PERFORM pg_catalog.set_config('search_path', '', false);");
    std::fs::write(schema_dump_file.path(), modified_content)?;

    Ok(schema_dump_file)
}

fn get_triggers_from_db() -> Result<Vec<(String, String)>> {
    let function_names = ProcessCommand::new("psql")
        .args(&[
            "-t",
            "-c",
            "SELECT proname AS function_name
             FROM pg_proc p
             JOIN pg_namespace n ON p.pronamespace = n.oid
             LEFT JOIN pg_depend d ON d.objid = p.oid AND d.deptype = 'e'
             WHERE 
                n.nspname NOT IN ('pg_catalog', 'information_schema')
                AND p.prokind = 'f' 
                AND d.objid IS NULL
                AND EXISTS (
                    SELECT 1
                    FROM pg_trigger t
                    WHERE t.tgfoid = p.oid
                )
             ORDER BY function_name;",
        ])
        .output()
        .context("Failed to execute psql command to get function names")?;
    let function_names = String::from_utf8(function_names.stdout)
        .context("Failed to convert function names output to UTF-8")?
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect::<Vec<String>>();

    let processes = function_names.iter().map(|name| {
        ProcessCommand::new("psql")
            .args(&[
                "-t",
                "-A",
                "-c",
                &format!(
                    "SELECT pg_get_functiondef(p.oid) AS function_definition
                     FROM pg_proc p
                     JOIN pg_namespace n ON p.pronamespace = n.oid
                     WHERE n.nspname = 'public' AND p.proname = '{}';",
                    name
                ),
            ])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .context(format!(
                "Failed to spawn psql command for function '{}'",
                name
            ))
    });

    let function_contents = processes
        .map(|process| {
            process.and_then(|child| {
                child
                    .wait_with_output()
                    .context("Failed to wait for psql command output")
            })
        })
        .map(|output| {
            output.map(|o| {
                let content = String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .map(|line| line.trim_end())
                    .collect::<Vec<_>>()
                    .join("\n");
                let content = content.trim_end();
                format!("{content};")
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to collect function contents")?;

    // Combine function names and contents
    let functions = function_names
        .into_iter()
        .zip(function_contents)
        .collect::<Vec<_>>();

    Ok(functions)
}

fn get_functions_from_db() -> Result<Vec<(String, String)>> {
    let function_names = ProcessCommand::new("psql")
        .args(&[
            "-t",
            "-c",
            "SELECT DISTINCT proname AS function_name
             FROM pg_proc p
             JOIN pg_namespace n ON p.pronamespace = n.oid
             LEFT JOIN pg_depend d ON d.objid = p.oid AND d.deptype = 'e'
             WHERE 
                n.nspname NOT IN ('pg_catalog', 'information_schema')
                AND p.prokind = 'f' 
                AND d.objid IS NULL
                AND NOT EXISTS (
                    SELECT 1
                    FROM pg_trigger t
                    WHERE t.tgfoid = p.oid
                )
             ORDER BY function_name;",
        ])
        .output()
        .context("Failed to execute psql command to get function names")?;
    let function_names = String::from_utf8(function_names.stdout)
        .context("Failed to convert function names output to UTF-8")?
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect::<Vec<String>>();

    let processes = function_names.iter().map(|name| {
        ProcessCommand::new("psql")
            .args(&[
                "-t",
                "-A",
                "-c",
                &format!(
                    "SELECT RTRIM(pg_get_functiondef(p.oid), E'\n') || ';\n' AS function_definition
                     FROM pg_proc p
                     JOIN pg_namespace n ON p.pronamespace = n.oid
                     WHERE n.nspname = 'public' AND p.proname = '{}';",
                    name
                ),
            ])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .context(format!(
                "Failed to spawn psql command for function '{}'",
                name
            ))
    });

    let function_contents = processes
        .map(|process| {
            process.and_then(|child| {
                child
                    .wait_with_output()
                    .context("Failed to wait for psql command output")
            })
        })
        .map(|output| {
            output.map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .map(|line| line.trim_end())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to collect function contents")?;

    // Combine function names and contents
    let functions = function_names
        .into_iter()
        .zip(function_contents)
        .collect::<Vec<_>>();

    Ok(functions)
}

fn get_views_from_db() -> Result<Vec<(String, String)>> {
    let view_names = ProcessCommand::new("psql")
        .args(&[
            "-t",
            "-c",
            "SELECT c.relname AS view_name
            FROM pg_class c
            JOIN pg_namespace n ON n.oid = c.relnamespace
            LEFT JOIN pg_depend d ON d.objid = c.oid AND d.deptype = 'e'
            WHERE c.relkind = 'v'
              AND n.nspname NOT IN ('pg_catalog', 'information_schema')
              AND d.objid IS NULL 
              AND c.relname NOT LIKE 'pg_%'
            ORDER BY c.relname;",
        ])
        .output()
        .context("Failed to execute psql command to get view names")?;
    let view_names = String::from_utf8(view_names.stdout)
        .context("Failed to convert view names output to UTF-8")?
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect::<Vec<String>>();

    let processes = view_names.iter().map(|name| {
        ProcessCommand::new("psql")
            .args(&[
                "-t",
                "-A",
                "-c",
                &format!("SELECT pg_get_viewdef('{}') AS view_definition;", name),
            ])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .context(format!("Failed to spawn psql command for view '{}'", name))
    });

    let view_contents = processes
        .map(|process| {
            process.and_then(|child| {
                child
                    .wait_with_output()
                    .context("Failed to wait for psql command output")
            })
        })
        .map(|output| {
            output.map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .map(|line| line.trim_end())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to collect view contents")?;

    // Combine view names and contents
    let views = view_names
        .into_iter()
        .zip(view_contents)
        .map(|(name, content)| {
            let view_definition = format!("CREATE OR REPLACE VIEW {name} AS\n{content}");
            (name, view_definition)
        })
        .collect::<Vec<_>>();
    Ok(views)
}

fn initialize(pgm_dir_path: &str, existing_db: bool) -> Result<()> {
    if Path::new(pgm_dir_path).exists() {
        return Err(anyhow::anyhow!(
            "Directory '{}' already exists",
            pgm_dir_path
        ));
    }

    if existing_db {
        // Call get_initial_migration_from_db to get schema-only dump
        let initial_migration_file = get_initial_migration_from_db()?;

        // Get functions from the database
        let functions = get_functions_from_db()?;

        // Get triggers from the database
        let triggers = get_triggers_from_db()?;

        // Get views from the database
        let views = get_views_from_db()?;

        // Create directory structure
        create_directory_structure(pgm_dir_path)?;

        // Copy schema dump to migrations directory
        let migrations_dir = Path::new(pgm_dir_path).join("migrations");
        std::fs::copy(
            initial_migration_file,
            migrations_dir.join(INITIAL_MIGRATION_FILE_NAME),
        )
        .context("Failed to copy schema dump to migrations directory")?;

        // Write all function to functions directory
        let functions_dir = Path::new(pgm_dir_path).join("functions");
        for (name, content) in functions {
            let function_file = functions_dir.join(format!("{}.sql", name));
            std::fs::write(function_file, content)
                .context(format!("Failed to write function '{}' to file", name))?;
        }

        // Write all triggers to triggers directory
        let triggers_dir = Path::new(pgm_dir_path).join("triggers");
        for (name, content) in triggers {
            let trigger_file = triggers_dir.join(format!("{}.sql", name));
            std::fs::write(trigger_file, content)
                .context(format!("Failed to write trigger '{}' to file", name))?;
        }

        // Write all views to views directory
        let views_dir = Path::new(pgm_dir_path).join("views");
        for (name, content) in views {
            let view_file = views_dir.join(format!("{}.sql", name));
            std::fs::write(view_file, content)
                .context(format!("Failed to write view '{}' to file", name))?;
        }
    } else {
        // Create directory structure without using pg_dump
        create_directory_structure(pgm_dir_path)?;
    }

    Ok(())
}

fn create_directory_structure(pgm_dir_path: &str) -> Result<()> {
    std::fs::create_dir_all(pgm_dir_path).context("Failed to create directory")?;
    std::fs::create_dir_all(format!("{}/migrations", pgm_dir_path))
        .context("Failed to create migrations directory")?;
    std::fs::create_dir_all(format!("{}/triggers", pgm_dir_path))
        .context("Failed to create triggers directory")?;
    std::fs::create_dir_all(format!("{}/views", pgm_dir_path))
        .context("Failed to create views directory")?;
    std::fs::create_dir_all(format!("{}/functions", pgm_dir_path))
        .context("Failed to create functions directory")?;
    Ok(())
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

    // Add schema creation with existence check
    compiled_content.push_str(
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
    );

    let functions_dir = format!("{}/functions", pgm_dir_path);
    let triggers_dir = format!("{}/triggers", pgm_dir_path);
    let views_dir = format!("{}/views", pgm_dir_path);
    let migrations_dir = format!("{}/migrations", pgm_dir_path);

    let initial_migration_file =
        Path::new(migrations_dir.as_str()).join(INITIAL_MIGRATION_FILE_NAME);

    let mut migration_files: Vec<_> = std::fs::read_dir(migrations_dir.as_str())?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.path().is_file() && entry.path().extension().map_or(false, |ext| ext == "sql")
        })
        // filter out initial migration file
        .filter(|entry| {
            entry.path().file_name().expect("Filename must exist") != INITIAL_MIGRATION_FILE_NAME
        })
        .collect();
    migration_files.sort_by_key(|entry| entry.file_name());

    // Always execute the initial migrations first
    compiled_content.push_str(&process_migration(&initial_migration_file)?);

    // Add all the content to the compiled SQL file, the hash is not updated here because we will update it below (where we also check the function body)
    compiled_content.push_str(&process_directory(&functions_dir, "pgm_function", false)?);
    compiled_content.push_str(&process_directory(&triggers_dir, "pgm_trigger", false)?);

    // Add the migrations and views
    for file in migration_files {
        compiled_content.push_str(&process_migration(&file.path())?);
    }
    compiled_content.push_str(&process_directory(&views_dir, "pgm_view", true)?);

    // At this point we already know that tables/functions/triggers/views are created
    // However we must check the function bodies (since the check was turned off above)
    compiled_content.push_str("SET LOCAL check_function_bodies = true;\n");
    compiled_content.push_str(&process_directory(&functions_dir, "pgm_function", true)?);
    compiled_content.push_str(&process_directory(&triggers_dir, "pgm_trigger", true)?);

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
    RAISE NOTICE 'Applied {file_path}';
ELSE
    RAISE NOTICE 'Skipped {file_path} (no changes)';"
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
RAISE NOTICE 'Applied migration: {file_name}';
ELSE
RAISE NOTICE 'Skipped migration: {file_name} (already applied)';
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

    let functions_content =
        process_directory_fake(&format!("{}/functions", pgm_dir_path), "pgm_function")?;
    let triggers_content =
        process_directory_fake(&format!("{}/triggers", pgm_dir_path), "pgm_trigger")?;
    let views_content = process_directory_fake(&format!("{}/views", pgm_dir_path), "pgm_view")?;
    let migrations_content = process_migrations_fake(pgm_dir_path)?;

    // Process directories and migrations, but only update pgm_ tables
    compiled_content.push_str(&functions_content);
    compiled_content.push_str(&triggers_content);
    compiled_content.push_str(&views_content);
    compiled_content.push_str(&migrations_content);

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
                RAISE NOTICE 'Fake applied: {table} - {file_name}';\n"
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
            RAISE NOTICE 'Fake applied migration: {file_name}';\n"
        ));
    }
    Ok(compiled_content)
}

fn apply(pgm_dir_path: &str, dry_run: bool, fake: bool) -> Result<()> {
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
    if !ProcessCommand::new("psql")
        .arg("--version")
        .output()
        .is_ok()
    {
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
    let mut command = ProcessCommand::new("psql");
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

fn create_function(pgm_dir_path: &str, name: &str) -> Result<()> {
    if !Path::new(pgm_dir_path).exists() {
        return Err(anyhow::anyhow!(
            "Directory '{}' not found. Have you run 'pgm init'?",
            pgm_dir_path
        ));
    }

    let file_path = Path::new(pgm_dir_path).join(format!("{}.sql", name));
    std::fs::create_dir_all(&file_path).context("Failed to create functions directory")?;

    if file_path.exists() {
        print!(
            "Function '{}' already exists. Do you want to reset it? (y/N): ",
            name
        );
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Function creation aborted.");
            return Ok(());
        }
    }

    let template = std::fs::read_to_string("./src/templates/function.sql")
        .context("Failed to read function template")?;
    let content = template.replace("<name_placeholder>", name);
    std::fs::write(file_path, content).context("Failed to create function file")?;

    println!("Function '{}' created successfully", name);
    Ok(())
}

fn create_migration(pgm_dir_path: &str) -> Result<()> {
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

fn create_trigger(pgm_dir_path: &str, name: &str) -> Result<()> {
    if !Path::new(pgm_dir_path).exists() {
        return Err(anyhow::anyhow!(
            "Directory '{}' not found. Have you run 'pgm init'?",
            pgm_dir_path
        ));
    }

    let file_path = Path::new(pgm_dir_path).join(format!("{}.sql", name));
    std::fs::create_dir_all(&file_path).context("Failed to create triggers directory")?;

    if file_path.exists() {
        print!(
            "Trigger '{}' already exists. Do you want to reset it? (y/N): ",
            name
        );
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Trigger creation aborted.");
            return Ok(());
        }
    }

    let template = std::fs::read_to_string("./src/templates/trigger_function.sql")
        .context("Failed to read trigger template")?;
    let content = template.replace("<name_placeholder>", name);
    std::fs::write(file_path, content).context("Failed to create trigger file")?;

    println!("Trigger '{}' created successfully", name);
    Ok(())
}

fn create_view(pgm_dir_path: &str, name: &str) -> Result<()> {
    if !Path::new(pgm_dir_path).exists() {
        return Err(anyhow::anyhow!(
            "Directory '{}' not found. Have you run 'pgm init'?",
            pgm_dir_path
        ));
    }

    let views_dir = Path::new(pgm_dir_path).join("views");
    std::fs::create_dir_all(&views_dir).context("Failed to create views directory")?;

    let file_path = views_dir.join(format!("{}.sql", name));

    if file_path.exists() {
        print!(
            "View '{}' already exists. Do you want to reset it? (y/N): ",
            name
        );
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        if !input.trim().eq_ignore_ascii_case("y") {
            println!("View creation aborted.");
            return Ok(());
        }
    }

    let template_name = "./src/templates/view.sql";

    let template =
        std::fs::read_to_string(template_name).context("Failed to read view template")?;

    let content = template.replace("<name_placeholder>", name);
    std::fs::write(file_path, content).context("Failed to create view file")?;

    println!("View '{name}' created successfully");

    Ok(())
}

fn main() {
    let matches = Command::new("pgm")
        .version(env!("CARGO_PKG_VERSION"))
        .arg_required_else_help(true)
        .about(
            "A CLI tool for managing postgres database migrations, triggers, views and functions",
        )
        .subcommand(
            Command::new("init")
                .about("Initializes the directory")
                .arg(
                    Arg::new("path")
                        .help("The path to the directory where pgm will store its files")
                        .default_value(DEFAULT_PGM_PATH)
                        .value_parser(clap::value_parser!(String)),
                )
                .arg(
                    Arg::new("existing-db")
                        .long("existing-db")
                        .help("Initialize from an existing database using pg_dump")
                        .action(clap::ArgAction::SetTrue),
                ),
        )
        .subcommand(
            Command::new("apply")
                .about("Compiles the changes and applies them to the database")
                .arg(
                    Arg::new("path")
                        .long("path")
                        .help("The path to the directory containing the database files")
                        .default_value(DEFAULT_PGM_PATH)
                        .value_parser(clap::value_parser!(String)),
                )
                .arg(
                    Arg::new("dry-run")
                        .long("dry-run")
                        .help("Prints the SQL that would be applied but does not apply it")
                        .action(clap::ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("fake")
                        .long("fake")
                        .help("Only updates pgm_ tables without executing the actual SQL")
                        .action(clap::ArgAction::SetTrue),
                ),
        )
        .subcommand(
            Command::new("create")
                .about("Creates a new database object")
                .subcommand_required(true)
                .subcommand(
                    Command::new("migration")
                        .about("Creates a new migration")
                        .arg(
                            Arg::new("path")
                                .long("path")
                                .help("The path to the directory containing the database files")
                                .default_value(DEFAULT_PGM_PATH)
                                .value_parser(clap::value_parser!(String)),
                        ),
                )
                .subcommand(
                    Command::new("trigger")
                        .about("Creates a new trigger")
                        .arg(
                            Arg::new("path")
                                .long("path")
                                .help("The path to the directory containing the database files")
                                .default_value(DEFAULT_PGM_PATH)
                                .value_parser(clap::value_parser!(String)),
                        )
                        .arg(
                            Arg::new("name")
                                .help("The name of the trigger")
                                .required(true)
                                .value_parser(clap::value_parser!(String)),
                        ),
                )
                .subcommand(
                    Command::new("view")
                        .about("Creates a new view")
                        .arg(
                            Arg::new("path")
                                .long("path")
                                .help("The path to the directory containing the database files")
                                .default_value(DEFAULT_PGM_PATH)
                                .value_parser(clap::value_parser!(String)),
                        )
                        .arg(
                            Arg::new("name")
                                .help("The name of the view")
                                .required(true)
                                .value_parser(clap::value_parser!(String)),
                        ),
                )
                .subcommand(
                    Command::new("function")
                        .about("Creates a new function")
                        .arg(
                            Arg::new("path")
                                .help("The path to the directory containing the database files")
                                .default_value(DEFAULT_PGM_PATH)
                                .value_parser(clap::value_parser!(String)),
                        )
                        .arg(
                            Arg::new("name")
                                .help("The name of the function")
                                .required(true)
                                .value_parser(clap::value_parser!(String)),
                        ),
                ),
        )
        .get_matches();

    match matches.subcommand() {
        Some(("init", init_matches)) => {
            let path = init_matches
                .get_one::<String>("path")
                .expect("Input argument is required");
            let existing_db = init_matches.get_flag("existing-db");
            if let Err(e) = initialize(path, existing_db) {
                eprintln!("Error during initialization:");
                for cause in e.chain() {
                    eprintln!("  - {}", cause);
                }
            } else {
                println!("Initialized successfully");
            }
        }
        Some(("apply", apply_matches)) => {
            let path = apply_matches
                .get_one::<String>("path")
                .expect("Input argument is required");
            let dry_run = apply_matches.get_flag("dry-run");
            let fake = apply_matches.get_flag("fake");

            match apply(path, dry_run, fake) {
                Ok(_) => {
                    if !dry_run {
                        println!("Changes applied successfully");
                    }
                }
                Err(e) => {
                    eprintln!("Error applying changes:");
                    for cause in e.chain() {
                        eprintln!("  - {}", cause);
                    }
                }
            }
        }
        Some(("create", create_matches)) => match create_matches.subcommand() {
            Some(("migration", _)) => {
                let path = create_matches
                    .get_one::<String>("path")
                    .expect("Input argument is required");
                if let Err(e) = create_migration(path) {
                    eprintln!("Error during migration creation:");
                    for cause in e.chain() {
                        eprintln!("  - {}", cause);
                    }
                } else {
                    println!("Migration created successfully");
                }
            }
            Some(("trigger", trigger_matches)) => {
                let path = trigger_matches
                    .get_one::<String>("path")
                    .expect("Input argument is required");
                let name = trigger_matches
                    .get_one::<String>("name")
                    .expect("Name argument is required");
                if let Err(e) = create_trigger(path, name) {
                    eprintln!("Error during trigger creation:");
                    for cause in e.chain() {
                        eprintln!("  - {}", cause);
                    }
                }
            }
            Some(("view", view_matches)) => {
                let path = view_matches
                    .get_one::<String>("path")
                    .expect("Input argument is required");
                let name = view_matches
                    .get_one::<String>("name")
                    .expect("Name argument is required");

                if let Err(e) = create_view(path, name) {
                    eprintln!("Error during view creation:");
                    for cause in e.chain() {
                        eprintln!("  - {}", cause);
                    }
                }
            }
            Some(("function", function_matches)) => {
                let path = function_matches
                    .get_one::<String>("path")
                    .expect("Input argument is required");
                let name = function_matches
                    .get_one::<String>("name")
                    .expect("Name argument is required");

                if let Err(e) = create_function(path, name) {
                    eprintln!("Error during function creation:");
                    for cause in e.chain() {
                        eprintln!("  - {}", cause);
                    }
                }
            }
            _ => {}
        },
        _ => {}
    }
}
