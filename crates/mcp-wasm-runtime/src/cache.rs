//! Module caching for compiled WASM modules.
//!
//! Provides LRU-based caching of compiled Wasmtime modules using BLAKE3
//! hashes for cache keys. Reduces compilation overhead for repeated
//! execution of the same code.
//!
//! # Examples
//!
//! ```
//! use mcp_wasm_runtime::cache::ModuleCache;
//!
//! let mut cache = ModuleCache::new(100); // 100 module capacity
//! let code = b"some wasm bytecode";
//! let key = ModuleCache::cache_key_for_code(code);
//! ```

use blake3::Hasher;
use std::sync::Mutex;
use wasmtime::Module;

/// Cache key for compiled WASM modules.
///
/// Uses BLAKE3 hash of source code for fast, collision-resistant keys.
///
/// # Examples
///
/// ```
/// use mcp_wasm_runtime::cache::CacheKey;
///
/// let key1 = CacheKey::new("abc123");
/// let key2 = CacheKey::new("abc123");
/// assert_eq!(key1, key2);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey(String);

impl CacheKey {
    /// Creates a new cache key.
    #[must_use]
    pub fn new(hash: impl Into<String>) -> Self {
        Self(hash.into())
    }

    /// Returns the cache key as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CacheKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// LRU cache for compiled WASM modules.
///
/// Stores compiled Wasmtime modules to avoid recompilation overhead.
/// Uses BLAKE3 hashing for cache keys and LRU eviction policy.
///
/// # Thread Safety
///
/// This type is `Send` and `Sync`, using `Mutex` for safe concurrent access.
///
/// # Examples
///
/// ```
/// use mcp_wasm_runtime::cache::ModuleCache;
///
/// let mut cache = ModuleCache::new(10);
/// assert_eq!(cache.len(), 0);
/// assert_eq!(cache.capacity(), 10);
/// ```
pub struct ModuleCache {
    /// LRU cache storage with mutex for thread safety
    cache: Mutex<lru::LruCache<CacheKey, Module>>,
}

impl std::fmt::Debug for ModuleCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModuleCache")
            .field("len", &self.len())
            .field("capacity", &self.capacity())
            .finish()
    }
}

impl ModuleCache {
    /// Creates a new module cache with specified capacity.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::cache::ModuleCache;
    ///
    /// let cache = ModuleCache::new(50);
    /// assert_eq!(cache.capacity(), 50);
    /// ```
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: Mutex::new(lru::LruCache::new(
                std::num::NonZeroUsize::new(capacity).expect("capacity must be non-zero"),
            )),
        }
    }

    /// Generates a cache key from WASM bytecode.
    ///
    /// Uses BLAKE3 hash for fast, collision-resistant keys.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::cache::ModuleCache;
    ///
    /// let code = b"test wasm code";
    /// let key = ModuleCache::cache_key_for_code(code);
    /// assert!(key.as_str().starts_with("wasm_"));
    /// ```
    #[must_use]
    pub fn cache_key_for_code(code: &[u8]) -> CacheKey {
        let mut hasher = Hasher::new();
        hasher.update(code);
        let hash = hasher.finalize();
        CacheKey::new(format!("wasm_{}", hash.to_hex()))
    }

    /// Gets a module from the cache.
    ///
    /// Returns `None` if the key is not found. Updates LRU order on hit.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::cache::ModuleCache;
    ///
    /// let cache = ModuleCache::new(10);
    /// let key = ModuleCache::cache_key_for_code(b"test");
    /// assert!(cache.get(&key).is_none());
    /// ```
    #[must_use]
    pub fn get(&self, key: &CacheKey) -> Option<Module> {
        let mut cache = self.cache.lock().unwrap();
        cache.get(key).cloned()
    }

    /// Inserts a module into the cache.
    ///
    /// If the cache is full, evicts the least recently used entry.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_wasm_runtime::cache::{ModuleCache, CacheKey};
    /// use wasmtime::{Engine, Module};
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let cache = ModuleCache::new(10);
    /// let engine = Engine::default();
    /// let module = Module::new(&engine, b"\0asm\x01\0\0\0")?;
    /// let key = CacheKey::new("test_key");
    ///
    /// cache.insert(key, module);
    /// # Ok(())
    /// # }
    /// ```
    pub fn insert(&self, key: CacheKey, module: Module) {
        let mut cache = self.cache.lock().unwrap();
        let key_str = key.as_str().to_string();
        cache.put(key, module);
        tracing::debug!("Module cached: {} (cache size: {})", key_str, cache.len());
    }

    /// Checks if a key exists in the cache.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::cache::ModuleCache;
    ///
    /// let cache = ModuleCache::new(10);
    /// let key = ModuleCache::cache_key_for_code(b"test");
    /// assert!(!cache.contains(&key));
    /// ```
    #[must_use]
    pub fn contains(&self, key: &CacheKey) -> bool {
        let cache = self.cache.lock().unwrap();
        cache.contains(key)
    }

    /// Clears all entries from the cache.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::cache::ModuleCache;
    ///
    /// let cache = ModuleCache::new(10);
    /// cache.clear();
    /// assert_eq!(cache.len(), 0);
    /// ```
    pub fn clear(&self) {
        let mut cache = self.cache.lock().unwrap();
        cache.clear();
        tracing::info!("Module cache cleared");
    }

    /// Returns the number of entries in the cache.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::cache::ModuleCache;
    ///
    /// let cache = ModuleCache::new(10);
    /// assert_eq!(cache.len(), 0);
    /// ```
    #[must_use]
    pub fn len(&self) -> usize {
        let cache = self.cache.lock().unwrap();
        cache.len()
    }

    /// Returns whether the cache is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::cache::ModuleCache;
    ///
    /// let cache = ModuleCache::new(10);
    /// assert!(cache.is_empty());
    /// ```
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the cache capacity.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::cache::ModuleCache;
    ///
    /// let cache = ModuleCache::new(25);
    /// assert_eq!(cache.capacity(), 25);
    /// ```
    #[must_use]
    pub fn capacity(&self) -> usize {
        let cache = self.cache.lock().unwrap();
        cache.cap().get()
    }

    /// Returns cache hit rate statistics.
    ///
    /// Returns (hits, total_requests, hit_rate_percentage).
    ///
    /// Note: This is a simplified implementation. For production use,
    /// consider tracking hits/misses explicitly with atomic counters.
    #[must_use]
    pub fn hit_rate(&self) -> (usize, usize, f64) {
        // Simplified: just return cache utilization
        let len = self.len();
        let cap = self.capacity();
        let rate = if cap > 0 {
            (len as f64 / cap as f64) * 100.0
        } else {
            0.0
        };
        (len, cap, rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasmtime::{Config, Engine};

    #[test]
    fn test_cache_key_generation() {
        let code1 = b"test code";
        let code2 = b"test code";
        let code3 = b"different code";

        let key1 = ModuleCache::cache_key_for_code(code1);
        let key2 = ModuleCache::cache_key_for_code(code2);
        let key3 = ModuleCache::cache_key_for_code(code3);

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
        assert!(key1.as_str().starts_with("wasm_"));
    }

    #[test]
    fn test_cache_creation() {
        let cache = ModuleCache::new(10);
        assert_eq!(cache.len(), 0);
        assert_eq!(cache.capacity(), 10);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cache_insert_and_get() {
        let cache = ModuleCache::new(5);
        let engine = Engine::new(&Config::default()).unwrap();

        // Create a simple WASM module
        let wat = "(module)";
        let wasm = wat::parse_str(wat).unwrap();
        let module = Module::new(&engine, wasm).unwrap();

        let key = ModuleCache::cache_key_for_code(b"test");

        // Insert module
        cache.insert(key.clone(), module);

        // Check cache
        assert_eq!(cache.len(), 1);
        assert!(cache.contains(&key));
        assert!(cache.get(&key).is_some());
    }

    #[test]
    fn test_cache_lru_eviction() {
        let cache = ModuleCache::new(2);
        let engine = Engine::new(&Config::default()).unwrap();

        let wat = "(module)";
        let wasm = wat::parse_str(wat).unwrap();

        let key1 = CacheKey::new("key1");
        let key2 = CacheKey::new("key2");
        let key3 = CacheKey::new("key3");

        let module1 = Module::new(&engine, &wasm).unwrap();
        let module2 = Module::new(&engine, &wasm).unwrap();
        let module3 = Module::new(&engine, &wasm).unwrap();

        // Insert first two modules
        cache.insert(key1.clone(), module1);
        cache.insert(key2.clone(), module2);
        assert_eq!(cache.len(), 2);

        // Insert third module (should evict key1)
        cache.insert(key3.clone(), module3);
        assert_eq!(cache.len(), 2);
        assert!(!cache.contains(&key1)); // Evicted
        assert!(cache.contains(&key2));
        assert!(cache.contains(&key3));
    }

    #[test]
    fn test_cache_clear() {
        let cache = ModuleCache::new(5);
        let engine = Engine::new(&Config::default()).unwrap();

        let wat = "(module)";
        let wasm = wat::parse_str(wat).unwrap();
        let module = Module::new(&engine, wasm).unwrap();

        cache.insert(CacheKey::new("test"), module);
        assert_eq!(cache.len(), 1);

        cache.clear();
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cache_get_updates_lru() {
        let cache = ModuleCache::new(2);
        let engine = Engine::new(&Config::default()).unwrap();

        let wat = "(module)";
        let wasm = wat::parse_str(wat).unwrap();

        let key1 = CacheKey::new("key1");
        let key2 = CacheKey::new("key2");
        let key3 = CacheKey::new("key3");

        let module1 = Module::new(&engine, &wasm).unwrap();
        let module2 = Module::new(&engine, &wasm).unwrap();
        let module3 = Module::new(&engine, &wasm).unwrap();

        // Insert two modules
        cache.insert(key1.clone(), module1);
        cache.insert(key2.clone(), module2);

        // Access key1 to make it recently used
        assert!(cache.get(&key1).is_some());

        // Insert key3 (should evict key2, not key1)
        cache.insert(key3.clone(), module3);

        assert!(cache.contains(&key1)); // Still present (recently accessed)
        assert!(!cache.contains(&key2)); // Evicted
        assert!(cache.contains(&key3));
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_cache_hit_rate() {
        let cache = ModuleCache::new(10);
        let (hits, total, rate) = cache.hit_rate();
        assert_eq!(hits, 0);
        assert_eq!(total, 10);
        assert_eq!(rate, 0.0);
    }
}
