//! Engine-wide statistics tracking.
//!
//! Thread-safe counters using AtomicU64, shared across all services.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Engine-wide statistics counters.
pub struct EngineStats {
    /// Total inbound connections accepted.
    pub connections_total: AtomicU64,
    /// Currently active inbound connections.
    pub connections_active: AtomicU64,
    /// Total inbound messages received and stored.
    pub messages_received: AtomicU64,
    /// Total outbound messages sent.
    pub messages_sent: AtomicU64,
    /// Total outbound messages that failed.
    pub messages_sent_failed: AtomicU64,
}

impl EngineStats {
    /// Create a new stats instance with all counters at zero.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            connections_total: AtomicU64::new(0),
            connections_active: AtomicU64::new(0),
            messages_received: AtomicU64::new(0),
            messages_sent: AtomicU64::new(0),
            messages_sent_failed: AtomicU64::new(0),
        })
    }

    /// Increment active connections (call on accept).
    pub fn connection_opened(&self) {
        self.connections_total.fetch_add(1, Ordering::Relaxed);
        self.connections_active.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement active connections (call on close).
    pub fn connection_closed(&self) {
        self.connections_active.fetch_sub(1, Ordering::Relaxed);
    }

    /// Record a received message.
    pub fn message_received(&self) {
        self.messages_received.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a sent message.
    pub fn message_sent(&self) {
        self.messages_sent.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a failed send.
    pub fn message_sent_failed(&self) {
        self.messages_sent_failed.fetch_add(1, Ordering::Relaxed);
    }

    /// Get a snapshot of all stats as JSON values.
    pub fn snapshot(&self) -> (u64, u64, u64, u64, u64) {
        (
            self.connections_total.load(Ordering::Relaxed),
            self.connections_active.load(Ordering::Relaxed),
            self.messages_received.load(Ordering::Relaxed),
            self.messages_sent.load(Ordering::Relaxed),
            self.messages_sent_failed.load(Ordering::Relaxed),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero_state() {
        let stats = EngineStats::new();
        let snap = stats.snapshot();
        assert_eq!(snap, (0, 0, 0, 0, 0));
    }

    #[test]
    fn test_connection_tracking() {
        let stats = EngineStats::new();
        stats.connection_opened();
        let snap = stats.snapshot();
        assert_eq!(snap.0, 1); // total
        assert_eq!(snap.1, 1); // active

        stats.connection_opened();
        stats.connection_opened();
        let snap = stats.snapshot();
        assert_eq!(snap.0, 3);
        assert_eq!(snap.1, 3);

        stats.connection_closed();
        let snap = stats.snapshot();
        assert_eq!(snap.0, 3);
        assert_eq!(snap.1, 2);
    }

    #[test]
    fn test_message_tracking() {
        let stats = EngineStats::new();

        stats.message_received();
        stats.message_received();
        stats.message_received();
        assert_eq!(stats.snapshot().2, 3);

        stats.message_sent();
        assert_eq!(stats.snapshot().3, 1);

        stats.message_sent_failed();
        stats.message_sent_failed();
        assert_eq!(stats.snapshot().4, 2);
    }

    #[test]
    fn test_mixed_operations() {
        let stats = EngineStats::new();
        stats.connection_opened();
        stats.connection_opened();
        stats.message_received();
        stats.connection_closed();
        stats.message_sent();

        let snap = stats.snapshot();
        assert_eq!(snap, (2, 1, 1, 1, 0));
    }
}
