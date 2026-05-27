//! Export the Firetrail Record JSON Schema to a file.
//!
//! Usage:
//!   `cargo run -p ft-core --example export_schema -- <output-path>`
//!
//! The default `just schema` target writes to `docs/schema/firetrail-record-v1.json`.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(out) = args.next() else {
        eprintln!("usage: export_schema <output-path>");
        return ExitCode::from(2);
    };
    if args.next().is_some() {
        eprintln!("usage: export_schema <output-path>");
        return ExitCode::from(2);
    }

    let path = PathBuf::from(out);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(e) = fs::create_dir_all(parent) {
                eprintln!("failed to create {}: {e}", parent.display());
                return ExitCode::FAILURE;
            }
        }
    }

    let mut json = ft_core::record_schema_json();
    json.push('\n');
    if let Err(e) = fs::write(&path, json) {
        eprintln!("failed to write {}: {e}", path.display());
        return ExitCode::FAILURE;
    }
    println!("wrote schema to {}", path.display());
    ExitCode::SUCCESS
}
