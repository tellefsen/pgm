mod commands;

use clap::{Arg, Command};
use dotenv::dotenv;

const DEFAULT_PGM_PATH: &str = "postgres";
const INITIAL_MIGRATION_FILE_NAME: &str = "00000.sql";

fn main() {
    // Load environment variables from .env file
    dotenv().ok();

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
            Command::new("seed")
                .about("Seeds the database with data")
                .arg(
                    Arg::new("path")
                        .long("path")
                        .help("The path to the directory containing the database files")
                        .default_value(DEFAULT_PGM_PATH)
                        .value_parser(clap::value_parser!(String)),
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
                                .long("path")
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
                )
                .subcommand(
                    Command::new("seed").about("Creates a new seed").arg(
                        Arg::new("path")
                            .long("path")
                            .help("The path to the directory containing the database files")
                            .default_value(DEFAULT_PGM_PATH)
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
            if let Err(e) = commands::init(path, existing_db) {
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

            match commands::apply(path, dry_run, fake) {
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
            Some(("migration", migration_matches)) => {
                let path = migration_matches
                    .get_one::<String>("path")
                    .expect("Input argument is required");
                if let Err(e) = commands::create_migration(path) {
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
                if let Err(e) = commands::create_trigger(path, name) {
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

                if let Err(e) = commands::create_view(path, name) {
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

                if let Err(e) = commands::create_function(path, name) {
                    eprintln!("Error during function creation:");
                    for cause in e.chain() {
                        eprintln!("  - {}", cause);
                    }
                }
            }
            Some(("seed", seed_matches)) => {
                let path = seed_matches
                    .get_one::<String>("path")
                    .expect("Input argument is required");
                if let Err(e) = commands::create_seed(path) {
                    eprintln!("Error during seed creation:");
                    for cause in e.chain() {
                        eprintln!("  - {}", cause);
                    }
                } else {
                    println!("Seed created successfully");
                }
            }
            _ => {}
        },
        Some(("seed", seed_matches)) => {
            let path = seed_matches
                .get_one::<String>("path")
                .expect("Input argument is required");
            if let Err(e) = commands::seed(path) {
                eprintln!("Error seeding database:");
                for cause in e.chain() {
                    eprintln!("  - {}", cause);
                }
            } else {
                println!("Database seeded successfully");
            }
        }
        _ => {}
    }
}
