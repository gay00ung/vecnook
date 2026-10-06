/// Borrowed operations in an atomic, ordered database batch.
#[derive(Clone, Copy, Debug)]
pub enum Mutation<'a> {
    /// Insert/replace an ID, observing earlier batch operations.
    Put {
        /// Application-assigned active ID.
        id: u64,
        /// Finite coordinates matching the configured dimensionality.
        vector: &'a [f32],
        /// Opaque UTF-8 payload, at most 16 KiB.
        metadata: &'a str,
    },
    /// Remove an ID, observing earlier batch operations.
    Delete {
        /// ID to remove; absence is a no-op.
        id: u64,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
/// Counts from an ordered atomic mutation batch.
pub struct BatchReport {
    /// One sequence per committed batch, irrespective of operation count.
    pub sequence: u64,
    /// Puts whose ID was absent at that point in the ordered batch.
    pub inserted: usize,
    /// Puts that replaced an active ID.
    pub updated: usize,
    /// Deletes that removed an active ID.
    pub deleted: usize,
    /// Deletes of an already absent ID.
    pub absent: usize,
}
