//! A dependency-free vector index and single-process persistent database.
//!
//! Coordinates are f32; squared L2, cosine and negative inner product distances
//! accumulate in f64. HNSW is implemented here. A validated checkpoint cache
//! speeds reopening; missing or invalid caches rebuild from authoritative records.
//!
//! ```
//! use vecnook::{Config, VectorIndex};
//! let mut index = VectorIndex::new(Config::new(3))?;
//! index.put(1, &[1.0, 0.0, 0.0], "first")?;
//! index.put(2, &[0.0, 1.0, 0.0], "second")?;
//! let found = index.search_hnsw(&[1.0, 0.1, 0.0], 2, 64)?;
//! assert_eq!(found.neighbors[0].id, 1);
//! index.delete(1);
//! assert_eq!(index.search_exact(&[1.0, 0.1, 0.0], 2)?.neighbors[0].id, 2);
//! # Ok::<(), vecnook::Error>(())
//! ```
//!
//! Use [`SearchOptions`] for Auto search and inspect [`SearchReport::reason`].
//! Candidate completeness is independent of approximate search recall:
//!
//! ```
//! use vecnook::{Config, Metric, SearchOptions, VectorIndex};
//! let mut index = VectorIndex::new(Config::new(2).with_metric(Metric::Cosine))?;
//! index.put(7, &[1.0, 0.0], "project-a")?;
//! index.put(8, &[0.0, 1.0], "project-b")?;
//! let result = index.search_filtered(&[0.9, 0.1], 10, SearchOptions::default(), |r| {
//!     r.metadata == "project-a"
//! })?;
//! assert_eq!(result.eligible_count, 1);
//! assert_eq!(result.neighbors[0].id, 7);
//! # Ok::<(), vecnook::Error>(())
//! ```
//!
//! [`Database`] adds synchronization, exclusive ownership, WAL recovery and backups.
//! A storage [`Error::Io`] can mean a write persisted before the failure; close the
//! poisoned handle, reopen and inspect the affected ID before retrying.
//!
//! [`Collection`] adds model identity, original documents and indexed tag/source
//! filters. Documents have a bounded versioned payload for safe CLI batch import:
//!
//! ```
//! use vecnook::Document;
//! let mut doc = Document::new(7, "Original text\nwith newlines", "notes.md");
//! doc.tags = vec!["rust".into()];
//! assert_eq!(Document::from_payload(&doc.to_payload()?)?, doc);
//! # Ok::<(), vecnook::Error>(())
//! ```
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod app;
mod batch;
pub mod bench;
mod collection;
mod config;
mod db;
mod diagnostics;
mod document;
mod error;
mod graph;
mod index;
mod math;
mod rng;
mod search;
mod storage;
#[cfg(test)]
mod storage_fault_tests;
mod storage_io;
mod transfer;

pub use batch::{BatchReport, Mutation};
pub use collection::{
    Collection, DocumentFilter, DocumentMutation, DocumentNeighbor, DocumentSearchReport,
    EmbeddingSpace,
};
pub use config::{Config, Metric};
pub use db::{
    Database, MaintenanceAction, MaintenancePolicy, MaintenanceReport, MaintenanceStatus,
    RecoveryInfo,
};
pub use diagnostics::{CapacityStatus, DiagnosticReport, doctor};
pub use document::Document;
pub use error::{Error, ErrorKind, Result};
pub use index::{IndexStats, Neighbor, Record, SearchMode, SearchReport, VectorIndex};
pub use search::{SearchOptions, SearchReason, SearchStrategy};
