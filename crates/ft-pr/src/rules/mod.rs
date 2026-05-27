//! Individual validation rules.
//!
//! Each submodule exposes a single `run(&ValidationContext, &mut PrReport)`
//! function (some return `Result<(), PrError>` when they can fail before
//! producing findings). The validator orchestrates them in [`crate::validator`].

pub(crate) mod ac_cap_exceeded;
pub(crate) mod chain_broken;
pub(crate) mod deprecated_reference;
pub(crate) mod draft_expired;
pub(crate) mod evidence_required;
pub(crate) mod incomplete_acceptance;
pub(crate) mod mixed_commit;
pub(crate) mod pr_link_missing;
pub(crate) mod secret_leak;
