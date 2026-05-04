#![allow(dead_code)]

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
    ///
    /// # Panics
    /// Panics if `capacity` is zero — a zero-capacity buffer cannot hold
    /// anything and would cause division-by-zero in every operation.
    pub fn new(capacity: usize) -> Self {
        // FIX #1: guard against zero capacity before any indexing or modulo.
        assert!(capacity > 0, "RingBuffer capacity must be > 0");
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

    /// Maximum number of items this buffer can hold.
    // FIX #4: expose capacity so callers can compute fill ratio (len / capacity).
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Reset the buffer to an empty state without reallocating.
    // FIX #2: clear() method — avoids drop-and-recreate when reusing the buffer.
    pub fn clear(&mut self) {
        for slot in self.buf.iter_mut() {
            *slot = None;
        }
        self.head = 0;
        self.len = 0;
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

#[cfg(test)]
mod tests {
    use super::*;

    // FIX #1: zero capacity must panic
    #[test]
    #[should_panic(expected = "RingBuffer capacity must be > 0")]
    fn zero_capacity_panics() {
        RingBuffer::<u32>::new(0);
    }

    #[test]
    fn new_buffer_is_empty() {
        let rb = RingBuffer::<i32>::new(4);
        assert!(rb.is_empty());
        assert_eq!(rb.len(), 0);
        assert_eq!(rb.capacity(), 4);
    }

    // FIX #4: capacity() accessor
    #[test]
    fn capacity_accessor() {
        let rb = RingBuffer::<u8>::new(8);
        assert_eq!(rb.capacity(), 8);
    }

    #[test]
    fn push_and_get_basic() {
        let mut rb = RingBuffer::new(4);
        rb.push(10);
        rb.push(20);
        rb.push(30);
        assert_eq!(rb.len(), 3);
        assert_eq!(rb.get(0), Some(&10)); // oldest
        assert_eq!(rb.get(1), Some(&20));
        assert_eq!(rb.get(2), Some(&30)); // newest
        assert_eq!(rb.get(3), None);      // out of range
    }

    #[test]
    fn get_out_of_range_returns_none() {
        let mut rb = RingBuffer::new(4);
        rb.push(1);
        assert_eq!(rb.get(1), None);
        assert_eq!(rb.get(100), None);
    }

    // Core correctness: wrap-around index math
    #[test]
    fn wrap_around_overwrites_oldest() {
        let mut rb = RingBuffer::new(3);
        rb.push(1);
        rb.push(2);
        rb.push(3);
        // Buffer full: [1, 2, 3]
        rb.push(4); // overwrites 1 → [2, 3, 4]
        assert_eq!(rb.len(), 3);
        assert_eq!(rb.get(0), Some(&2)); // oldest is now 2
        assert_eq!(rb.get(1), Some(&3));
        assert_eq!(rb.get(2), Some(&4)); // newest
    }

    #[test]
    fn wrap_around_multiple_laps() {
        let mut rb = RingBuffer::new(3);
        for i in 0..9u32 {
            rb.push(i);
        }
        // After 9 pushes into capacity-3: last 3 are 6, 7, 8
        assert_eq!(rb.len(), 3);
        assert_eq!(rb.get(0), Some(&6));
        assert_eq!(rb.get(1), Some(&7));
        assert_eq!(rb.get(2), Some(&8));
    }

    // FIX #2: clear() tests
    #[test]
    fn clear_resets_state() {
        let mut rb = RingBuffer::new(4);
        rb.push(1);
        rb.push(2);
        rb.clear();
        assert!(rb.is_empty());
        assert_eq!(rb.len(), 0);
        assert_eq!(rb.get(0), None);
    }

    #[test]
    fn clear_then_reuse() {
        let mut rb = RingBuffer::new(3);
        rb.push(10);
        rb.push(20);
        rb.clear();
        rb.push(99);
        assert_eq!(rb.len(), 1);
        assert_eq!(rb.get(0), Some(&99));
    }

    #[test]
    fn clear_after_wrap_resets_correctly() {
        let mut rb = RingBuffer::new(3);
        rb.push(1);
        rb.push(2);
        rb.push(3);
        rb.push(4); // wraps head
        rb.clear();
        assert!(rb.is_empty());
        rb.push(42);
        assert_eq!(rb.get(0), Some(&42));
        assert_eq!(rb.len(), 1);
    }

    #[test]
    fn iter_empty() {
        let rb = RingBuffer::<i32>::new(4);
        assert_eq!(rb.iter().count(), 0);
    }

    #[test]
    fn iter_partial_fill() {
        let mut rb = RingBuffer::new(4);
        rb.push(1);
        rb.push(2);
        let collected: Vec<_> = rb.iter().copied().collect();
        assert_eq!(collected, vec![1, 2]);
    }

    #[test]
    fn iter_after_wrap() {
        let mut rb = RingBuffer::new(3);
        rb.push(1);
        rb.push(2);
        rb.push(3);
        rb.push(4); // oldest (1) overwritten
        let collected: Vec<_> = rb.iter().copied().collect();
        assert_eq!(collected, vec![2, 3, 4]);
    }

    #[test]
    fn capacity_1_works() {
        let mut rb = RingBuffer::new(1);
        rb.push(42);
        assert_eq!(rb.get(0), Some(&42));
        rb.push(99);
        assert_eq!(rb.get(0), Some(&99));
        assert_eq!(rb.len(), 1);
    }
}