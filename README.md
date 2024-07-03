# pgm

A command-line tool for managing PostgreSQL migrations, functions, triggers, and views.

## Features

- Initialize projects
- Manage migrations, functions, triggers, and views
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
pgm create materialized-view <name> [--path <path>]
pgm create function <name> [--path <path>]
```

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
