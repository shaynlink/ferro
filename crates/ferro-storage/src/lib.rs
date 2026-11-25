//! Persistent [`RecentWorkspaceStore`](ferro_contracts::RecentWorkspaceStore) adapters.
//!
//! `liliadb` is the default backend. The `sqlite` feature provides a deliberately small,
//! interoperable fallback. Exactly one backend feature must be selected.

#![cfg_attr(all(feature = "liliadb", feature = "sqlite"), allow(dead_code))]

#[cfg(all(feature = "liliadb", feature = "sqlite"))]
compile_error!("select exactly one storage backend: `liliadb` or `sqlite`");

#[cfg(not(any(feature = "liliadb", feature = "sqlite")))]
compile_error!("select a storage backend: `liliadb` or `sqlite`");

#[cfg(feature = "liliadb")]
mod liliadb;
#[cfg(feature = "sqlite")]
mod sqlite;

#[cfg(feature = "liliadb")]
pub use liliadb::PersistentRecentWorkspaceStore;
#[cfg(feature = "sqlite")]
pub use sqlite::PersistentRecentWorkspaceStore;
