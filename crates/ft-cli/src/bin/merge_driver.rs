//! `firetrail-merge-driver` — git custom merge driver entry point.
//!
//! Registered in a repository's `.git/config` via:
//!
//! ```ini
//! [merge "firetrail"]
//!     name = Firetrail record three-way merge
//!     driver = firetrail-merge-driver %O %A %B
//! ```
//!
//! Git substitutes `%O` (common ancestor blob), `%A` (local), `%B` (remote)
//! with temp file paths. The driver runs [`ft_pr::merge::merge_driver_cli`]
//! and writes the merged JSON back to `%A`. Exits 0 on a clean merge and 1
//! when conflicts remain — matching git's contract.

#![deny(missing_docs)]

use std::path::PathBuf;
use std::process::ExitCode;

use ft_pr::merge::{MergeDriverArgs, merge_driver_cli};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!(
            "firetrail-merge-driver: expected 3 arguments (%O %A %B); got {}",
            args.len().saturating_sub(1)
        );
        return ExitCode::from(2);
    }

    let driver_args = MergeDriverArgs {
        base_path: PathBuf::from(&args[1]),
        ours_path: PathBuf::from(&args[2]),
        theirs_path: PathBuf::from(&args[3]),
    };

    match merge_driver_cli(&driver_args) {
        Ok(out) => {
            // Map ft-pr's i32 exit code to ExitCode without panicking on
            // out-of-range values (i32 -> u8 saturates to a documented code).
            let code = u8::try_from(out.exit_code.clamp(0, 255)).unwrap_or(1);
            ExitCode::from(code)
        }
        Err(e) => {
            eprintln!("firetrail-merge-driver: {e}");
            ExitCode::from(2)
        }
    }
}
