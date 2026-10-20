//! A bounded search/write worker for GUI/event-loop applications. No framework dependencies.
use crate::{
    BatchReport, CapacityStatus, Collection, Document, DocumentFilter, DocumentMutation,
    DocumentSearchReport, EmbeddingSpace, FilterExpr, HybridSearchOptions, HybridSearchReport,
    KeywordSearchReport, MaintenancePolicy, MaintenanceReport, MaintenanceStatus, SearchOptions,
};
use std::{
    fmt,
    path::PathBuf,
    sync::mpsc::{self, Receiver, SyncSender, TrySendError},
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
};

/// A failure to enqueue a GUI request; no database work occurred.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SubmitError {
    /// Wrong model identity or oversized request.
    InvalidRequest,
    /// The bounded queue is full; let the UI retry later.
    Busy,
    /// The worker has stopped.
    Stopped,
}
impl fmt::Display for SubmitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidRequest => "model, dimensions or request limits differ",
            Self::Busy => "application queue is full",
            Self::Stopped => "application worker has stopped",
        })
    }
}
impl std::error::Error for SubmitError {}

/// An owned atomic write operation for the application queue.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum WriteOperation {
    /// Insert or replace a document with coordinates from the worker's embedding space.
    Put {
        /// Original text, source, tags and chunk identity.
        document: Document,
        /// Original coordinates in the worker's model space.
        vector: Vec<f32>,
    },
    /// Remove an active ID. Deleting an absent ID is a no-op.
    Delete {
        /// ID to remove.
        id: u64,
    },
}

/// Owned eligibility for queued keyword and hybrid queries.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct QueryFilter {
    /// Optional exact source.
    pub source: Option<String>,
    /// Required tags.
    pub tags: Vec<String>,
    /// Optional Boolean/range expression, intersected with source and tags.
    pub expression: Option<FilterExpr>,
}
impl QueryFilter {
    fn valid(&self) -> bool {
        self.tags.len() <= 32
            && self
                .source
                .as_ref()
                .is_none_or(|s| !s.is_empty() && s.len() <= 1024 && !s.contains('\0'))
            && self
                .tags
                .iter()
                .all(|s| !s.is_empty() && s.len() <= 128 && !s.contains('\0'))
            && self
                .expression
                .as_ref()
                .is_none_or(|e| e.validate().is_ok())
    }
}

/// Owning-handle diagnostics, without opening the locked directory again.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct WorkerStatus {
    /// Current committed sequence.
    pub sequence: u64,
    /// Snapshot/node budget, not RSS.
    pub capacity: CapacityStatus,
    /// WAL pressure and recommendations for the requested policy.
    pub maintenance: MaintenanceStatus,
}

/// Queue activity accessible even while synchronous maintenance is running.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct WorkerActivity {
    /// Accepted requests that have not started.
    pub pending: usize,
    /// Name of the current operation, or None while idle/stopped.
    pub running: Option<&'static str>,
}

enum Request {
    Keywords {
        text: String,
        k: usize,
        filter: QueryFilter,
        reply: SyncSender<crate::Result<KeywordSearchReport>>,
    },
    Hybrid {
        vector: Vec<f32>,
        text: String,
        k: usize,
        options: HybridSearchOptions,
        filter: QueryFilter,
        reply: SyncSender<crate::Result<HybridSearchReport>>,
    },
    Status {
        policy: MaintenancePolicy,
        reply: SyncSender<crate::Result<WorkerStatus>>,
    },
    Compact {
        reply: SyncSender<crate::Result<usize>>,
    },
    Maintain {
        policy: MaintenancePolicy,
        reply: SyncSender<crate::Result<MaintenanceReport>>,
    },
    Backup {
        root: PathBuf,
        name: String,
        reply: SyncSender<crate::Result<()>>,
    },
    Search {
        vector: Vec<f32>,
        k: usize,
        source: Option<String>,
        tags: Vec<String>,
        expression: Option<FilterExpr>,
        reply: SyncSender<crate::Result<DocumentSearchReport>>,
    },
    Write {
        operations: Vec<WriteOperation>,
        expected_sequence: Option<u64>,
        reply: SyncSender<crate::Result<BatchReport>>,
    },
    Checkpoint {
        reply: SyncSender<crate::Result<()>>,
    },
}

/// Cloneable frontend handle. Submission never blocks on storage/search.
#[derive(Clone)]
pub struct SearchClient {
    sender: SyncSender<Request>,
    space: EmbeddingSpace,
    activity: Arc<Mutex<WorkerActivity>>,
}
impl SearchClient {
    /// Model/dimension/metric identity expected by this worker.
    pub fn space(&self) -> &EmbeddingSpace {
        &self.space
    }
    /// Submit a bounded query and return a one-result receiver. Poll it or await
    /// it on a background thread; do not block the GUI event loop on recv.
    /// Core query validation errors arrive through the receiver.
    pub fn submit(
        &self,
        model: &str,
        vector: Vec<f32>,
        k: usize,
        source: Option<String>,
        tags: Vec<String>,
    ) -> Result<Receiver<crate::Result<DocumentSearchReport>>, SubmitError> {
        if model != self.space.model
            || vector.len() != self.space.dimensions
            || k > 100
            || tags.len() > 32
            || source
                .as_ref()
                .is_some_and(|s| s.is_empty() || s.len() > 1024 || s.contains('\0'))
            || tags
                .iter()
                .any(|s| s.is_empty() || s.len() > 128 || s.contains('\0'))
        {
            return Err(SubmitError::InvalidRequest);
        }
        let (reply, receiver) = mpsc::sync_channel(1);
        self.enqueue(Request::Search {
            vector,
            k,
            source,
            tags,
            expression: None,
            reply,
        })?;
        Ok(receiver)
    }

    /// Submit an owned compound filter through the same bounded worker queue.
    /// Invalid expressions are rejected before enqueueing; core vector errors
    /// arrive through the receiver. K is at most 100, as for submit.
    pub fn submit_filtered(
        &self,
        model: &str,
        vector: Vec<f32>,
        k: usize,
        expression: FilterExpr,
    ) -> Result<Receiver<crate::Result<DocumentSearchReport>>, SubmitError> {
        if model != self.space.model
            || vector.len() != self.space.dimensions
            || k > 100
            || expression.validate().is_err()
        {
            return Err(SubmitError::InvalidRequest);
        }
        let (reply, receiver) = mpsc::sync_channel(1);
        self.enqueue(Request::Search {
            vector,
            k,
            source: None,
            tags: vec![],
            expression: Some(expression),
            reply,
        })?;
        Ok(receiver)
    }

    /// Inspect queue activity without waiting for a running database operation.
    pub fn activity(&self) -> WorkerActivity {
        self.activity
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Queue exact lexical search with the collection's selected analyzer.
    pub fn submit_keywords(
        &self,
        model: &str,
        text: String,
        k: usize,
        filter: QueryFilter,
    ) -> Result<Receiver<crate::Result<KeywordSearchReport>>, SubmitError> {
        if model != self.space.model
            || k > 100
            || !filter.valid()
            || crate::keyword::validate_query(&text).is_err()
        {
            return Err(SubmitError::InvalidRequest);
        }
        let (reply, receiver) = mpsc::sync_channel(1);
        self.enqueue(Request::Keywords {
            text,
            k,
            filter,
            reply,
        })?;
        Ok(receiver)
    }

    /// Queue RRF search. Submission validates limits; vector validation and search
    /// errors arrive through the receiver. Both branches use the same filter.
    pub fn submit_hybrid(
        &self,
        model: &str,
        vector: Vec<f32>,
        text: String,
        k: usize,
        options: HybridSearchOptions,
        filter: QueryFilter,
    ) -> Result<Receiver<crate::Result<HybridSearchReport>>, SubmitError> {
        if model != self.space.model
            || vector.len() != self.space.dimensions
            || k > 100
            || !filter.valid()
            || options.validate(k).is_err()
            || crate::keyword::validate_query(&text).is_err()
        {
            return Err(SubmitError::InvalidRequest);
        }
        let (reply, receiver) = mpsc::sync_channel(1);
        self.enqueue(Request::Hybrid {
            vector,
            text,
            k,
            options,
            filter,
            reply,
        })?;
        Ok(receiver)
    }

    /// Queue diagnostics on the owning handle; never reopens the locked files.
    pub fn status(
        &self,
        policy: MaintenancePolicy,
    ) -> Result<Receiver<crate::Result<WorkerStatus>>, SubmitError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.enqueue(Request::Status { policy, reply })?;
        Ok(receiver)
    }

    /// Queue synchronous compaction. Later requests wait; use activity to show it.
    pub fn compact(&self) -> Result<Receiver<crate::Result<usize>>, SubmitError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.enqueue(Request::Compact { reply })?;
        Ok(receiver)
    }

    /// Queue explicit policy-driven maintenance. No background auto-maintenance.
    pub fn maintain(
        &self,
        policy: MaintenancePolicy,
    ) -> Result<Receiver<crate::Result<MaintenanceReport>>, SubmitError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.enqueue(Request::Maintain { policy, reply })?;
        Ok(receiver)
    }

    /// Queue an independent named backup to an application-owned root. Existing
    /// destinations are refused; dropping the receiver does not cancel the backup.
    pub fn backup(
        &self,
        root: PathBuf,
        name: String,
    ) -> Result<Receiver<crate::Result<()>>, SubmitError> {
        if name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        {
            return Err(SubmitError::InvalidRequest);
        }
        let (reply, receiver) = mpsc::sync_channel(1);
        self.enqueue(Request::Backup { root, name, reply })?;
        Ok(receiver)
    }

    fn enqueue(&self, request: Request) -> Result<(), SubmitError> {
        let mut state = self.activity.lock().unwrap_or_else(|e| e.into_inner());
        state.pending += 1;
        match self.sender.try_send(request) {
            Ok(()) => Ok(()),
            Err(error) => {
                state.pending -= 1;
                Err(match error {
                    TrySendError::Full(_) => SubmitError::Busy,
                    TrySendError::Disconnected(_) => SubmitError::Stopped,
                })
            }
        }
    }

    /// Queue at most 64 operations and 256 KiB of encoded documents/coordinates.
    /// Writes and searches share one FIFO queue. An accepted batch is atomic;
    /// errors arrive through the receiver. Optional sequence guards reject stale state.
    /// Submission validates bounded payloads but never waits for storage.
    pub fn submit_batch(
        &self,
        model: &str,
        operations: Vec<WriteOperation>,
        expected_sequence: Option<u64>,
    ) -> Result<Receiver<crate::Result<BatchReport>>, SubmitError> {
        if model != self.space.model || operations.is_empty() || operations.len() > 64 {
            return Err(SubmitError::InvalidRequest);
        }
        let mut bytes = 13usize;
        for operation in &operations {
            bytes += match operation {
                WriteOperation::Put { document, vector } => {
                    if vector.len() != self.space.dimensions {
                        return Err(SubmitError::InvalidRequest);
                    }
                    let payload = document
                        .to_payload()
                        .map_err(|_| SubmitError::InvalidRequest)?;
                    13 + payload.len() + vector.len() * 4
                }
                WriteOperation::Delete { .. } => 9,
            };
            if bytes > 256 * 1024 {
                return Err(SubmitError::InvalidRequest);
            }
        }
        let (reply, receiver) = mpsc::sync_channel(1);
        self.enqueue(Request::Write {
            operations,
            expected_sequence,
            reply,
        })?;
        Ok(receiver)
    }

    /// Queue a checkpoint after all earlier accepted requests. It does not stop the worker.
    pub fn checkpoint(&self) -> Result<Receiver<crate::Result<()>>, SubmitError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.enqueue(Request::Checkpoint { reply })?;
        Ok(receiver)
    }
}

/// Move one locked collection onto a named worker thread with a 32-request queue.
/// Drop all clients to drain queued queries and stop; joining returns the
/// collection so the application can checkpoint/mutate it during shutdown.
pub fn start(
    mut collection: Collection,
) -> std::io::Result<(SearchClient, JoinHandle<Collection>)> {
    let (sender, receiver) = mpsc::sync_channel::<Request>(32);
    let space = collection.space().clone();
    let activity = Arc::new(Mutex::new(WorkerActivity::default()));
    let worker_activity = activity.clone();
    let worker = thread::Builder::new()
        .name("vecnook-search".into())
        .spawn(move || {
            while let Ok(request) = receiver.recv() {
                {
                    let mut state = worker_activity.lock().unwrap_or_else(|e| e.into_inner());
                    state.pending -= 1;
                    state.running = Some(match &request {
                        Request::Search { .. } => "vector",
                        Request::Keywords { .. } => "keyword",
                        Request::Hybrid { .. } => "hybrid",
                        Request::Status { .. } => "status",
                        Request::Compact { .. } => "compact",
                        Request::Maintain { .. } => "maintain",
                        Request::Backup { .. } => "backup",
                        Request::Write { .. } => "write",
                        Request::Checkpoint { .. } => "checkpoint",
                    });
                }
                match request {
                    Request::Keywords {
                        text,
                        k,
                        filter,
                        reply,
                    } => {
                        let tags: Vec<_> = filter.tags.iter().map(String::as_str).collect();
                        let eligibility = DocumentFilter::default()
                            .with_optional_source(filter.source.as_deref())
                            .with_tags(&tags)
                            .with_optional_expression(filter.expression.as_ref());
                        let _ = reply.send(collection.search_keywords(&text, k, eligibility));
                    }
                    Request::Hybrid {
                        vector,
                        text,
                        k,
                        options,
                        filter,
                        reply,
                    } => {
                        let tags: Vec<_> = filter.tags.iter().map(String::as_str).collect();
                        let eligibility = DocumentFilter::default()
                            .with_optional_source(filter.source.as_deref())
                            .with_tags(&tags)
                            .with_optional_expression(filter.expression.as_ref());
                        let _ = reply.send(collection.search_hybrid(
                            &vector,
                            &text,
                            k,
                            options,
                            eligibility,
                        ));
                    }
                    Request::Status { policy, reply } => {
                        let _ =
                            reply.send(collection.maintenance_status(policy).map(|maintenance| {
                                WorkerStatus {
                                    sequence: collection.sequence(),
                                    capacity: collection.capacity(),
                                    maintenance,
                                }
                            }));
                    }
                    Request::Compact { reply } => {
                        let _ = reply.send(collection.compact());
                    }
                    Request::Maintain { policy, reply } => {
                        let _ = reply.send(collection.maintain(policy));
                    }
                    Request::Backup { root, name, reply } => {
                        let _ = reply.send(collection.backup(root, &name));
                    }
                    Request::Search {
                        vector,
                        k,
                        source,
                        tags,
                        expression,
                        reply,
                    } => {
                        let tags: Vec<_> = tags.iter().map(String::as_str).collect();
                        let report = collection.search(
                            &vector,
                            k,
                            SearchOptions::default(),
                            DocumentFilter {
                                source: source.as_deref(),
                                tags: &tags,
                                expression: expression.as_ref(),
                            },
                        );
                        // A closed frontend receiver does not stop or poison the worker.
                        let _ = reply.send(report);
                    }
                    Request::Write {
                        operations,
                        expected_sequence,
                        reply,
                    } => {
                        let borrowed = operations
                            .iter()
                            .map(|operation| match operation {
                                WriteOperation::Put { document, vector } => {
                                    DocumentMutation::Put { document, vector }
                                }
                                WriteOperation::Delete { id } => {
                                    DocumentMutation::Delete { id: *id }
                                }
                            })
                            .collect::<Vec<_>>();
                        let report = match expected_sequence {
                            Some(sequence) => {
                                collection.write_batch_if_sequence(sequence, &borrowed)
                            }
                            None => collection.write_batch(&borrowed),
                        };
                        let _ = reply.send(report);
                    }
                    Request::Checkpoint { reply } => {
                        let _ = reply.send(collection.checkpoint());
                    }
                }
                worker_activity
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .running = None;
            }
            collection
        })?;
    Ok((
        SearchClient {
            sender,
            space,
            activity,
        },
        worker,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Metric;
    #[test]
    fn stalled_queue_reports_busy_and_disconnection_reports_stopped() {
        let (sender, receiver) = mpsc::sync_channel(32);
        let client = SearchClient {
            sender,
            space: EmbeddingSpace::new("fixture", 1, Metric::SquaredL2),
            activity: Arc::new(Mutex::new(WorkerActivity::default())),
        };
        for _ in 0..32 {
            client
                .submit("fixture", vec![1.0], 1, None, vec![])
                .unwrap();
        }
        assert!(matches!(
            client.submit("fixture", vec![1.0], 1, None, vec![]),
            Err(SubmitError::Busy)
        ));
        drop(receiver);
        assert!(matches!(
            client.submit("fixture", vec![1.0], 1, None, vec![]),
            Err(SubmitError::Stopped)
        ));
    }
}
