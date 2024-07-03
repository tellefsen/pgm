use anyhow::{Context, Result};
use clap::{Arg, Command};
use md5;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command as ProcessCommand;
use tempfile::NamedTempFile;

const DEFAULT_PGM_PATH: &str = "postgres";

fn parse_schema_dump(pgm_dir_path: &str, schema_dump: &String) -> std::io::Result<()> {
    let tokens: Vec<&str> = schema_dump
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
                let output_path = Path::new(pgm_dir_path)
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

    // Remove all lines starting with -- and other unnecessary lines
    let migrations_file_content = migrations_file_content
        .lines()
        .filter(|line| !line.starts_with("--"))
        .filter(|line| !line.starts_with("SET check_function_bodies = false"))
        .filter(|line| !line.starts_with("SELECT pg_catalog.set_config('search_path'"))
        .filter(|line| !line.is_empty())
        .collect::<Vec<&str>>()
        .join("\n");

    // Write the new file content without functions to a new file
    let migrations_file_path = Path::new(pgm_dir_path).join("migrations").join("00000.sql");
    std::fs::write(migrations_file_path, migrations_file_content)?;

    Ok(())
}

fn initialize(pgm_dir_path: &str) -> Result<()> {
    if Path::new(pgm_dir_path).exists() {
        return Err(anyhow::anyhow!(
            "Directory '{}' already exists",
            pgm_dir_path
        ));
    }

    // Create temporary file for schema dump
    let schema_dump_file =
        NamedTempFile::new().context("Failed to create temporary file for schema dump")?;

    // Call pg_dump to get schema-only dump
    let output_path = schema_dump_file.path().to_str().unwrap();
    let mut child = match ProcessCommand::new("pg_dump")
        .args(&["-f", output_path, "--no-owner", "--schema-only"])
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

    // Read the schema dump
    let schema_dump = std::fs::read_to_string(output_path)?;

    // Create new directory "./postgres" and subdirectories
    std::fs::create_dir_all(pgm_dir_path).context("Failed to create directory")?;
    std::fs::create_dir_all(format!("{}/migrations", pgm_dir_path))
        .context("Failed to create migrations directory")?;
    std::fs::create_dir_all(format!("{}/triggers", pgm_dir_path))
        .context("Failed to create triggers directory")?;
    std::fs::create_dir_all(format!("{}/views", pgm_dir_path))
        .context("Failed to create views directory")?;
    std::fs::create_dir_all(format!("{}/functions", pgm_dir_path))
        .context("Failed to create functions directory")?;

    // Extract all functions and triggers from schema dump and write them to the appropriate folders
    // Then create a migration file with the changes
    parse_schema_dump(pgm_dir_path, &schema_dump)
        .context("Failed to extract functions and triggers")?;

    Ok(())
}

fn build(pgm_dir_path: &str) -> Result<String> {
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

    // Add all the content to the compiled SQL file, the hash is not updated here because we will update it below (where we also check the function body)
    compiled_content.push_str(&process_directory(&functions_dir, "pgm_function", false)?);
    compiled_content.push_str(&process_directory(&triggers_dir, "pgm_trigger", false)?);

    // Add the migrations and views
    compiled_content.push_str(&process_migrations(pgm_dir_path)?);
    compiled_content.push_str(&process_directory(&views_dir, "pgm_view", true)?);

    // At this point we already know that tables/functions/triggers/views are created
    // However we must check the function bodies (since the check was turned off above)
    compiled_content.push_str("SET LOCAL check_function_bodies = true;\n");
    compiled_content.push_str(&process_directory(&functions_dir, "pgm_function", true)?);
    compiled_content.push_str(&process_directory(&triggers_dir, "pgm_trigger", true)?);


    // End the main DO block
    compiled_content.push_str("END $pgm$;\n");

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
                    "INSERT INTO {table} (name, hash) VALUES ('{file_name}', '{hash}') ON CONFLICT (name) DO UPDATE SET hash = EXCLUDED.hash, applied_at = CURRENT_TIMESTAMP;"
                )
            } else {
                String::new()
            };

            compiled_content.push_str(&format!(
                "-- RUN {file_path} --
IF (SELECT hash FROM {table} WHERE name = '{file_name}') IS DISTINCT FROM '{hash}' THEN
{content}

{update_hash_query}

    RAISE NOTICE 'Applied {file_path}';
ELSE
    RAISE NOTICE 'Skipped {file_path} (no changes)';
END IF;
-- DONE {file_path} --

"
            ));
        }
    }
    Ok(compiled_content)
}

fn process_migrations(pgm_dir_path: &str) -> Result<String> {
    let migrations_dir = format!("{}/migrations", pgm_dir_path);
    let migrations_dir = migrations_dir.as_str();
    let mut migration_files: Vec<_> = std::fs::read_dir(migrations_dir)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.path().is_file() && entry.path().extension().map_or(false, |ext| ext == "sql")
        })
        .collect();

    migration_files.sort_by_key(|entry| entry.file_name());

    let mut compiled_content = String::new();
    for entry in migration_files {
        let path = entry.path();
        let content = std::fs::read_to_string(&path)?;

        let file_name = path.file_stem().unwrap().to_str().unwrap();

        let file_path = format!("{}/{}.sql", &migrations_dir, file_name);

        compiled_content.push_str(&format!(
            "-- RUN {file_path} --
IF NOT EXISTS (SELECT 1 FROM pgm_migration WHERE name = '{file_name}') THEN
{content}

INSERT INTO pgm_migration (name) VALUES ('{file_path}');
RAISE NOTICE 'Applied migration: {file_path}';
ELSE
RAISE NOTICE 'Skipped migration: {file_path} (already applied)';
END IF;
-- DONE {file_path} --

"
        ));
    }
    Ok(compiled_content)
}

fn apply(pgm_dir_path: &str, dry_run: bool) -> Result<()> {
    // Compile the SQL
    let sql = build(pgm_dir_path).expect("Failed to compile SQL");

    if dry_run {
        println!("{}", sql);
        return Ok(());
    }

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

    // Execute the psql command
    match command.output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            // Process stderr and stdout to remove prefix 'psql:/path/to/temp/file:1234: '
            stderr.lines().chain(stdout.lines()).for_each(|line| {
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
        Err(e) => Err(anyhow::anyhow!("Failed to execute psql command: {}.", e)),
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

fn create_view(pgm_dir_path: &str, name: &str, materialized: bool) -> Result<()> {
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

    let template_name = if materialized {
        "./src/templates/materialized_view.sql"
    } else {
        "./src/templates/view.sql"
    };

    let template =
        std::fs::read_to_string(template_name).context("Failed to read view template")?;
    let content = template.replace("<name_placeholder>", name);
    std::fs::write(file_path, content).context("Failed to create view file")?;

    println!(
        "{} '{}' created successfully",
        if materialized {
            "Materialized view"
        } else {
            "View"
        },
        name
    );
    Ok(())
}

fn main() {
    let matches = Command::new("pgm")
        .version("0.1")
        .arg_required_else_help(true)
        .about(
            "A CLI tool for managing postgres database migrations, triggers, views and functions",
        )
        .subcommand(
            Command::new("init").about("Initializes the directory").arg(
                Arg::new("path")
                    .help("The path to the directory containing the database files")
                    .default_value(DEFAULT_PGM_PATH)
                    .value_parser(clap::value_parser!(String)),
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
                    Command::new("materialized-view")
                        .about("Creates a new materialized view")
                        .arg(
                            Arg::new("path")
                                .long("path")
                                .help("The path to the directory containing the database files")
                                .default_value(DEFAULT_PGM_PATH)
                                .value_parser(clap::value_parser!(String)),
                        )
                        .arg(
                            Arg::new("name")
                                .help("The name of the materialized view")
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
            if let Err(e) = initialize(path) {
                eprintln!("Error during initialization: {}", e);
            } else {
                println!("Initialized successfully");
            }
        }
        Some(("apply", apply_matches)) => {
            let path = apply_matches
                .get_one::<String>("path")
                .expect("Input argument is required");
            let dry_run = *apply_matches
                .get_one::<bool>("dry-run")
                .expect("Default value should always be present");

            match apply(path, dry_run) {
                Ok(_) => {
                    if !dry_run {
                        println!("Changes applied successfully");
                    }
                }
                Err(e) => eprintln!("Error applying changes: {}", e),
            }
        }
        Some(("create", create_matches)) => match create_matches.subcommand() {
            Some(("migration", _)) => {
                let path = create_matches
                    .get_one::<String>("path")
                    .expect("Input argument is required");
                if let Err(e) = create_migration(path) {
                    eprintln!("Error during migration creation: {}", e);
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
                    eprintln!("Error during trigger creation: {}", e);
                }
            }
            Some(("view", view_matches)) => {
                let path = view_matches
                    .get_one::<String>("path")
                    .expect("Input argument is required");
                let name = view_matches
                    .get_one::<String>("name")
                    .expect("Name argument is required");

                if let Err(e) = create_view(path, name, false) {
                    eprintln!("Error during view creation: {}", e);
                }
            }
            Some(("materialized-view", view_matches)) => {
                let path = view_matches
                    .get_one::<String>("path")
                    .expect("Input argument is required");
                let name = view_matches
                    .get_one::<String>("name")
                    .expect("Name argument is required");

                if let Err(e) = create_view(path, name, true) {
                    eprintln!("Error during materialized view creation: {}", e);
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
                    eprintln!("Error during function creation: {}", e);
                }
            }
            _ => {}
        },
        _ => {}
    }
}
