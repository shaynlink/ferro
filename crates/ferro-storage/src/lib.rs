//! Persistent Ferro adapters backed by embedded `LiliaDB`.
//!
//! Platform hosts use Ferro contracts; this crate is the sole dependency boundary for `LiliaDB`.

mod liliadb;

pub use liliadb::{
    LiliaApplicationStorage, LiliaDocumentRepository, PersistentRecentWorkspaceStore,
    WorkspaceDatabase,
};
