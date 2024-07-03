# pgm

pgm is a command-line tool used to manage PostgreSQL migrations, functions, triggers, and views.

## Features

- Initialize a new project structure
- Create and manage migrations
- Handle functions, triggers, and views
- Apply changes to the database
- Dry-run option for testing changes

## Installation

### For macOS and Linux:

```bash
curl -sSL https://raw.githubusercontent.com/tellefsen/pgm/main/install.sh | bash
```

### For Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/tellefsen/pgm/main/install.ps1 | iex
```

These commands will download and run a script that installs the appropriate pre-compiled binary for your system.

## Requirements

- `psql`: Required for applying changes to the database.
- `pg_dump`: Required only if initializing pgm from an existing database schema.

These tools are part of the PostgreSQL distribution. If you don't have them installed:

- macOS: Use Homebrew
  ```
  brew install libpq
  sudo ln -s $(brew --prefix)/opt/libpq/bin/psql /usr/local/bin/psql
  sudo ln -s $(brew --prefix)/opt/libpq/bin/pg_dump /usr/local/bin/pg_dump
  ```

- Linux:
  - Debian/Ubuntu:
    ```
    sudo apt-get update
    sudo apt-get install postgresql-client
    ```

- Windows: Use [Scoop](https://scoop.sh/)
  ```
  scoop install postgresql
  ```

If CLI options are not available, you can download the full PostgreSQL distribution from the [official PostgreSQL website](https://www.postgresql.org/download/).

Make sure these tools are in your system's PATH after installation.

## Usage

### Initialize a new project

```
pgm init [--path <path>] [--existing-db]
```

Initializes a new project structure in the specified path (default: "postgres").

Options:
- `--path <path>`: Specify a custom path for the project (default: "postgres")
- `--existing-db`: Initialize from an existing database using pg_dump

The `init` command works in two ways:

1. Creating a new project structure: This creates the necessary directories for managing your database objects.

2. Initializing from an existing database: When used with the `--existing-db` flag, pgm uses `pg_dump` to get a schema-only dump of your existing database. It then parses this dump to extract functions, triggers, and other database objects, creating a structured project from your current schema. This allows you to start managing your existing database with pgm.

Note: When initializing from an existing database, ensure you have the necessary database connection details set up (e.g., through environment variables or a .pgpass file).

Example:
```
PGDATABASE=mydb pgm init --existing-db
```

### Apply changes

```
pgm apply [--path <path>] [--dry-run]
```

Compiles and applies changes to the database. Use `--path` for a custom path and `--dry-run` to test without applying.

pgm uses standard PostgreSQL environment variables for connection. Set these before running or use a `.pgpass` file for credentials.

Example:
```
PGDATABASE=mydb pgm apply
```

For a dry run:
```
PGDATABASE=mydb pgm apply --dry-run
```

### Create new elements

#### Create a migration

```
pgm create migration [--path <path>]
```

Creates a new migration file with an auto-incremented number. Default path: "postgres".

#### Create a trigger

```
pgm create trigger <name> [--path <path>]
```

Creates a new trigger file with the specified name. Default path: "postgres".

#### Create a view

```
pgm create view <name> [--path <path>]
```

Creates a new view file with the specified name. Default path: "postgres".

#### Create a materialized view

```
pgm create materialized-view <name> [--path <path>]
```

Creates a new materialized view file with the specified name. Default path: "postgres".

#### Create a function

```
pgm create function <name> [--path <path>]
```

Creates a new function file with the specified name. Default path: "postgres".

Note: For all create commands, the `--path` option refers to the pgm folder path, not the specific subfolder (e.g., triggers, views, etc.). The tool will automatically place the created file in the appropriate subfolder within the specified path.

## Project Structure

After initialization, your project will have the following structure:

```
postgres/
├── functions/
├── triggers/
├── views/
└── migrations/
```

## How It Works

pgm manages your database schema by tracking changes in SQL files. It detects changes and applies only the necessary updates to your database.

pgm applies changes to the database in a specific order to ensure dependencies are met and to maintain consistency:
1. Functions: Applied first as they may be required by triggers, views, or migrations.
2. Triggers: Applied second as they often depend on functions but should be in place before data changes.
3. Migrations: Applied third to handle schema changes and data modifications.
4. Views: Applied last as they may depend on schema changes made by migrations.

This order helps to minimize errors due to dependencies and ensures that all necessary database objects are in place before they are referenced or used.

pgm uses a transaction-based approach, wrapping all changes in a single transaction. This ensures that all changes are applied atomically, maintaining database consistency even if an error occurs during the application process.