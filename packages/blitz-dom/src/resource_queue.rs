//! Request/completion leases bound the entire pipeline, including errors.
use crate::{ResourceLimits, document::DocumentEvent};
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, mpsc};
use std::task::{Context, Poll, Waker};

#[derive(Clone)]
pub(crate) struct DocumentSender {
    tx: mpsc::Sender<DocumentEvent>,
    pub pool: Arc<RequestPool>,
}
impl DocumentSender {
    pub fn new(tx: mpsc::Sender<DocumentEvent>) -> Self {
        Self {
            tx,
            pool: Arc::new(RequestPool::default()),
        }
    }
    pub fn send(&self, event: DocumentEvent) -> Result<(), mpsc::SendError<DocumentEvent>> {
        self.tx.send(event)
    }
}
#[derive(Default)]
struct Slot {
    bytes: usize,
    delivered: bool,
    cancelled: bool,
    waker: Option<Waker>,
}
#[derive(Default)]
struct State {
    limits: ResourceLimits,
    slots: BTreeMap<usize, Slot>,
    next: usize,
    bytes: usize,
    rejected: u64,
    closed: bool,
    notified: bool,
}
#[derive(Default)]
pub(crate) struct RequestPool(Mutex<State>);
impl RequestPool {
    pub fn register(self: &Arc<Self>) -> Option<Ticket> {
        let mut state = self.0.lock().expect("resource pool");
        if state.closed || state.slots.len() >= state.limits.requests || state.next == usize::MAX {
            state.rejected = state.rejected.saturating_add(1);
            return None;
        }
        let id = state.next;
        state.next += 1;
        state.slots.insert(id, Slot::default());
        Some(Ticket(Arc::new(Lease {
            pool: self.clone(),
            id,
        })))
    }
    pub fn set_limits(&self, limits: ResourceLimits) -> Result<(), &'static str> {
        let wakers = {
            let mut state = self.0.lock().expect("resource pool");
            if state.slots.len() > limits.requests || state.bytes > limits.completion_bytes {
                return Err("new resource limits are below live completion leases");
            }
            state.limits = limits;
            state
                .slots
                .values_mut()
                .filter_map(|slot| slot.waker.take())
                .collect::<Vec<_>>()
        };
        for waker in wakers {
            waker.wake();
        }
        Ok(())
    }
    pub fn stats(&self) -> (usize, usize, usize, usize, u64) {
        let state = self.0.lock().expect("resource pool");
        (
            state.slots.len(),
            state.slots.values().filter(|s| s.delivered).count(),
            state.bytes,
            state.slots.values().filter(|s| s.waker.is_some()).count(),
            state.rejected,
        )
    }
    pub fn observe(&self) {
        self.0.lock().expect("resource pool").notified = false;
    }
    pub fn cancel(&self, id: usize) {
        let waker = {
            let mut state = self.0.lock().expect("resource pool");
            state.slots.get_mut(&id).and_then(|slot| {
                slot.cancelled = true;
                slot.waker.take()
            })
        };
        if let Some(waker) = waker {
            waker.wake();
        }
        // Queued payloads retain their bytes until the actual last lease drops.
    }
    pub fn close(&self) {
        let wakers = {
            let mut state = self.0.lock().expect("resource pool");
            state.closed = true;
            state
                .slots
                .values_mut()
                .filter_map(|slot| slot.waker.take())
                .collect::<Vec<_>>()
        };
        for waker in wakers {
            waker.wake();
        }
    }
}
struct Lease {
    pool: Arc<RequestPool>,
    id: usize,
}
impl Drop for Lease {
    fn drop(&mut self) {
        let (removed, wakers) = {
            let mut state = self.pool.0.lock().expect("resource pool");
            let removed = state.slots.remove(&self.id);
            state.bytes -= removed.as_ref().map_or(0, |slot| slot.bytes);
            let wakers = state
                .slots
                .values_mut()
                .filter_map(|slot| slot.waker.take())
                .collect::<Vec<_>>();
            (removed, wakers)
        };
        drop(removed);
        for waker in wakers {
            waker.wake();
        }
    }
}
#[derive(Clone)]
pub(crate) struct Ticket(Arc<Lease>);
impl Ticket {
    pub fn id(&self) -> usize {
        self.0.id
    }
    pub fn limits(&self) -> ResourceLimits {
        self.0.pool.0.lock().expect("resource pool").limits
    }
    pub fn live(&self) -> bool {
        let state = self.0.pool.0.lock().expect("resource pool");
        !state.closed
            && state
                .slots
                .get(&self.id())
                .is_some_and(|slot| !slot.cancelled)
    }
    pub fn reserve(&self, bytes: usize, waker: Option<Waker>) -> Poll<Result<(), String>> {
        let (result, previous, wake) = {
            let mut state = self.0.pool.0.lock().expect("resource pool");
            let Some(slot) = state.slots.get(&self.id()) else {
                return Poll::Ready(Err("retired resource".into()));
            };
            if state.closed || slot.cancelled || slot.delivered {
                return Poll::Ready(Err("retired resource".into()));
            }
            if bytes > state.limits.completion_bytes {
                state.rejected = state.rejected.saturating_add(1);
                return Poll::Ready(Err("resource completion byte budget".into()));
            }
            let before = slot.bytes;
            let fits = bytes <= state.limits.completion_bytes - (state.bytes - before);
            if fits {
                state.bytes = state.bytes - before + bytes;
            }
            let slot = state.slots.get_mut(&self.id()).expect("resource slot");
            let previous = slot.waker.take();
            if fits {
                slot.bytes = bytes;
            } else {
                slot.waker = waker;
            }
            let wake = if fits && bytes < before {
                state
                    .slots
                    .values_mut()
                    .filter_map(|slot| slot.waker.take())
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            (
                if fits {
                    Poll::Ready(Ok(()))
                } else {
                    Poll::Pending
                },
                previous,
                wake,
            )
        };
        drop(previous);
        for waker in wake {
            waker.wake();
        }
        result
    }
    pub fn wait(&self, bytes: usize) -> Reserve {
        Reserve {
            ticket: self.clone(),
            bytes,
            done: false,
        }
    }
    pub fn publish(&self) -> Option<bool> {
        let mut state = self.0.pool.0.lock().expect("resource pool");
        if state.closed {
            return None;
        }
        let slot = state.slots.get_mut(&self.id())?;
        if slot.cancelled || slot.delivered {
            return None;
        }
        slot.delivered = true;
        let notify = !state.notified;
        state.notified = true;
        Some(notify)
    }
    pub fn reject(&self) {
        let mut state = self.0.pool.0.lock().expect("resource pool");
        state.rejected = state.rejected.saturating_add(1);
    }
}
pub(crate) struct Reserve {
    ticket: Ticket,
    bytes: usize,
    done: bool,
}
impl Future for Reserve {
    type Output = Result<(), String>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let result = self.ticket.reserve(self.bytes, Some(cx.waker().clone()));
        self.done = result.is_ready();
        result
    }
}
impl Drop for Reserve {
    fn drop(&mut self) {
        if !self.done {
            let previous = self
                .ticket
                .0
                .pool
                .0
                .lock()
                .expect("resource pool")
                .slots
                .get_mut(&self.ticket.id())
                .and_then(|slot| slot.waker.take());
            drop(previous);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Weak,
        atomic::{AtomicUsize, Ordering},
    };
    struct Wake {
        pool: Weak<RequestPool>,
        count: Arc<AtomicUsize>,
        payload: Option<Weak<Vec<u8>>>,
    }
    impl std::task::Wake for Wake {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            if let Some(payload) = &self.payload {
                assert!(
                    payload.upgrade().is_none(),
                    "payload must be dropped before budget is refunded"
                );
            }
            self.pool.upgrade().unwrap().stats(); // Reenter; no pool lock held.
            self.count.fetch_add(1, Ordering::Relaxed);
        }
    }
    impl Drop for Wake {
        fn drop(&mut self) {
            if let Some(pool) = self.pool.upgrade() {
                pool.stats();
            }
        }
    }
    fn pool() -> Arc<RequestPool> {
        let pool = Arc::new(RequestPool::default());
        pool.set_limits(ResourceLimits {
            requests: 3,
            completion_bytes: 32,
            ..Default::default()
        })
        .unwrap();
        pool
    }
    fn make_waker(pool: &Arc<RequestPool>) -> (Waker, Arc<AtomicUsize>) {
        let count = Arc::new(AtomicUsize::new(0));
        (
            Waker::from(Arc::new(Wake {
                pool: Arc::downgrade(pool),
                count: count.clone(),
                payload: None,
            })),
            count,
        )
    }
    #[test]
    fn queued_cancel_does_not_refund_memory_until_actual_result_drop() {
        let pool = pool();
        let first = pool.register().unwrap();
        let second = pool.register().unwrap();
        assert!(matches!(first.reserve(32, None), Poll::Ready(Ok(()))));
        assert_eq!(first.publish(), Some(true));
        let (waker, count) = make_waker(&pool);
        assert!(second.reserve(32, Some(waker)).is_pending());
        let queued = first.clone();
        drop(first);
        pool.cancel(queued.id());
        assert_eq!(pool.stats(), (2, 1, 32, 1, 0));
        assert_eq!(count.load(Ordering::Relaxed), 0);
        drop(queued);
        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert!(matches!(second.reserve(32, None), Poll::Ready(Ok(()))));
        drop(second);
        assert_eq!(pool.stats(), (0, 0, 0, 0, 0));
    }
    #[test]
    fn dropping_a_queued_response_frees_payload_before_waking_budget_waiters() {
        let pool = pool();
        let first = pool.register().unwrap();
        let second = pool.register().unwrap();
        assert!(matches!(first.reserve(32, None), Poll::Ready(Ok(()))));
        assert_eq!(first.publish(), Some(true));
        let payload = Arc::new(vec![0; 32]);
        let count = Arc::new(AtomicUsize::new(0));
        let waker = Waker::from(Arc::new(Wake {
            pool: Arc::downgrade(&pool),
            count: count.clone(),
            payload: Some(Arc::downgrade(&payload)),
        }));
        assert!(second.reserve(32, Some(waker)).is_pending());
        let response = crate::net::ResourceLoadResponse {
            request_id: first.id(),
            node_id: None,
            resolved_url: None,
            result: Ok(crate::net::Resource::Image(
                crate::util::ImageType::Image,
                8,
                1,
                payload,
            )),
            ticket: Some(first),
        };
        drop(response);
        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert!(matches!(second.reserve(32, None), Poll::Ready(Ok(()))));
    }
    #[test]
    fn errors_and_rejections_never_grow_a_second_queue_or_wrap_identities() {
        let pool = pool();
        let mut tickets = Vec::new();
        for index in 0..3 {
            let ticket = pool.register().unwrap();
            assert_eq!(ticket.id(), index);
            assert_eq!(ticket.publish(), Some(index == 0));
            tickets.push(ticket);
        }
        for _ in 0..1000 {
            assert!(pool.register().is_none());
        }
        assert_eq!(pool.stats(), (3, 3, 0, 0, 1000));
        drop(tickets);
        pool.0.lock().unwrap().next = usize::MAX;
        assert!(pool.register().is_none());
        assert_eq!(pool.0.lock().unwrap().next, usize::MAX);
    }
    #[test]
    fn close_wakes_waiters_and_never_reopens_but_keeps_live_byte_charges() {
        let pool = pool();
        let first = pool.register().unwrap();
        let second = pool.register().unwrap();
        assert!(matches!(first.reserve(32, None), Poll::Ready(Ok(()))));
        let (waker, count) = make_waker(&pool);
        assert!(second.reserve(32, Some(waker)).is_pending());
        pool.close();
        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert!(pool.register().is_none());
        assert!(matches!(second.reserve(32, None), Poll::Ready(Err(_))));
        assert_eq!(pool.stats().2, 32);
        drop(first);
        drop(second);
        assert_eq!(pool.stats().0, 0);
        assert_eq!(pool.stats().2, 0);
    }
    #[test]
    fn dropping_waiters_and_shrinking_reservations_wakes_outside_locks() {
        let pool = pool();
        let first = pool.register().unwrap();
        let second = pool.register().unwrap();
        assert!(matches!(first.reserve(32, None), Poll::Ready(Ok(()))));
        let (waker, count) = make_waker(&pool);
        assert!(second.reserve(1, Some(waker)).is_pending());
        assert!(matches!(first.reserve(0, None), Poll::Ready(Ok(()))));
        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert!(matches!(second.reserve(1, None), Poll::Ready(Ok(()))));
        assert!(
            pool.set_limits(ResourceLimits {
                completion_bytes: 0,
                ..Default::default()
            })
            .is_err()
        );
        let (waker, _) = make_waker(&pool);
        assert!(first.reserve(32, Some(waker)).is_pending());
        drop(first);
        drop(second);
        assert_eq!(pool.stats().0, 0);
    }
    #[test]
    fn raising_limits_wakes_a_waiter_and_dropping_pending_future_releases_its_waker() {
        let pool = pool();
        let first = pool.register().unwrap();
        let second = pool.register().unwrap();
        assert!(matches!(first.reserve(32, None), Poll::Ready(Ok(()))));
        let (waker, count) = make_waker(&pool);
        let mut cx = Context::from_waker(&waker);
        let mut future = second.wait(32);
        assert!(Pin::new(&mut future).poll(&mut cx).is_pending());
        assert_eq!(pool.stats().3, 1);
        drop(future);
        assert_eq!(pool.stats().3, 0);
        let mut future = second.wait(32);
        assert!(Pin::new(&mut future).poll(&mut cx).is_pending());
        pool.set_limits(ResourceLimits {
            completion_bytes: 64,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert!(matches!(
            Pin::new(&mut future).poll(&mut cx),
            Poll::Ready(Ok(()))
        ));
        drop(future);
        drop(first);
        drop(second);
        assert_eq!(pool.stats().2, 0);
    }
}
