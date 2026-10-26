//! Optional platform acceleration, isolated from Vecnook's safe std-only core.
//!
//! SIMD retains the scalar reference's left-to-right f64 additions. Mapped
//! generations keep OS reader pins and are never written by Publisher.
//! File-backed mapping has an explicit immutability safety requirement.
#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]

mod kernels;
mod mapping;
pub use kernels::{Acceleration, AutoKernel};
pub use mapping::mapped_snapshot;
