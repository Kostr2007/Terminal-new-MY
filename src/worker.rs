//! One worker per I/O class, one pending request, and generation cancellation.
//! Dropping never waits for a remote filesystem operation to return.
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicU64, Ordering},
    mpsc,
};

pub struct Cancel {
    generation: Arc<AtomicU64>,
    id: u64,
}
impl Cancel {
    pub fn cancelled(&self) -> bool {
        self.generation.load(Ordering::Relaxed) != self.id
    }
    pub fn fresh() -> Self {
        Self {
            generation: Arc::new(AtomicU64::new(0)),
            id: 0,
        }
    }
}
struct Slot<Q> {
    request: Option<(u64, Q)>,
    closed: bool,
}
pub struct Worker<Q, R> {
    slot: Arc<(Mutex<Slot<Q>>, Condvar)>,
    generation: Arc<AtomicU64>,
    receiver: mpsc::Receiver<(u64, R)>,
}
impl<Q: Send + 'static, R: Send + 'static> Worker<Q, R> {
    pub fn new(mut execute: impl FnMut(Q, &Cancel) -> R + Send + 'static) -> Self {
        let slot = Arc::new((
            Mutex::new(Slot::<Q> {
                request: None,
                closed: false,
            }),
            Condvar::new(),
        ));
        let generation = Arc::new(AtomicU64::new(0));
        let (sender, receiver) = mpsc::channel();
        let shared = slot.clone();
        let current = generation.clone();
        std::thread::spawn(move || {
            loop {
                let (id, request) = {
                    let (mutex, wake) = &*shared;
                    let mut slot = mutex.lock().unwrap();
                    while slot.request.is_none() && !slot.closed {
                        slot = wake.wait(slot).unwrap();
                    }
                    if slot.closed {
                        return;
                    }
                    slot.request.take().unwrap()
                };
                let cancel = Cancel {
                    generation: current.clone(),
                    id,
                };
                let result = execute(request, &cancel);
                if !cancel.cancelled() && sender.send((id, result)).is_err() {
                    return;
                }
            }
        });
        Self {
            slot,
            generation,
            receiver,
        }
    }
    pub fn submit(&self, request: Q) -> u64 {
        let id = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let (mutex, wake) = &*self.slot;
        mutex.lock().unwrap().request = Some((id, request));
        wake.notify_one();
        id
    }
    pub fn cancel(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.slot.0.lock().unwrap().request = None;
    }
    pub fn result(&self) -> Option<R> {
        let current = self.generation.load(Ordering::Relaxed);
        self.receiver
            .try_iter()
            .filter_map(|(id, r)| (id == current).then_some(r))
            .last()
    }
}
impl<Q, R> Drop for Worker<Q, R> {
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        let (mutex, wake) = &*self.slot;
        let mut slot = mutex.lock().unwrap();
        slot.closed = true;
        slot.request = None;
        wake.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn new_request_cancels_old_work_and_replaces_pending_slot() {
        let (started, ready) = mpsc::channel();
        let worker = Worker::new(move |n: u32, cancel: &Cancel| {
            if n == 1 {
                started.send(()).unwrap();
                while !cancel.cancelled() {
                    std::thread::yield_now();
                }
            }
            n
        });
        worker.submit(1);
        ready
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        worker.submit(2);
        worker.submit(3);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        loop {
            if let Some(result) = worker.result() {
                assert_eq!(result, 3);
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
}
