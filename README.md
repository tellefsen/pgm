# pgm

pgm is a command-line tool used to manage PostgreSQL migrations, functions, triggers, and views.

## Features

- Initialize a new project structure
- Create and manage migrations
- Handle functions, triggers, and views
- Apply changes to the database
- Dry-run option for testing changes

## Installation

[Add installation instructions here]

## Usage

### Initialize a new project

```
pgm init [path]
```

Initializes a new project structure in the specified path (default: "postgres").

### Apply changes

```
pgm apply [path] [--dry-run]
```

Compiles and applies changes to the database. Use the `--dry-run` flag to test changes without applying them.

### Create new elements

#### Create a migration

```
pgm create migration [--path <path>]
```

Creates a new migration file with an auto-incremented number. Default path: "postgres/migrations".

#### Create a trigger

```
pgm create trigger <name> [--path <path>]
```

Creates a new trigger file with the specified name. Default path: "postgres/triggers".

#### Create a view

```
pgm create view <name> [--path <path>]
```

Creates a new view file with the specified name. Default path: "postgres/views".

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

## Technical Information

pgm applies changes to the database in a specific order to ensure dependencies are met and to maintain consistency:

1. Functions: Applied first as they may be required by triggers, views, or migrations.
2. Triggers: Applied second as they often depend on functions but should be in place before data changes.
3. Migrations: Applied third to handle schema changes and data modifications.
4. Views: Applied last as they may depend on schema changes made by migrations.

This order helps to minimize errors due to dependencies and ensures that all necessary database objects are in place before they are referenced or used.

pgm uses a transaction-based approach, wrapping all changes in a single transaction. This ensures that all changes are applied atomically, maintaining database consistency even if an error occurs during the application process.
