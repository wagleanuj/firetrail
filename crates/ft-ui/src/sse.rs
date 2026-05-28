//! Server-Sent Events stream for the GUI.
//!
//! Subscribes to [`ft_ops::EventBus`] and forwards every [`ft_ops::EmittedEvent`]
//! envelope as an `event: emitted` SSE frame with a monotonic `id` for
//! `Last-Event-Id` based replay from the in-memory ring buffer.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::State,
    http::{HeaderMap, header},
    response::{
        Sse,
        sse::{Event as SseEvent, KeepAlive},
    },
};
use ft_ops::EmittedEvent;
use futures_util::stream::Stream;
use tokio_stream::wrappers::BroadcastStream;

use crate::server::AppState;

/// SSE handler for `GET /api/events`.
#[tracing::instrument(skip_all)]
pub async fn events_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let last_event_id: Option<u64> = headers
        .get("Last-Event-Id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .or_else(|| {
            headers
                .get(header::HeaderName::from_static("last-event-id"))
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse().ok())
        });

    let replay: Vec<(u64, EmittedEvent)> = {
        let ring = state.sse_ring.lock().expect("ring mutex");
        ring.replay_after(last_event_id)
    };

    let rx = state.events.subscribe();
    let live = BroadcastStream::new(rx);
    let seq_counter = state.sse_seq.clone();
    let ring = state.sse_ring.clone();

    let replay_stream = futures_util::stream::iter(replay.into_iter().map(|(seq, env)| {
        Ok::<_, Infallible>(encode_event(seq, &env))
    }));

    let live_stream = async_stream::stream! {
        let mut s = live;
        use futures_util::StreamExt;
        while let Some(item) = s.next().await {
            if let Ok(env) = item {
                let seq = seq_counter
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                    + 1;
                {
                    let mut r = ring.lock().expect("ring mutex");
                    r.push(seq, env.clone());
                }
                yield Ok::<_, Infallible>(encode_event(seq, &env));
            }
            // else: lagged or closed — skip and keep streaming.
        }
    };

    let combined = futures_util::stream::StreamExt::chain(replay_stream, live_stream);

    Sse::new(combined).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

fn encode_event(seq: u64, env: &EmittedEvent) -> SseEvent {
    let data = serde_json::to_string(env).unwrap_or_else(|_| "{}".to_string());
    SseEvent::default().id(seq.to_string()).event("emitted").data(data)
}

/// Bounded in-memory ring buffer of recent emitted events for SSE replay.
#[derive(Debug)]
pub struct RingBuffer<T> {
    capacity: usize,
    items: std::collections::VecDeque<(u64, T)>,
}

impl<T: Clone> RingBuffer<T> {
    /// Construct an empty ring buffer with the given capacity.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            items: std::collections::VecDeque::with_capacity(capacity),
        }
    }

    /// Push a `(seq, value)` pair, evicting the oldest entry if full.
    pub fn push(&mut self, seq: u64, value: T) {
        if self.items.len() == self.capacity {
            self.items.pop_front();
        }
        self.items.push_back((seq, value));
    }

    /// Return everything with `seq > after`. If `after` is `None`, returns
    /// nothing (clients without `Last-Event-Id` should not receive a flood of
    /// historic events).
    pub fn replay_after(&self, after: Option<u64>) -> Vec<(u64, T)> {
        let Some(a) = after else { return Vec::new() };
        self.items
            .iter()
            .filter(|(seq, _)| *seq > a)
            .cloned()
            .collect()
    }
}
