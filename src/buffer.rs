//! Fixed-capacity ring buffer used for scrollback-like history.
//!
//! The emulator needs bounded memory while still preserving recently scrolled
//! rows. A circular buffer gives O(1) append and indexed reads without moving
//! existing elements, which is ideal for high-frequency terminal scrolling.

/// A fixed-capacity circular buffer. Generic over T so it
/// can store any kind of row (or anything else, really).
pub struct RingBuffer<T> {
    buf: Vec<Option<T>>,
    /// Index where the next push will write.
    head: usize,
    /// Number of valid items currently stored (0..=capacity).
    len: usize,
    /// Maximum number of items the buffer can hold.
    capacity: usize,
}

impl<T> RingBuffer<T> {
    /// Create a new ring buffer with the given fixed capacity.
    pub fn new(capacity: usize) -> Self {
        let mut buf = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            buf.push(None);
        }
        RingBuffer { buf, head: 0, len: 0, capacity }
    }

    /// Push a new item. If the buffer is full, the oldest item is overwritten.
    /// O(1) — no shifting, no allocation.
    pub fn push(&mut self, item: T) {
        self.buf[self.head] = Some(item);
        self.head = (self.head + 1) % self.capacity;
        if self.len < self.capacity {
            self.len += 1;
        }
    }

    /// Number of items currently stored.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the buffer holds zero items.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Get the i-th oldest item (0 = oldest, len-1 = newest).
    /// Returns None if the index is out of range.
    pub fn get(&self, i: usize) -> Option<&T> {
        if i >= self.len {
            return None;
        }
        // Oldest item lives at (head - len) mod capacity.
        let start = (self.head + self.capacity - self.len) % self.capacity;
        let idx = (start + i) % self.capacity;
        self.buf[idx].as_ref()
    }

    /// Iterate from oldest to newest.
    pub fn iter(&self) -> RingIter<'_, T> {
        RingIter { ring: self, pos: 0 }
    }
}

/// Iterator over the ring buffer in oldest-to-newest order.
pub struct RingIter<'a, T> {
    ring: &'a RingBuffer<T>,
    pos: usize,
}

impl<'a, T> Iterator for RingIter<'a, T> {
    type Item = &'a T;
    fn next(&mut self) -> Option<Self::Item> {
        let item = self.ring.get(self.pos)?;
        self.pos += 1;
        Some(item)
    }
}
