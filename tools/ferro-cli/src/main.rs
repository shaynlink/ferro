//! The Ferro developer command-line interface.

use std::env;

use ferro_contracts::WorkspaceMaintenance;
use ferro_storage::WorkspaceDatabase;

fn main() {
    match env::args().nth(1).as_deref() {
        Some("doctor") => doctor(),
        Some("storage") => storage(env::args().nth(2)),
        Some("help" | "--help" | "-h") | None => print_help(),
        Some(command) => {
            eprintln!("unknown command: {command}");
            print_help();
            std::process::exit(2);
        }
    }
}

fn doctor() {
    println!("Ferro doctor");
    println!("- Rust toolchain: available");
    println!("- Host OS: {}", env::consts::OS);
    println!("- Storage: embedded LiliaDB workspace databases");
    println!("- Next: add platform-specific SDK checks in ferro-cli.");
}

fn storage(path: Option<String>) {
    let Some(path) = path else {
        eprintln!("usage: ferro storage <workspace-db-path>");
        std::process::exit(2);
    };
    let database = WorkspaceDatabase::open(path).unwrap_or_else(|error| {
        eprintln!("could not open workspace database: {error}");
        std::process::exit(1);
    });
    match database.integrity_check() {
        Ok(true) => println!("Ferro storage: healthy"),
        Ok(false) => {
            eprintln!("Ferro storage: integrity check failed");
            std::process::exit(1);
        }
        Err(error) => {
            eprintln!("could not check workspace database: {error}");
            std::process::exit(1);
        }
    }
}

fn print_help() {
    println!(
        "Usage: ferro <command>\n\nCommands:\n  doctor          inspect the local toolchain\n  storage <path>  verify a workspace LiliaDB database"
    );
}
