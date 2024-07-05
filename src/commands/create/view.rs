use std::io::{self, Write};
use std::path::Path;
use anyhow::{Context, Result};

pub fn create_view(pgm_dir_path: &str, name: &str) -> Result<()> {
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

    let template = std::fs::read_to_string("./src/commands/create/templates/view.sql")
        .context("Failed to read view template")?;
    let content = template.replace("<name_placeholder>", name);
    std::fs::File::create(&file_path).context("Failed to create view file")?;
    std::fs::write(file_path, content).context("Failed to write to view file")?;

    println!("View '{}' created successfully", name);
    Ok(())
}
