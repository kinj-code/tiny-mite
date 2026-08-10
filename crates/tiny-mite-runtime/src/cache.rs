//! Cache layers — LRU caches for embeddings, tokenization, and model metadata.
//!
//! Reduces redundant computation and network calls for frequently
//! accessed data. Designed for CPU-first, memory-constrained environments.

use std::collections::HashMap;
use std::hash::Hash;
use tokio::sync::Mutex;

// ── Generic LRU Cache ────────────────────────────────────────────

/// A bounded LRU cache for computed values.
///
/// Evicts least recently used entries when capacity is exceeded.
pub struct LruCache<K, V> {
    capacity: usize,
    entries: HashMap<K, (V, usize)>,
    /// Monotonic counter for tracking recency.
    tick: usize,
}

impl<K: Eq + Hash + Clone, V: Clone> LruCache<K, V> {
    /// Create a new LRU cache with the given capacity.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self { capacity, entries: HashMap::new(), tick: 0 }
    }

    /// Get a value by key, marking it as recently used.
    pub fn get(&mut self, key: &K) -> Option<&V> {
        let (val, tick) = self.entries.get_mut(key)?;
        *tick = self.tick;
        self.tick += 1;
        Some(val)
    }

    /// Insert a value, evicting LRU if full.
    pub fn insert(&mut self, key: K, value: V) {
        if self.entries.len() >= self.capacity {
            let lru =
                self.entries.iter().min_by_key(|(_, (_, tick))| *tick).map(|(k, _)| k.clone());
            if let Some(k) = lru {
                self.entries.remove(&k);
            }
        }
        self.entries.insert(key, (value, self.tick));
        self.tick += 1;
    }

    /// Number of cached entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clear all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

// ── Caches for Tiny Mite ─────────────────────────────────────────

/// Caches for various deterministic computations.
pub struct RuntimeCaches {
    /// Cache for tokenized text (avoids re-tokenizing).
    pub token_cache: Mutex<LruCache<String, Vec<i32>>>,
    /// Cache for embedding vectors.
    pub embedding_cache: Mutex<LruCache<String, Vec<f32>>>,
    /// Cache for model metadata queries.
    pub metadata_cache: Mutex<LruCache<String, String>>,
}

impl RuntimeCaches {
    /// Create a new set of runtime caches.
    #[must_use]
    pub fn new() -> Self {
        Self {
            token_cache: Mutex::new(LruCache::new(1024)),
            embedding_cache: Mutex::new(LruCache::new(256)),
            metadata_cache: Mutex::new(LruCache::new(64)),
        }
    }

    /// Create caches with custom capacities.
    #[must_use]
    pub fn with_capacities(tokens: usize, embeddings: usize, metadata: usize) -> Self {
        Self {
            token_cache: Mutex::new(LruCache::new(tokens)),
            embedding_cache: Mutex::new(LruCache::new(embeddings)),
            metadata_cache: Mutex::new(LruCache::new(metadata)),
        }
    }
}

impl Default for RuntimeCaches {
    fn default() -> Self {
        Self::new()
    }
}

// ── Memory Pressure Manager ──────────────────────────────────────

/// Monitors memory usage and triggers cache eviction under pressure.
pub struct MemoryPressureManager {
    /// High water mark (bytes) — above this, start evicting.
    high_water_mark_bytes: u64,
    /// Low water mark (bytes) — below this, stop evicting.
    low_water_mark_bytes: u64,
    /// Percentage of caches to evict on each pressure cycle.
    evict_fraction: f64,
}

impl MemoryPressureManager {
    /// Create a new memory pressure manager.
    #[must_use]
    pub fn new() -> Self {
        Self {
            high_water_mark_bytes: 12_884_901_888, // 12 GB
            low_water_mark_bytes: 8_589_934_592,   // 8 GB
            evict_fraction: 0.25,
        }
    }

    /// Returns true if memory is under pressure.
    #[must_use]
    pub fn is_under_pressure(&self, current_usage_bytes: u64) -> bool {
        current_usage_bytes > self.high_water_mark_bytes
    }

    /// Returns true if pressure has been relieved.
    #[must_use]
    pub fn is_pressure_relieved(&self, current_usage_bytes: u64) -> bool {
        current_usage_bytes < self.low_water_mark_bytes
    }

    /// Evict entries from the cache under pressure.
    pub async fn evict_if_pressured(&self, caches: &RuntimeCaches, current_usage_bytes: u64) {
        if self.is_under_pressure(current_usage_bytes) {
            let mut tc = caches.token_cache.lock().await;
            tc.clear();

            let mut ec = caches.embedding_cache.lock().await;
            ec.clear();
        }
    }
}

impl Default for MemoryPressureManager {
    fn default() -> Self {
        Self::new()
    }
}

// ── Latency Trace ────────────────────────────────────────────────

/// A single latency measurement.
#[derive(Debug, Clone)]
pub struct LatencySample {
    pub operation: String,
    pub duration_ms: f64,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Traces latency across operations.
pub struct LatencyTracer {
    samples: Vec<LatencySample>,
    max_samples: usize,
}

impl LatencyTracer {
    /// Create a new latency tracer.
    #[must_use]
    pub fn new(max_samples: usize) -> Self {
        Self { samples: Vec::with_capacity(max_samples), max_samples }
    }

    /// Record a latency sample.
    pub fn record(&mut self, operation: impl Into<String>, duration_ms: f64) {
        if self.samples.len() >= self.max_samples {
            self.samples.remove(0);
        }
        self.samples.push(LatencySample {
            operation: operation.into(),
            duration_ms,
            timestamp: chrono::Utc::now(),
        });
    }

    /// Average latency across all samples.
    #[must_use]
    pub fn average_ms(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        self.samples.iter().map(|s| s.duration_ms).sum::<f64>() / self.samples.len() as f64
    }

    /// Number of recorded samples.
    #[must_use]
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Returns true if no samples recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

impl Default for LatencyTracer {
    fn default() -> Self {
        Self::new(1000)
    }
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lru_cache_evicts_oldest() {
        let mut cache: LruCache<String, i32> = LruCache::new(2);
        cache.insert("a".into(), 1);
        cache.insert("b".into(), 2);
        cache.get(&"a".into());
        cache.insert("c".into(), 3); // should evict "b"
        assert!(cache.get(&"a".into()).is_some());
        assert!(cache.get(&"b".into()).is_none());
        assert!(cache.get(&"c".into()).is_some());
    }

    #[test]
    fn memory_pressure_detection() {
        let mgr = MemoryPressureManager::new();
        assert!(mgr.is_under_pressure(14_000_000_000));
        assert!(!mgr.is_under_pressure(4_000_000_000));
    }

    #[test]
    fn latency_tracer_average() {
        let mut tracer = LatencyTracer::new(100);
        tracer.record("tokenize", 10.0);
        tracer.record("tokenize", 20.0);
        assert_eq!(tracer.average_ms(), 15.0);
    }
}
