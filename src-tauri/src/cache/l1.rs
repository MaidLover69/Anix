use moka::future::Cache;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

/// In-memory cache with a fixed time-to-live per entry.
pub struct L1Cache<K, V>
where
    K: Send + Sync + 'static + std::hash::Hash + Eq + Clone,
    V: Send + Sync + 'static + Clone,
{
    cache: Cache<K, V>,
}

impl<K, V> L1Cache<K, V>
where
    K: Send + Sync + 'static + std::hash::Hash + Eq + Clone,
    V: Send + Sync + 'static + Clone,
{
    pub fn new(max_capacity: u64, ttl_secs: u64) -> Self {
        let cache = Cache::builder()
            .max_capacity(max_capacity)
            .time_to_live(Duration::from_secs(ttl_secs))
            .build();
        Self { cache }
    }

    pub async fn get(&self, key: &K) -> Option<V> {
        self.cache.get(key).await
    }

    pub async fn insert(&self, key: K, value: V) {
        self.cache.insert(key, value).await;
    }

    pub async fn invalidate(&self, key: &K) {
        self.cache.invalidate(key).await;
    }

    /// Returns the cached value or computes it with `init`. Concurrent callers for the same key
    /// share one computation, and an `Err` is handed to all of them without being cached.
    pub async fn try_get_with<F, E>(&self, key: K, init: F) -> Result<V, Arc<E>>
    where
        F: Future<Output = Result<V, E>>,
        E: Send + Sync + 'static,
    {
        self.cache.try_get_with(key, init).await
    }
}
