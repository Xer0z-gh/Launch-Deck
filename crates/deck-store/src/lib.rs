//! Persistence for Launch Deck: `SQLite` via `SQLx`.
//!
//! One database file in the app data directory holds registered projects, run
//! history and app settings. Three rules shape this crate:
//!
//! 1. **Repositories speak domain types.** Everything public here accepts and
//!    returns `deck_domain` types -- a caller never sees a row, a column name,
//!    or an `sqlx` type. The mapping lives in one private module so a schema
//!    change touches exactly one place.
//! 2. **Log lines never enter the database.** A run row stores a pointer to
//!    its log file; the bytes stay on disk. See the log-pipeline notes in
//!    `docs/ARCHITECTURE.md`.
//! 3. **Every query is exercised against a real database in tests.** Queries
//!    are runtime-bound (no `sqlx::query!` macros): the offline-cache workflow
//!    the macros require adds a toolchain step (`sqlx-cli`, re-prepare on every
//!    schema change) without adding coverage beyond what the repository tests
//!    already prove at `cargo test` time. A typo'd column still cannot reach a
//!    user -- it fails the suite instead of the build.

#![deny(unsafe_code)]
#![warn(missing_docs, clippy::all, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod diagnostics;
mod map;
mod projects;
mod runs;
mod settings;
mod store;

pub use diagnostics::StoreDiagnostics;
pub use store::Store;
