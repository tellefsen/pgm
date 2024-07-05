# pgm

A command-line tool for managing PostgreSQL migrations, functions, triggers, views, and seeds.

## Features

- Initialize projects
- Manage migrations, functions, triggers, views, and seeds
- Apply changes with dry-run option

## Installation

### macOS/Linux:
```bash
curl -sSL https://raw.githubusercontent.com/tellefsen/pgm/main/install.sh | bash
```

### Windows (PowerShell):
```powershell
irm https://raw.githubusercontent.com/tellefsen/pgm/main/install.ps1 | iex
```

## Requirements

- `psql` and `pg_dump` (part of PostgreSQL distribution)

## Usage

### Initialize project
```
pgm init [--path <path>] [--existing-db]
```

### Apply changes
```
pgm apply [--path <path>] [--dry-run] [--fake]
```

### Create new elements
```
pgm create migration [--path <path>]
pgm create trigger <name> [--path <path>]
pgm create view <name> [--path <path>]
pgm create function <name> [--path <path>]
pgm create seed [--path <path>]
```

### Seed the database
```
pgm seed [--path <path>]
```

### Environment Variables

pgm uses environment variables for database connection. You can set these in three ways:

1. Directly in your shell:
   ```bash
   export PGHOST=localhost
   export PGPORT=5432
   # ... other variables ...
   ```

2. Using a `.env` file in your project root:
   ```
   PGHOST=localhost
   PGPORT=5432
   # ... other variables ...
   ```

   pgm will automatically load variables from a `.env` file if present.

3. Prepending to the pgm command:
   ```bash
   PGUSER=myuser pgm apply
   ```

   This method allows you to set or override environment variables for a single command execution.

## Project Structure
```
postgres/
├── functions/
├── triggers/
├── views/
└── migrations/
```

## How It Works

pgm tracks changes in SQL files and applies updates in this order:
1. Functions
2. Triggers
3. Migrations
4. Views

Changes are applied atomically within a single transaction.

For detailed usage and examples, visit our [GitHub repository](https://github.com/tellefsen/pgm).
