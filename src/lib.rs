//! A dependency-free vector index and single-process persistent database.
//!
//! Coordinates are f32; squared L2 distances accumulate in f64. The HNSW
//! graph is implemented here and rebuilt from authoritative records on open.
//!
//! ```
//! use vector::{Config, VectorIndex};
//! let mut index = VectorIndex::new(Config::new(3))?;
//! index.put(1, &[1.0, 0.0, 0.0], "first")?;
//! index.put(2, &[0.0, 1.0, 0.0], "second")?;
//! let found = index.search_hnsw(&[1.0, 0.1, 0.0], 2, 64)?;
//! assert_eq!(found.neighbors[0].id, 1);
//! index.delete(1);
//! assert_eq!(index.search_exact(&[1.0, 0.1, 0.0], 2)?.neighbors[0].id, 2);
//! # Ok::<(), vector::Error>(())
//! ```
#![forbid(unsafe_code)]

pub mod bench;
mod config;
mod db;
mod error;
mod index;
mod math;
mod rng;
mod storage;

pub use config::Config;
pub use db::{Database, RecoveryInfo};
pub use error::{Error, Result};
pub use index::{IndexStats, Neighbor, Record, SearchMode, SearchReport, VectorIndex};
