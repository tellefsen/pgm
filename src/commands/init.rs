use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command as ProcessCommand;
use tempfile::NamedTempFile;

use crate::INITIAL_MIGRATION_FILE_NAME;

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
        .replace("SELECT pg_catalog.set_config('search_path', '', false);", "PERFORM pg_catalog.set_config('search_path', '', false);")
        .replace("SET client_min_messages = warning;", "SET client_min_messages = notice;");
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

pub fn init(pgm_dir_path: &str, existing_db: bool) -> Result<()> {
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
