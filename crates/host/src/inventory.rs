//! Short-lived inventory snapshots shared by overlapping views. Invalidation
//! has a generation so an event arriving during a fetch cannot revive stale data.

use docker::Result;
use model::{Container, Image, Network, Volume};
use std::future::Future;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

pub struct Cache<T>(Mutex<Option<(String, Instant, Result<T>)>>);

impl<T> Default for Cache<T> {
    fn default() -> Self {
        Self(Mutex::new(None))
    }
}

impl<T: Clone> Cache<T> {
    pub async fn get(&self, key: String, fetch: impl Future<Output = Result<T>>) -> Result<T> {
        let mut slot = self.0.lock().await;
        if let Some((old, at, value)) = slot.as_ref() {
            let ttl = if value.is_ok() {
                Duration::from_secs(2)
            } else {
                Duration::from_millis(100)
            };
            if *old == key && at.elapsed() < ttl {
                return value.clone();
            }
        }
        let result = fetch.await;
        *slot = Some((key, Instant::now(), result.clone()));
        result
    }
}

#[derive(Default)]
pub struct Inventory {
    revision: AtomicU64,
    pub containers: [Cache<Vec<Container>>; 2],
    pub images: [Cache<Vec<Image>>; 2],
    pub volumes: Cache<Vec<Volume>>,
    pub networks: Cache<Vec<Network>>,
}

impl Inventory {
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }
    pub fn invalidate(&self) {
        self.revision.fetch_add(1, Ordering::AcqRel);
    }
    pub fn mutation(self: &Arc<Self>) -> Mutation {
        self.invalidate();
        Mutation(Arc::clone(self))
    }
}

pub struct Mutation(Arc<Inventory>);
impl Drop for Mutation {
    fn drop(&mut self) {
        self.0.invalidate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn overlapping_readers_share_one_fetch_and_new_generations_refetch() {
        let cache = Cache::default();
        let calls = AtomicU64::new(0);
        let fetch = || async {
            calls.fetch_add(1, Ordering::Relaxed);
            tokio::task::yield_now().await;
            Ok(7)
        };
        let (a, b) = tokio::join!(
            cache.get("1".into(), fetch()),
            cache.get("1".into(), fetch())
        );
        assert_eq!((a.unwrap(), b.unwrap()), (7, 7));
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        cache.get("2".into(), fetch()).await.unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn cancelled_mutations_invalidate_both_sides() {
        let inventory = Arc::new(Inventory::default());
        let guard = inventory.mutation();
        assert_eq!(inventory.revision(), 1);
        drop(guard);
        assert_eq!(inventory.revision(), 2);
    }
}
