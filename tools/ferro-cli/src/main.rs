//! The Ferro developer command-line interface.

use std::env;

fn main() {
    match env::args().nth(1).as_deref() {
        Some("doctor") => doctor(),
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
    println!("- Next: add platform-specific SDK checks in ferro-cli.");
}

fn print_help() {
    println!("Usage: ferro <command>\n\nCommands:\n  doctor  inspect the local toolchain");
}
