use anyhow::{Context, Result};
use clap::{Arg, Command};
use md5;
use std::fs;
use std::path::Path;
use std::process::Command as ProcessCommand;
use tempfile::NamedTempFile;

fn parse_schema_dump(input_path: &str, output_dir: &str) -> std::io::Result<()> {
    let file_content = std::fs::read_to_string(input_path)?;

    let tokens: Vec<&str> = file_content
        .split_inclusive(|c: char| c.is_whitespace())
        .collect();
    let mut token_iter = tokens.iter();

    let mut migrations_file_content = String::new();

    let mut function_name = String::new();
    let mut function_content = String::new();
    let mut in_function = false;
    let mut block_label = String::new();
    let mut is_trigger = false;

    while let Some(&token) = token_iter.next() {
        if !in_function {
            // Look for CREATE
            if token.trim().eq_ignore_ascii_case("CREATE") {
                // Then look for FUNCTION
                if let Some(next_token) = token_iter.next() {
                    if next_token.trim().eq_ignore_ascii_case("FUNCTION") {
                        in_function = true;
                        function_content.push_str("CREATE OR REPLACE FUNCTION ");
                        // Then look for the function name
                        if let Some(name_token) = token_iter.next() {
                            let func_name_block = name_token.to_string();
                            function_name = func_name_block
                                .split("(")
                                .next()
                                .expect("Expected function name")
                                .to_string();
                            function_content.push_str(&func_name_block);
                            // Then look for what the function returns
                            while let Some(next_token) = token_iter.next() {
                                function_content.push_str(next_token);
                                if next_token.trim().eq_ignore_ascii_case("RETURNS") {
                                    let return_statement = token_iter.next().unwrap().to_string();
                                    function_content.push_str(&return_statement);
                                    is_trigger =
                                        return_statement.trim().eq_ignore_ascii_case("TRIGGER");
                                    break;
                                }
                            }
                            // Then look for opening block eg. $$ or $function$ or any other label like $mylabel$
                            while let Some(next_token) = token_iter.next() {
                                function_content.push_str(next_token);
                                if next_token.trim().starts_with('$')
                                    && next_token.trim().ends_with('$')
                                {
                                    block_label = next_token.trim().to_string();
                                    break;
                                }
                            }
                        }
                    } else {
                        migrations_file_content.push_str(token);
                        migrations_file_content.push_str(&next_token)
                    }
                }
            } else {
                migrations_file_content.push_str(token);
            }
        } else {
            // Look for closing block label if not keep reading
            function_content.push_str(token);
            if token.contains(&block_label) {
                let folder = if is_trigger { "triggers" } else { "functions" };
                let output_path = Path::new(output_dir)
                    .join(folder)
                    .join(format!("{}.sql", function_name));
                std::fs::write(output_path, function_content).unwrap();
                in_function = false;
                function_name = String::new();
                block_label = String::new();
                function_content = String::new();
                is_trigger = false;
            }
        }
    }

    // Remove all lines starting with --
    let migrations_file_content = migrations_file_content
        .lines()
        .filter(|line| !line.starts_with("--"))
        .filter(|line| !line.is_empty())
        .collect::<Vec<&str>>()
        .join("\n");

    // Write the new file content without functions to a new file
    let migrations_file_path = Path::new(output_dir).join("migrations").join("00001.sql");
    std::fs::write(migrations_file_path, migrations_file_content)?;

    Ok(())
}

fn initialize() -> Result<()> {
    // If "./postgres" already exists, exit
    if Path::new("./postgres").exists() {
        return Err(anyhow::anyhow!("Postgres directory already exists"));
    }

    // Create a temporary file for pg_dump
    let temp_file = NamedTempFile::new().context("Failed to create temporary file")?;
    let temp_path = temp_file.path().to_str().unwrap();

    // Call pg_dump to get a dump of the current database
    let mut child = match ProcessCommand::new("pg_dump")
        .args(&["-f", temp_path, "--schema-only", "--no-owner"])
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

    // Create new directory "./postgres"
    std::fs::create_dir_all("./postgres").context("Failed to create directory")?;

    // Create subfolder "./postgres/migrations"
    std::fs::create_dir_all("./postgres/migrations").context("Failed to create migrations directory")?;
    // Create subfolder "./postgres/triggers"
    std::fs::create_dir_all("./postgres/triggers").context("Failed to create triggers directory")?;
    // Create subfolder "./postgres/views"
    std::fs::create_dir_all("./postgres/views").context("Failed to create views directory")?;
    // Create subfolder "./postgres/functions"
    std::fs::create_dir_all("./postgres/functions").context("Failed to create functions directory")?;

    // Extract all functions and write them to the appropriate folder
    parse_schema_dump(&temp_path, "./postgres").context("Failed to extract functions")?;

    // Move tempfile to migrations folder
    std::fs::rename(temp_path, "./postgres/migrations/00000.sql").context("Failed to move dump file")?;

    Ok(())
}

fn build(output: &str) -> Result<()> {
    // Check if the postgres directory exists
    if !Path::new("./postgres").is_dir() {
        return Err(anyhow::anyhow!(
            "Directory './postgres' not found. Have you run 'pgm init'?"
        ));
    }

    let mut compiled_content = String::new();

    compiled_content.push_str("DO $pgm$ BEGIN");
    // Create tables if they don't exist
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

"#,
    );

    // Process functions
    process_directory(
        "./postgres/functions",
        "pgm_function",
        &mut compiled_content,
    )?;

    // Process triggers
    process_directory("./postgres/triggers", "pgm_trigger", &mut compiled_content)?;

    // Process migrations
    process_migrations(&mut compiled_content)?;

    // End the single DO block
    compiled_content.push_str("END $pgm$;\n");

    // Write the compiled content to the output file
    std::fs::write(output, compiled_content).context("Failed to write compiled file")?;

    println!("Successfully compiled changes to: {}", output);
    Ok(())
}

fn process_directory(dir: &str, table: &str, compiled_content: &mut String) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension().map_or(false, |ext| ext == "sql") {
            let content = std::fs::read_to_string(&path)?;

            let hash = format!("{:x}", md5::compute(&content));
            let file_name = path.file_stem().unwrap().to_str().unwrap();

            compiled_content.push_str(&format!(
                "-- RUN {dir}/{file_name}.sql --
IF (SELECT hash FROM {table} WHERE name = '{file_name}') IS DISTINCT FROM '{hash}' THEN
{content}

    INSERT INTO {table} (name, hash) 
    VALUES ('{file_name}', '{hash}')
    ON CONFLICT (name) 
    DO UPDATE SET hash = EXCLUDED.hash, applied_at = CURRENT_TIMESTAMP;
    
    RAISE NOTICE 'Applied {dir}/{file_name}.sql';
ELSE
    RAISE NOTICE 'Skipped {dir}/{file_name}.sql (no changes)';
END IF;
-- DONE {dir}/{file_name}.sql --

"
            ));
        }
    }
    Ok(())
}

fn process_migrations(compiled_content: &mut String) -> Result<()> {
    let migrations_dir = "./postgres/migrations";
    let mut migration_files: Vec<_> = std::fs::read_dir(migrations_dir)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.path().is_file() && entry.path().extension().map_or(false, |ext| ext == "sql")
        })
        .collect();

    migration_files.sort_by_key(|entry| entry.file_name());

    for entry in migration_files {
        let path = entry.path();
        let content = std::fs::read_to_string(&path)?;

        let file_name = path.file_stem().unwrap().to_str().unwrap();

        compiled_content.push_str(&format!(
            "-- RUN {migrations_dir}/{file_name}.sql --
IF NOT EXISTS (SELECT 1 FROM pgm_migration WHERE name = '{file_name}') THEN
{content}

INSERT INTO pgm_migration (name) VALUES ('{file_name}');
RAISE NOTICE 'Applied migration: {file_name}';
ELSE
RAISE NOTICE 'Skipped migration: {file_name} (already applied)';
END IF;
-- DONE {migrations_dir}/{file_name}.sql --

"
        ));
    }
    Ok(())
}

fn main() {
    let matches = Command::new("pgm")
        .version("0.1")
        .about(
            "A CLI tool for managing postgres database migrations, triggers, views and functions",
        )
        .subcommand(Command::new("init").about("Initializes the directory"))
        .subcommand(
            Command::new("apply").about("Applies the changes").arg(
                Arg::new("url")
                    .help("The postgres URL")
                    .value_parser(clap::value_parser!(String)),
            ),
        )
        .subcommand(
            Command::new("create")
                .about("Creates a new database object")
                .subcommand_required(true)
                .subcommand(Command::new("migration").about("Creates a new migration"))
                .subcommand(
                    Command::new("trigger").about("Creates a new trigger").arg(
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
                            Arg::new("name")
                                .help("The name of the view")
                                .required(true)
                                .value_parser(clap::value_parser!(String)),
                        )
                        .arg(
                            Arg::new("materialized")
                                .long("materialized")
                                .help("Creates a materialized view"),
                        ),
                )
                .subcommand(
                    Command::new("function")
                        .about("Creates a new function")
                        .arg(
                            Arg::new("name")
                                .help("The name of the function")
                                .required(true)
                                .value_parser(clap::value_parser!(String)),
                        ),
                ),
        )
        .subcommand(
            Command::new("build")
                .about("Compiles changes to a single .sql file that can be deployed")
                .arg(
                    Arg::new("output")
                        .help("The output file path")
                        .default_value("postgres/output.sql")
                        .value_parser(clap::value_parser!(String)),
                ),
        )
        .get_matches();

    match matches.subcommand() {
        Some(("init", _)) => {
            if let Err(e) = initialize() {
                eprintln!("Error during initialization: {}", e);
            } else {
                println!("Initialized successfully");
            }
        }
        Some(("apply", apply_matches)) => {
            if let Some(url) = apply_matches.get_one::<String>("url") {
                println!("Applying changes to database at: {}", url);
            } else {
                println!("Applying changes to default database");
            }
            todo!("apply command logic");
        }
        Some(("create", create_matches)) => match create_matches.subcommand() {
            Some(("migration", _)) => {
                println!("Creating a new migration");
                todo!("create migration command logic");
            }
            Some(("trigger", trigger_matches)) => {
                let name = trigger_matches
                    .get_one::<String>("name")
                    .expect("Name argument is required");
                println!("Creating a new trigger: {}", name);
                todo!("create trigger command logic");
            }
            Some(("view", view_matches)) => {
                let name = view_matches
                    .get_one::<String>("name")
                    .expect("Name argument is required");
                if view_matches.contains_id("materialized") {
                    println!("Creating a new materialized view: {}", name);
                } else {
                    println!("Creating a new view: {}", name);
                }
                todo!("create view command logic");
            }
            Some(("function", function_matches)) => {
                let name = function_matches
                    .get_one::<String>("name")
                    .expect("Name argument is required");
                println!("Creating a new function: {}", name);
                todo!("create function command logic");
            }
            _ => {}
        },
        Some(("build", build_matches)) => {
            let output = build_matches
                .get_one::<String>("output")
                .expect("Output argument is required");
            if let Err(e) = build(output) {
                eprintln!("Error during build: {}", e);
            }
        }
        _ => {}
    }
}
