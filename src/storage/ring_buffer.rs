use managed::ManagedSlice;
use storage::Resettable;

/// A ring buffer.
#[derive(Debug)]
pub struct RingBuffer<'a, T: 'a> {
    storage: ManagedSlice<'a, T>,
    read_at: usize,
    length: usize,
}

impl<'a, T: 'a> RingBuffer<'a, T> {
    /// Create a ring buffer with the given storage.
    ///
    /// During creation, every element in `storage` is reset.
    pub fn new<S>(storage: S) -> RingBuffer<'a, T>
        where S: Into<ManagedSlice<'a, T>>, T: Resettable,
    {
        let mut storage = storage.into();
        for elem in storage.iter_mut() {
            elem.reset();
        }

        RingBuffer {
            storage: storage,
            read_at: 0,
            length:  0,
        }
    }

    /// Create a ring buffer with the given storage.
    ///
    /// During creation, every element in `storage` set to Default::default().
    pub fn new_default<S>(storage: S) -> RingBuffer<'a, T>
        where S: Into<ManagedSlice<'a, T>>, T: Default,
    {
        let mut storage = storage.into();
        for elem in storage.iter_mut() {
            *elem = Default::default();
        }

        RingBuffer {
            storage: storage,
            read_at: 0,
            length:  0,
        }
    }

    fn mask(&self, index: usize) -> usize {
        index % self.storage.len()
    }

    fn incr(&self, index: usize) -> usize {
        self.mask(index + 1)
    }

    /// Query whether the buffer is empty.
    pub fn empty(&self) -> bool {
        self.length == 0
    }

    /// Query whether the buffer is full.
    pub fn full(&self) -> bool {
        self.length == self.storage.len()
    }

    /// Enqueue an element into the buffer, and return a pointer to it, or return
    /// `Err(())` if the buffer is full.
    pub fn enqueue(&mut self) -> Result<&mut T, ()> {
        if self.full() {
            Err(())
        } else {
            let index = self.mask(self.read_at + self.length);
            let result = &mut self.storage[index];
            self.length += 1;
            Ok(result)
        }
    }

    /// Dequeue an element from the buffer, and return a mutable reference to it, or return
    /// `Err(())` if the buffer is empty.
    pub fn dequeue(&mut self) -> Result<&mut T, ()> {
        if self.empty() {
            Err(())
        } else {
            self.length -= 1;
            let read_at = self.read_at;
            self.read_at = self.incr(self.read_at);
            let result = &mut self.storage[read_at];
            Ok(result)
        }
    }

    /// Get capacity of the underlying storage.
    pub fn capacity(&self) -> usize {
        self.storage.len()
    }

    /// Remove the first element equal to `value` from the buffer,
    /// reutrn `Err(())` if no such element was found.
    pub fn remove(&mut self, value: &T) -> Result<(), ()>
        where T: Eq + Copy {
        if self.empty() {
            return Err(());
        }

        let mut p = None;
        for i in 0..self.length {
            if self.storage[self.mask(self.read_at + i)] == *value {
                p = Some(i);
                break;
            }
        }

        if let Some(i) = p {
            for j in i..self.length-1 {
                self.storage[self.mask(self.read_at + j)] =
                    self.storage[self.mask(self.read_at + j + 1)];
            }
            self.length -= 1;
            Ok(())
        } else {
            Err(())
        }
    }

    #[cfg(not(any(feature = "std", feature = "collections")))]
    pub fn expand_storage(&mut self) {
        panic!("Cannot expand a ring_buffer in not-owned memory")
    }

    #[cfg(any(feature = "std", feature = "collections"))]
    pub fn expand_storage(&mut self)
        where T: Copy + Default {
        match self.storage {
            ManagedSlice::Borrowed(_) => {
                panic!("Cannot expand a ring_buffer in not-owned memory")
            }
            ManagedSlice::Owned(ref mut storage) => {
                storage.push(Default::default());
            }
        }
        if self.length != 0 && self.storage.len() > 1{
            let end = (self.read_at + self.length) % (self.storage.len() - 1);
            if end <= self.read_at {
                for j in self.length - end..self.length {
                    self.storage[self.mask(self.read_at + j)] =
                        self.storage[self.mask(self.read_at + j + 1)];
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    impl Resettable for usize {
        fn reset(&mut self) {
            *self = 0;
        }
    }

    #[test]
    pub fn expand_empty() {
        let mut ring_buffer = RingBuffer::new(vec![]);
        assert!(ring_buffer.empty());
        assert!(ring_buffer.full());
        assert_eq!(ring_buffer.enqueue(), Err(()));
        ring_buffer.expand_storage();
        *ring_buffer.enqueue().unwrap() = 123;
        assert!(!ring_buffer.empty());
        assert!(ring_buffer.full());
        assert_eq!(*ring_buffer.dequeue().unwrap(), 123);
        assert!(ring_buffer.empty());
        assert!(!ring_buffer.full());
    }

    #[test]
    pub fn test_buffer() {
        const TEST_BUFFER_SIZE: usize = 10;
        let mut storage = vec![];
        for i in 0..TEST_BUFFER_SIZE {
            storage.push(i + 10);
        }

        let mut ring_buffer = RingBuffer::new(storage);
        assert!(ring_buffer.empty());
        assert!(!ring_buffer.full());
        assert_eq!(ring_buffer.dequeue(), Err(()));
        ring_buffer.enqueue().unwrap();
        assert!(!ring_buffer.empty());
        assert!(!ring_buffer.full());
        for i in 1..TEST_BUFFER_SIZE/2 {
            *ring_buffer.enqueue().unwrap() = i;
            assert!(!ring_buffer.empty());
        }
        for i in 0..TEST_BUFFER_SIZE/2 {
            assert_eq!(*ring_buffer.dequeue().unwrap(), i);
            assert!(!ring_buffer.full());
        }
        for i in 0..TEST_BUFFER_SIZE {
            *ring_buffer.enqueue().unwrap() = i;
            assert!(!ring_buffer.empty());
        }
        assert!(ring_buffer.full());
        assert_eq!(ring_buffer.enqueue(), Err(()));
        ring_buffer.expand_storage();
        assert!(!ring_buffer.full());
        *ring_buffer.enqueue().unwrap() = TEST_BUFFER_SIZE;
        assert!(ring_buffer.full());
        assert_eq!(ring_buffer.enqueue(), Err(()));

        for i in 0..TEST_BUFFER_SIZE+1 {
            assert_eq!(*ring_buffer.dequeue().unwrap(), i);
            assert!(!ring_buffer.full());
        }
        assert_eq!(ring_buffer.dequeue(), Err(()));
        assert!(ring_buffer.empty());

        for i in &[1usize, 2, 1, 3, 1, 4, 1] {
            *ring_buffer.enqueue().unwrap() = *i;
            assert!(!ring_buffer.empty());
        }

        assert_eq!(ring_buffer.remove(&1), Ok(()));
        assert_eq!(ring_buffer.remove(&5), Err(()));
        assert_eq!(ring_buffer.remove(&4), Ok(()));
        assert_eq!(ring_buffer.remove(&4), Err(()));

        for i in &[2usize, 1, 3, 1, 1] {
            assert_eq!(*ring_buffer.dequeue().unwrap(), *i);
        }

        assert!(ring_buffer.empty());
    }
}
