//! Replay detection for nonces or frame identifiers.

use std::collections::{BTreeSet, VecDeque};

use crate::security::SecurityError;

/// Bounded replay-protection structure.
///
/// The protector remembers the most recent unique values up to `capacity`.
/// Attempting to insert the same value again returns `ReplayDetected`.
#[derive(Debug, Clone)]
pub struct ReplayProtector {
    capacity: usize,
    order: VecDeque<Vec<u8>>,
    seen: BTreeSet<Vec<u8>>,
}

impl ReplayProtector {
    pub fn new(capacity: usize) -> Result<Self, SecurityError> {
        if capacity == 0 {
            return Err(SecurityError::InvalidCapacity);
        }

        Ok(Self {
            capacity,
            order: VecDeque::with_capacity(capacity),
            seen: BTreeSet::new(),
        })
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn check_and_insert(&mut self, value: impl Into<Vec<u8>>) -> Result<(), SecurityError> {
        let value = value.into();
        if self.seen.contains(&value) {
            return Err(SecurityError::ReplayDetected);
        }

        if self.order.len() == self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.seen.remove(&oldest);
            }
        }

        self.seen.insert(value.clone());
        self.order.push_back(value);
        Ok(())
    }
}
