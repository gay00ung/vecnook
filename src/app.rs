//! A bounded search/write worker for GUI/event-loop applications. No framework dependencies.
use crate::{
    BatchReport, Collection, Document, DocumentFilter, DocumentMutation, DocumentSearchReport,
    EmbeddingSpace, SearchOptions,
};
use std::{
    fmt,
    sync::mpsc::{self, Receiver, SyncSender, TrySendError},
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

enum Request {
    Search {
        vector: Vec<f32>,
        k: usize,
        source: Option<String>,
        tags: Vec<String>,
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
            reply,
        })?;
        Ok(receiver)
    }

    fn enqueue(&self, request: Request) -> Result<(), SubmitError> {
        match self.sender.try_send(request) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(SubmitError::Busy),
            Err(TrySendError::Disconnected(_)) => Err(SubmitError::Stopped),
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
    let worker = thread::Builder::new()
        .name("vecnook-search".into())
        .spawn(move || {
            while let Ok(request) = receiver.recv() {
                match request {
                    Request::Search {
                        vector,
                        k,
                        source,
                        tags,
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
            }
            collection
        })?;
    Ok((SearchClient { sender, space }, worker))
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
