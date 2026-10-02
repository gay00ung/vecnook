/// Borrowed operations in an atomic, ordered database batch.
#[derive(Clone, Copy, Debug)]
pub enum Mutation<'a> {
    Put {
        id: u64,
        vector: &'a [f32],
        metadata: &'a str,
    },
    Delete {
        id: u64,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BatchReport {
    /// One sequence per committed batch, irrespective of operation count.
    pub sequence: u64,
    pub inserted: usize,
    pub updated: usize,
    pub deleted: usize,
    pub absent: usize,
}
