//! Logging setup — 設計書.md §15.
//!
//! Always keeps the last 500 log lines in memory (for the future
//! diagnostics screen) and mirrors everything to stdout. File logging with
//! size-based rotation (1MB x 5) is explicitly deferred — `tracing-appender`
//! only supports time-based rotation, and §25's Phase 1 bullet list has no
//! logging item at all, so implementing a custom size-aware writer is
//! pushed to the stabilization phase.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::prelude::*;

const RING_BUFFER_CAPACITY: usize = 500;

#[derive(Clone)]
pub struct LogRingBuffer {
    inner: Arc<Mutex<VecDeque<String>>>,
    capacity: usize,
}

impl LogRingBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
            capacity,
        }
    }

    /// Returns a snapshot of the most recent log lines, oldest first.
    pub fn recent_logs(&self) -> Vec<String> {
        self.inner
            .lock()
            .expect("log ring buffer mutex poisoned")
            .iter()
            .cloned()
            .collect()
    }

    fn push_line(&self, line: &str) {
        if line.is_empty() {
            return;
        }
        let mut buf = self.inner.lock().expect("log ring buffer mutex poisoned");
        if buf.len() >= self.capacity {
            buf.pop_front();
        }
        buf.push_back(line.to_string());
    }
}

struct RingBufferWriter {
    buffer: LogRingBuffer,
}

impl std::io::Write for RingBufferWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        let text = String::from_utf8_lossy(data);
        for line in text.lines() {
            self.buffer.push_line(line);
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(Clone)]
struct RingBufferMakeWriter {
    buffer: LogRingBuffer,
}

impl<'a> MakeWriter<'a> for RingBufferMakeWriter {
    type Writer = RingBufferWriter;

    fn make_writer(&'a self) -> Self::Writer {
        RingBufferWriter {
            buffer: self.buffer.clone(),
        }
    }
}

/// Initializes the global `tracing` subscriber (stdout + in-memory ring
/// buffer) and returns a handle to the ring buffer for the diagnostics
/// screen. Must be called exactly once, as early as possible in `main`.
pub fn init(default_level: &str, file_logging_requested: bool) -> LogRingBuffer {
    let ring = LogRingBuffer::new(RING_BUFFER_CAPACITY);
    let ring_writer = RingBufferMakeWriter {
        buffer: ring.clone(),
    };

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default_level));

    // Default timer is UTC, which reads as several hours off from what the
    // user sees on their own clock — use the local offset instead.
    let timer = tracing_subscriber::fmt::time::LocalTime::rfc_3339();
    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stdout)
        .with_timer(timer.clone());
    let ring_layer = tracing_subscriber::fmt::layer()
        .with_writer(ring_writer)
        .with_ansi(false)
        .with_timer(timer);

    tracing_subscriber::registry()
        .with(filter)
        .with(stdout_layer)
        .with(ring_layer)
        .init();

    if file_logging_requested {
        tracing::warn!(
            "file logging is requested in config but not yet implemented; continuing with stdout + in-memory logging only"
        );
    }

    ring
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_caps_at_capacity_and_drops_oldest() {
        let ring = LogRingBuffer::new(3);
        ring.push_line("a");
        ring.push_line("b");
        ring.push_line("c");
        ring.push_line("d");
        assert_eq!(ring.recent_logs(), vec!["b", "c", "d"]);
    }

    #[test]
    fn ring_buffer_ignores_empty_lines() {
        let ring = LogRingBuffer::new(3);
        ring.push_line("");
        ring.push_line("hello");
        assert_eq!(ring.recent_logs(), vec!["hello"]);
    }
}
