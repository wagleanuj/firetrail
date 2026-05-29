//! # ft-vec
//!
//! A deliberately tiny crate whose only job is to register the statically
//! linked [`sqlite-vec`] extension with SQLite so that downstream crates
//! (notably [`ft_search`]) can use the `vec0` virtual table for vector search.
//!
//! ## Why this crate exists
//!
//! The `sqlite-vec` crate exposes a single C entrypoint, `sqlite3_vec_init`,
//! that must be handed to SQLite via `sqlite3_auto_extension`. That call is
//! `unsafe`, and the Firetrail workspace sets `unsafe_code = "forbid"`
//! workspace-wide — a `forbid` that cannot be relaxed with a local
//! `#![allow(unsafe_code)]`. Rather than weaken the lint for `ft-search`,
//! the one unavoidable `unsafe` call lives here, in a crate that opts out of
//! the workspace lint table. Everything else stays unsafe-free.
//!
//! ## Usage
//!
//! Call [`register`] once before opening any SQLite connection that needs
//! vector search. Registration is process-global and idempotent: the first
//! call performs the registration and probes that it worked; every subsequent
//! call returns the cached result. Because `sqlite3_auto_extension` only
//! affects connections opened *after* it runs, callers must `register()`
//! before `Connection::open`.

use std::sync::OnceLock;

/// Cached outcome of the one-time registration + probe.
static REGISTERED: OnceLock<bool> = OnceLock::new();

/// Register the `sqlite-vec` extension with SQLite for all connections opened
/// afterward in this process.
///
/// Returns `true` when the `vec0` module is available (verified by probing a
/// throwaway in-memory connection for `vec_version()`), `false` otherwise.
/// The work happens exactly once; later calls return the cached result.
#[must_use]
pub fn register() -> bool {
    *REGISTERED.get_or_init(|| {
        // SAFETY: `sqlite3_vec_init` is the extension entrypoint exported by
        // the statically-linked `sqlite-vec` crate; the transmute to the
        // auto-extension function-pointer type is the exact pattern documented
        // by `sqlite-vec` itself. `sqlite3_auto_extension` registers the
        // module for connections opened afterward and is safe to call once at
        // startup. The `OnceLock` guarantees this runs at most once.
        //
        // The transmute reinterprets a bare fn pointer as SQLite's
        // auto-extension callback type; spelling that FFI type out adds no
        // safety and couples us to libsqlite3-sys internals, so we allow the
        // un-annotated form (as upstream `sqlite-vec` does).
        #[allow(clippy::missing_transmute_annotations)]
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite_vec::sqlite3_vec_init as *const (),
            )));
        }

        // Probe a throwaway connection: registration succeeded only if the
        // `vec_version()` scalar resolves on a freshly opened connection.
        match rusqlite::Connection::open_in_memory() {
            Ok(conn) => conn
                .query_row("SELECT vec_version()", [], |r| r.get::<_, String>(0))
                .is_ok(),
            Err(_) => false,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn register_enables_vec_version() {
        assert!(
            register(),
            "register() should report the vec0 module available"
        );
        let conn = Connection::open_in_memory().unwrap();
        let version: String = conn
            .query_row("SELECT vec_version()", [], |r| r.get(0))
            .expect("vec_version() should resolve after register()");
        assert!(
            version.starts_with('v'),
            "vec_version() should return a version string, got {version:?}"
        );
    }
}
