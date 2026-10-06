//! A bounded query worker for GUI/event-loop applications. No framework dependencies.
use crate::{Collection, DocumentFilter, DocumentSearchReport, EmbeddingSpace, SearchOptions};
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
            Self::Busy => "search queue is full",
            Self::Stopped => "search worker has stopped",
        })
    }
}
impl std::error::Error for SubmitError {}

struct Request {
    vector: Vec<f32>,
    k: usize,
    source: Option<String>,
    tags: Vec<String>,
    reply: SyncSender<crate::Result<DocumentSearchReport>>,
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
        match self.sender.try_send(Request {
            vector,
            k,
            source,
            tags,
            reply,
        }) {
            Ok(()) => Ok(receiver),
            Err(TrySendError::Full(_)) => Err(SubmitError::Busy),
            Err(TrySendError::Disconnected(_)) => Err(SubmitError::Stopped),
        }
    }
}

/// Move one locked collection onto a named worker thread with a 32-request queue.
/// Drop all clients to drain queued queries and stop; joining returns the
/// collection so the application can checkpoint/mutate it during shutdown.
pub fn start(collection: Collection) -> std::io::Result<(SearchClient, JoinHandle<Collection>)> {
    let (sender, receiver) = mpsc::sync_channel::<Request>(32);
    let space = collection.space().clone();
    let worker = thread::Builder::new()
        .name("vecnook-search".into())
        .spawn(move || {
            while let Ok(request) = receiver.recv() {
                let tags: Vec<_> = request.tags.iter().map(String::as_str).collect();
                let report = collection.search(
                    &request.vector,
                    request.k,
                    SearchOptions::default(),
                    DocumentFilter {
                        source: request.source.as_deref(),
                        tags: &tags,
                    },
                );
                // A closed frontend receiver does not stop or poison the worker.
                let _ = request.reply.send(report);
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
