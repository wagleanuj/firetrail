//! Workspace handle used by every op.
//!
//! The canonical implementation now lives in the shared `ft-workspace` crate
//! (extracted in firetrail-jyc) so ft-cli, ft-ops, and ft-ui resolve workspace
//! paths from one place. ft-ops re-exports it here, and maps
//! [`ft_workspace::WorkspaceError`] into [`crate::error::OpsError`] (see the
//! `From` impl in `crate::error`) so existing `?`-propagating call sites keep
//! their transport-agnostic error type.

pub use ft_workspace::{Workspace, WorkspaceError, find_git_root};
