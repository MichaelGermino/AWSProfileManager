//! AWS Profile Manager 2.0 core: everything the app does, with no UI dependency.
//!
//! Ported module by module from the 1.x TypeScript (docs/native-rewrite-plan.md §3.3). Each port
//! is pinned by the shared fixtures in `native/tests/fixtures`: pure functions by the vectors
//! (`tests/vectors.rs`), behaviour by the scenarios (run through `dispatch`, later in Phase 3).

pub mod credentials_ini;
pub mod crypto;
pub mod grouping;
pub mod js;
pub mod master_password;
pub mod network_classifier;
pub mod org_config;
pub mod saml_roles;
pub mod sso_org;
pub mod terminal_profile;
