use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, SyncSender, Receiver, TryRecvError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;

#[derive(Clone)]
pub struct Stack<T> {
    push: SyncSender<T>,
    pop: Arc<Pop<T>>,
}

impl<T: Send + 'static> Stack<T> {
    pub fn new() -> Self {
       Self::with_buf_size(0)
    }

    pub fn with_buf_size(buf_size: usize) -> Self {
        let (push_tx, push_rx) = mpsc::sync_channel(buf_size);
        let pop = Arc::new(Pop::new());

        Server {
            push: push_rx,
            pop: pop.clone(),
        }.start();

        Stack {
            push: push_tx,
            pop: pop,
        }
    }

    pub fn pop(&self) -> Option<T> {
        self.pop.pop()
    }

    pub fn push(&self, val: T) {
        self.push.send(val).unwrap();
    }
}

struct Pop<T> {
    len: AtomicUsize,
    cvar: Condvar,
    val: Mutex<Option<T>>,
}

impl<T> Pop<T> {
    fn new() -> Self {
        Pop {
            len: AtomicUsize::new(0),
            cvar: Condvar::new(),
            val: Mutex::new(None),
        }
    }

    fn pop(&self) -> Option<T> {
        let mut maybe = None;
        let mut guard = None;

        while maybe.is_none() {
            if self.len.load(Ordering::Relaxed) == 0 {
                break;
            }

            let mut guard_ = guard.unwrap_or_else(|| self.val.lock().unwrap());

            maybe = guard_.take();

            if maybe.is_some() {
                break;
            }

            guard = Some(self.cvar.wait(guard_).unwrap());
        }

        maybe
    }
}

struct Server<T> {
    push: Receiver<T>,
    pop: Arc<Pop<T>>,
}

impl<T: Send + 'static> Server<T> {
    fn start(self) {
        thread::spawn(move || { self.serve(0); });
    }

    fn serve(&self, len: usize) -> Option<MutexGuard<Option<T>>> {
        loop {
            self.store_len(len);
            
            match self.push.try_recv() {
                Ok(val) => if let Some(mut guard) = self.serve(len + 1) {
                    *guard = Some(val);
                    drop(guard);
                    self.store_len(len);
                    self.pop.cvar.notify_one();
                } else { 
                    return None;
                },
                Err(TryRecvError::Empty) => {
                    let guard = self.pop.val.lock().unwrap();

                    if guard.is_none() {
                        return Some(guard);
                    }
                },
                Err(TryRecvError::Disconnected) => return None,
            }
        }
    }

    fn store_len(&self, len: usize) {
        self.pop.len.store(len, Ordering::Relaxed);
    }
}
