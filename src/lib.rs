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
#![forbid(unsafe_code)]

mod batch;
pub mod bench;
mod config;
mod db;
mod error;
mod graph;
mod index;
mod math;
mod rng;
mod search;
mod storage;

pub use batch::{BatchReport, Mutation};
pub use config::{Config, Metric};
pub use db::{
    Database, MaintenanceAction, MaintenancePolicy, MaintenanceReport, MaintenanceStatus,
    RecoveryInfo,
};
pub use error::{Error, Result};
pub use index::{IndexStats, Neighbor, Record, SearchMode, SearchReport, VectorIndex};
pub use search::{SearchOptions, SearchReason, SearchStrategy};
