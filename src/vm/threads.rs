/// Thread support for mica VM.
///
/// This module provides OS thread support with:
/// - Thread spawning with independent VM instances
/// - Join handles for waiting on thread completion
/// - Channel-based communication between threads

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

use super::Value;

/// Thread ID counter for generating unique IDs.
static NEXT_THREAD_ID: AtomicUsize = AtomicUsize::new(1);

/// Generate a new unique thread ID.
fn next_thread_id() -> usize {
    NEXT_THREAD_ID.fetch_add(1, Ordering::Relaxed)
}

/// A handle to a spawned thread.
pub struct ThreadHandle {
    /// Unique thread ID
    pub id: usize,
    /// Join handle for the OS thread
    handle: Option<JoinHandle<Value>>,
    /// Whether the thread has been joined
    joined: bool,
}

impl ThreadHandle {
    /// Create a new thread handle.
    fn new(id: usize, handle: JoinHandle<Value>) -> Self {
        Self {
            id,
            handle: Some(handle),
            joined: false,
        }
    }

    /// Wait for the thread to complete and return its result.
    pub fn join(&mut self) -> Result<Value, String> {
        if self.joined {
            return Err("Thread already joined".to_string());
        }

        match self.handle.take() {
            Some(h) => {
                self.joined = true;
                h.join().map_err(|e| format!("Thread panicked: {:?}", e))
            }
            None => Err("Thread handle already taken".to_string()),
        }
    }

    /// Check if the thread has been joined.
    pub fn is_joined(&self) -> bool {
        self.joined
    }
}

/// Thread spawner that creates new threads running mica code.
pub struct ThreadSpawner {
    /// Active thread handles
    handles: Vec<ThreadHandle>,
}

impl ThreadSpawner {
    /// Create a new thread spawner.
    pub fn new() -> Self {
        Self {
            handles: Vec::new(),
        }
    }

    /// Spawn a new thread that runs the given closure.
    /// The closure should set up a VM and run mica code.
    pub fn spawn<F>(&mut self, f: F) -> usize
    where
        F: FnOnce() -> Value + Send + 'static,
    {
        let id = next_thread_id();
        let handle = thread::spawn(f);
        self.handles.push(ThreadHandle::new(id, handle));
        id
    }

    /// Get a thread handle by ID.
    pub fn get_handle(&mut self, id: usize) -> Option<&mut ThreadHandle> {
        self.handles.iter_mut().find(|h| h.id == id)
    }

    /// Join a thread by ID and return its result.
    pub fn join(&mut self, id: usize) -> Result<Value, String> {
        match self.get_handle(id) {
            Some(handle) => handle.join(),
            None => Err(format!("Thread {} not found", id)),
        }
    }

    /// Clean up finished threads.
    pub fn cleanup(&mut self) {
        self.handles.retain(|h| !h.joined);
    }
}

impl Default for ThreadSpawner {
    fn default() -> Self {
        Self::new()
    }
}

/// A channel for communication between threads.
///
/// Channels are multiple-producer, multiple-consumer (MPMC) queues.
pub struct Channel<T> {
    /// The message queue
    queue: Mutex<VecDeque<T>>,
    /// Condition variable for blocking recv
    not_empty: Condvar,
    /// Whether the channel is closed
    closed: AtomicBool,
    /// Number of messages sent
    sent_count: AtomicUsize,
    /// Number of messages received
    recv_count: AtomicUsize,
}

impl<T> Channel<T> {
    /// Create a new channel.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            queue: Mutex::new(VecDeque::new()),
            not_empty: Condvar::new(),
            closed: AtomicBool::new(false),
            sent_count: AtomicUsize::new(0),
            recv_count: AtomicUsize::new(0),
        })
    }

    /// Send a value through the channel.
    /// Returns Err if the channel is closed.
    pub fn send(&self, value: T) -> Result<(), T> {
        if self.closed.load(Ordering::Acquire) {
            return Err(value);
        }

        {
            let mut queue = self.queue.lock().unwrap();
            queue.push_back(value);
            self.sent_count.fetch_add(1, Ordering::Relaxed);
        }

        self.not_empty.notify_one();
        Ok(())
    }

    /// Receive a value from the channel, blocking if empty.
    /// Returns None if the channel is closed and empty.
    pub fn recv(&self) -> Option<T> {
        let mut queue = self.queue.lock().unwrap();

        loop {
            if let Some(value) = queue.pop_front() {
                self.recv_count.fetch_add(1, Ordering::Relaxed);
                return Some(value);
            }

            if self.closed.load(Ordering::Acquire) {
                return None;
            }

            queue = self.not_empty.wait(queue).unwrap();
        }
    }

    /// Try to receive a value without blocking.
    /// Returns None if the channel is empty.
    pub fn try_recv(&self) -> Option<T> {
        let mut queue = self.queue.lock().unwrap();
        let value = queue.pop_front();
        if value.is_some() {
            self.recv_count.fetch_add(1, Ordering::Relaxed);
        }
        value
    }

    /// Close the channel.
    /// No more values can be sent, but existing values can still be received.
    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.not_empty.notify_all();
    }

    /// Check if the channel is closed.
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// Get the number of messages currently in the queue.
    pub fn len(&self) -> usize {
        self.queue.lock().unwrap().len()
    }

    /// Check if the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get send/receive statistics.
    pub fn stats(&self) -> (usize, usize) {
        (
            self.sent_count.load(Ordering::Relaxed),
            self.recv_count.load(Ordering::Relaxed),
        )
    }
}

impl<T> Default for Channel<T> {
    fn default() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            not_empty: Condvar::new(),
            closed: AtomicBool::new(false),
            sent_count: AtomicUsize::new(0),
            recv_count: AtomicUsize::new(0),
        }
    }
}

/// A sender handle for a channel.
pub struct Sender<T> {
    channel: Arc<Channel<T>>,
}

impl<T> Sender<T> {
    /// Create a new sender for the given channel.
    pub fn new(channel: Arc<Channel<T>>) -> Self {
        Self { channel }
    }

    /// Send a value through the channel.
    pub fn send(&self, value: T) -> Result<(), T> {
        self.channel.send(value)
    }

    /// Close the channel.
    pub fn close(&self) {
        self.channel.close();
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            channel: Arc::clone(&self.channel),
        }
    }
}

/// A receiver handle for a channel.
pub struct Receiver<T> {
    channel: Arc<Channel<T>>,
}

impl<T> Receiver<T> {
    /// Create a new receiver for the given channel.
    pub fn new(channel: Arc<Channel<T>>) -> Self {
        Self { channel }
    }

    /// Receive a value from the channel, blocking if empty.
    pub fn recv(&self) -> Option<T> {
        self.channel.recv()
    }

    /// Try to receive a value without blocking.
    pub fn try_recv(&self) -> Option<T> {
        self.channel.try_recv()
    }

    /// Close the channel.
    pub fn close(&self) {
        self.channel.close();
    }
}

impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        Self {
            channel: Arc::clone(&self.channel),
        }
    }
}

/// Create a new channel and return (sender, receiver) pair.
pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let ch = Channel::new();
    (Sender::new(Arc::clone(&ch)), Receiver::new(ch))
}

/// A channel specialized for mica Values.
pub type ValueChannel = Channel<Value>;
pub type ValueSender = Sender<Value>;
pub type ValueReceiver = Receiver<Value>;

/// Create a new channel for mica Values.
pub fn value_channel() -> (ValueSender, ValueReceiver) {
    channel()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thread_spawn_and_join() {
        let mut spawner = ThreadSpawner::new();

        let id = spawner.spawn(|| {
            // Simulate some work
            std::thread::sleep(std::time::Duration::from_millis(10));
            Value::Int(42)
        });

        let result = spawner.join(id).unwrap();
        assert_eq!(result, Value::Int(42));
    }

    #[test]
    fn test_multiple_threads() {
        let mut spawner = ThreadSpawner::new();

        let id1 = spawner.spawn(|| Value::Int(1));
        let id2 = spawner.spawn(|| Value::Int(2));
        let id3 = spawner.spawn(|| Value::Int(3));

        let r1 = spawner.join(id1).unwrap();
        let r2 = spawner.join(id2).unwrap();
        let r3 = spawner.join(id3).unwrap();

        assert_eq!(r1, Value::Int(1));
        assert_eq!(r2, Value::Int(2));
        assert_eq!(r3, Value::Int(3));
    }

    #[test]
    fn test_channel_basic() {
        let (tx, rx) = channel::<i32>();

        tx.send(42).unwrap();
        tx.send(100).unwrap();

        assert_eq!(rx.recv(), Some(42));
        assert_eq!(rx.recv(), Some(100));
    }

    #[test]
    fn test_channel_try_recv() {
        let (tx, rx) = channel::<i32>();

        assert_eq!(rx.try_recv(), None);

        tx.send(42).unwrap();
        assert_eq!(rx.try_recv(), Some(42));
        assert_eq!(rx.try_recv(), None);
    }

    #[test]
    fn test_channel_close() {
        let (tx, rx) = channel::<i32>();

        tx.send(42).unwrap();
        tx.close();

        // Can still receive existing values
        assert_eq!(rx.recv(), Some(42));
        // But now recv returns None
        assert_eq!(rx.recv(), None);

        // Can't send after close
        assert!(tx.send(100).is_err());
    }

    #[test]
    fn test_channel_multi_producer() {
        let (tx, rx) = channel::<i32>();
        let tx2 = tx.clone();

        tx.send(1).unwrap();
        tx2.send(2).unwrap();
        tx.send(3).unwrap();

        let mut values = vec![];
        values.push(rx.recv().unwrap());
        values.push(rx.recv().unwrap());
        values.push(rx.recv().unwrap());

        assert_eq!(values, vec![1, 2, 3]);
    }

    #[test]
    fn test_channel_cross_thread() {
        let (tx, rx) = channel::<i32>();

        let handle = std::thread::spawn(move || {
            for i in 0..5 {
                tx.send(i).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            tx.close();
        });

        let mut values = vec![];
        while let Some(v) = rx.recv() {
            values.push(v);
        }

        handle.join().unwrap();
        assert_eq!(values, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn test_value_channel() {
        let (tx, rx) = value_channel();

        tx.send(Value::Int(42)).unwrap();
        tx.send(Value::Bool(true)).unwrap();
        tx.send(Value::Nil).unwrap();

        assert_eq!(rx.recv(), Some(Value::Int(42)));
        assert_eq!(rx.recv(), Some(Value::Bool(true)));
        assert_eq!(rx.recv(), Some(Value::Nil));
    }

    #[test]
    fn test_channel_stats() {
        let (tx, rx) = channel::<i32>();

        tx.send(1).unwrap();
        tx.send(2).unwrap();
        rx.recv();

        let (sent, recv) = tx.channel.stats();
        assert_eq!(sent, 2);
        assert_eq!(recv, 1);
    }
}
