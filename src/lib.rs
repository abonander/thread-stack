use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, SyncSender, Receiver, TryRecvError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, LockResult, PoisonError};
use std::{mem, thread};

pub struct Stack<T> {
    inner: Arc<Inner<T>>,
}

impl<T: Send + 'static> Stack<T> {
    pub fn new() -> Self {
        let inner = Arc::new(Inner::new());

        Server {
            inner: inner.clone(),
        }.start();

        Stack {
            inner: inner,
        }
    }

    pub fn pop(&self) -> Option<T> {
        self.inner.pop()
    }

    pub fn push(&self, val: T) {
        self.inner.push(val);
    }
}

impl<T> Clone for Stack<T> {
    fn clone(&self) -> Self {
        self.inner.new_client();

        Stack {
            inner: self.inner.clone()
        }
    }
}

impl<T> Drop for Stack<T> {
    fn drop(&mut self) {
        self.inner.client_dropped();
    }
}

struct Inner<T> {
    clients: AtomicUsize,
    len: AtomicUsize,
    server: Condvar,
    val: Mutex<Option<T>>,
}

impl<T> Inner<T> {
    fn new() -> Self {
        Inner {
            clients: AtomicUsize::new(1),
            len: AtomicUsize::new(0),
            server: Condvar::new(),
            val: Mutex::new(None),
        }
    }

    fn pop(&self) -> Option<T> {
        let mut maybe = None;
        let mut guard = self.val.lock().unwrap();

        while maybe.is_none() {
            if self.len.load(Ordering::Relaxed) == 0 {
                break;
            }

            maybe = guard.take();

            if maybe.is_some() {
                self.len.fetch_sub(1, Ordering::Relaxed);
                break;
            }

            drop(guard);

            self.server.notify_one();

            guard = self.val.lock().unwrap();
        }

        drop(guard);

        self.server.notify_one();

        maybe
    }

    fn push(&self, val: T) {
        let mut guard = self.val.lock().unwrap();
        
        while guard.is_some() {
            drop(guard);
            self.server.notify_one();
            guard = self.val.lock().unwrap();
        }

        *guard = Some(val);

        self.len.fetch_add(1, Ordering::Relaxed);

        drop(guard);
        self.server.notify_one();
    }

    fn new_client(&self) {
        self.clients.fetch_add(1, Ordering::Relaxed);
    }

    fn client_dropped(&self) {
        if self.clients.fetch_sub(1, Ordering::Relaxed) == 1 {
            self.server.notify_one();
        }
    }
 
    fn len(&self) -> usize {
        self.len.load(Ordering::Relaxed)
    }

    fn clients(&self) -> usize {
        self.clients.load(Ordering::Relaxed)
    }
}

struct Server<T> {
    inner: Arc<Inner<T>>,
}

impl<T: Send + 'static> Server<T> {
    fn start(self) {
        thread::spawn(move || { 
            self.serve(0, self.inner.val.lock().unwrap());
        });
    }

    fn serve<'a>(&'a self, len: usize, mut guard: MutexGuard<'a, Option<T>>) -> LockResult<MutexGuard<'a, Option<T>>> {
        let mut val = None;
        loop {
            guard = try!(self.inner.server.wait(guard));

            if self.inner.clients() == 0 {
                return Err(PoisonError::new(guard));
            }

            let len_ = self.inner.len();
            
            if len_ > len { 
                guard = try!(self.serve(len + 1, guard));
            } else if len_ < len {
                return Ok(guard);
            }

            if guard.is_some() != val.is_some() {
                mem::swap(&mut val, &mut guard);
            }
        }
    }
}

#[test]
fn test_stack_basic() {
    let vals: Vec<u64> = (0 .. 5).collect();

    let stack = Stack::new();

    for &val in vals.iter().rev() {
        stack.push(val);
        println!("Push!");
    }

    let mut out_vals = Vec::new();

    while let Some(out) = stack.pop() {
        println!("Pop!");
        out_vals.push(out);
    }

    assert_eq!(vals, out_vals);
}
