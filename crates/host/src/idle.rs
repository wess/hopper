//! Opt-in empty-engine suspension. Event sockets are disconnected during the
//! final check so they cannot be confused with an external Docker client.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};
use tokio::sync::watch;

pub struct Idle {
    pub events: watch::Sender<bool>,
    pub active_events: AtomicUsize,
    since: Mutex<Option<Instant>>,
}

impl Default for Idle {
    fn default() -> Self {
        Self {
            events: watch::channel(false).0,
            active_events: AtomicUsize::new(0),
            since: Mutex::new(None),
        }
    }
}

impl Idle {
    #[cfg(test)]
    pub fn expire(&self) {
        *self.since.lock().unwrap() = Some(Instant::now() - Duration::from_secs(120));
    }

    pub fn reset(&self) {
        *self.since.lock().unwrap() = None;
    }
    pub fn due(&self, delay: Duration) -> bool {
        self.since
            .lock()
            .unwrap()
            .get_or_insert_with(Instant::now)
            .elapsed()
            >= delay
    }
    pub fn event(self: &Arc<Self>) -> Event {
        self.active_events.fetch_add(1, Ordering::AcqRel);
        Event(Arc::clone(self))
    }
    pub fn pause(self: &Arc<Self>) -> Pause {
        self.events.send_replace(true);
        Pause(Arc::clone(self))
    }
    pub async fn drained(&self) -> bool {
        tokio::time::timeout(Duration::from_secs(2), async {
            while self.active_events.load(Ordering::Acquire) != 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .is_ok()
    }
}

pub struct Event(Arc<Idle>);
impl Drop for Event {
    fn drop(&mut self) {
        self.0.active_events.fetch_sub(1, Ordering::AcqRel);
    }
}
pub struct Pause(Arc<Idle>);
impl Drop for Pause {
    fn drop(&mut self) {
        self.0.events.send_replace(false);
        self.0.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn event_drain_and_pause_are_cancel_safe() {
        let idle = Arc::new(Idle::default());
        let event = idle.event();
        let pause = idle.pause();
        assert!(*idle.events.borrow());
        drop(event);
        assert!(idle.drained().await);
        drop(pause);
        assert!(!*idle.events.borrow());
        assert!(!idle.due(Duration::from_secs(1)));
        assert!(idle.due(Duration::ZERO));
        idle.reset();
        assert!(!idle.due(Duration::from_secs(1)));
    }
}
