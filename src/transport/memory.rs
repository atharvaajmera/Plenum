//! In-memory transport simulator for tests.

use std::collections::VecDeque;

use crate::transport::{Transport, TransportError, TransportResult};

/// Deterministic network behavior controls for `MemoryTransport`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MemoryTransportConfig {
    /// Number of ticks a frame waits before becoming receivable.
    pub latency_ticks: u64,
    /// Drops every Nth send attempt when set.
    pub drop_every: Option<u64>,
    /// Duplicates every Nth accepted send when set.
    pub duplicate_every: Option<u64>,
    /// Delays every Nth accepted send by one extra tick when set, allowing
    /// later frames to arrive first.
    pub reorder_every: Option<u64>,
    /// Maximum number of frames that may be buffered across pending and ready
    /// queues.
    pub max_buffered_frames: Option<usize>,
}

/// Single-ended loopback transport used to test stream and protocol behavior
/// without sockets.
#[derive(Debug, Default)]
pub struct MemoryTransport {
    config: MemoryTransportConfig,
    current_tick: u64,
    send_attempts: u64,
    accepted_sends: u64,
    closed: bool,
    pending: VecDeque<PendingFrame>,
    ready: VecDeque<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingFrame {
    deliver_at: u64,
    bytes: Vec<u8>,
}

impl MemoryTransport {
    pub fn new(config: MemoryTransportConfig) -> Self {
        Self {
            config,
            ..Self::default()
        }
    }

    pub fn config(&self) -> MemoryTransportConfig {
        self.config
    }

    pub fn current_tick(&self) -> u64 {
        self.current_tick
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    pub fn buffered_len(&self) -> usize {
        self.pending.len() + self.ready.len()
    }

    /// Advances simulated time by one tick and moves deliverable frames into the
    /// receive queue.
    pub fn tick(&mut self) {
        self.current_tick = self.current_tick.saturating_add(1);
        self.flush_ready_frames();
    }

    pub fn advance_ticks(&mut self, ticks: u64) {
        for _ in 0..ticks {
            self.tick();
        }
    }

    fn should_apply(send_index: u64, interval: Option<u64>) -> bool {
        matches!(interval, Some(value) if value > 0 && send_index % value == 0)
    }

    fn ensure_capacity(&self, requested: usize) -> TransportResult<()> {
        if let Some(capacity) = self.config.max_buffered_frames {
            if self.buffered_len() + requested > capacity {
                return Err(TransportError::BufferFull {
                    capacity,
                    requested,
                });
            }
        }

        Ok(())
    }

    fn enqueue_pending(&mut self, deliver_at: u64, bytes: Vec<u8>) {
        self.pending.push_back(PendingFrame { deliver_at, bytes });
        self.flush_ready_frames();
    }

    fn flush_ready_frames(&mut self) {
        let mut remaining = VecDeque::new();

        while let Some(frame) = self.pending.pop_front() {
            if frame.deliver_at <= self.current_tick {
                self.ready.push_back(frame.bytes);
            } else {
                remaining.push_back(frame);
            }
        }

        self.pending = remaining;
    }
}

impl Transport for MemoryTransport {
    fn send(&mut self, bytes: &[u8]) -> TransportResult<()> {
        if self.closed {
            return Err(TransportError::Closed);
        }

        self.send_attempts = self.send_attempts.saturating_add(1);

        if Self::should_apply(self.send_attempts, self.config.drop_every) {
            return Ok(());
        }

        self.accepted_sends = self.accepted_sends.saturating_add(1);

        let copies = if Self::should_apply(self.accepted_sends, self.config.duplicate_every) {
            2
        } else {
            1
        };

        self.ensure_capacity(copies)?;

        let extra_reorder_delay =
            if Self::should_apply(self.accepted_sends, self.config.reorder_every) {
                1
            } else {
                0
            };
        let deliver_at = self
            .current_tick
            .saturating_add(self.config.latency_ticks)
            .saturating_add(extra_reorder_delay);

        for _ in 0..copies {
            self.enqueue_pending(deliver_at, bytes.to_vec());
        }

        Ok(())
    }

    fn recv(&mut self) -> TransportResult<Option<Vec<u8>>> {
        if self.closed {
            return Err(TransportError::Closed);
        }

        self.flush_ready_frames();
        Ok(self.ready.pop_front())
    }

    fn close(&mut self) -> TransportResult<()> {
        self.closed = true;
        self.pending.clear();
        self.ready.clear();
        Ok(())
    }

    fn is_closed(&self) -> bool {
        self.closed
    }
}
