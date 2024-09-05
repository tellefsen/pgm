use anyhow::{Context, Result};
use std::io::{self, Write};
use std::path::Path;

pub fn create_trigger(pgm_dir_path: &str, name: &str) -> Result<()> {
    if !Path::new(pgm_dir_path).exists() {
        return Err(anyhow::anyhow!(
            "Directory '{}' not found. Have you run 'pgm init'?",
            pgm_dir_path
        ));
    }

    let triggers_dir = Path::new(pgm_dir_path).join("triggers");
    std::fs::create_dir_all(&triggers_dir).context("Failed to create triggers directory")?;

    let file_path = triggers_dir.join(format!("{}.sql", name));
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

    let template = include_str!("templates/trigger_function.sql");
    let content = template.replace("<name_placeholder>", name);
    std::fs::File::create(&file_path).context("Failed to create trigger file")?;
    std::fs::write(file_path, content).context("Failed to write to trigger file")?;

    println!("Trigger '{}' created successfully", name);
    Ok(())
}
