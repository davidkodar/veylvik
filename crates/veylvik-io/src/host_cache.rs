//! Process-wide LRU of decoded host frames shared by every video and sequence source:
//! retained bytes never exceed one limit, so a project may hold any number of clips.
use std::any::Any;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Hard cap on entries, so tiny frames cannot grow the lookup without bound.
const MAX_ENTRIES: usize = 4096;
type Value = Arc<dyn Any + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Key {
    pub owner: u64,
    pub kind: u8,
    pub index: usize,
    pub variant: u64,
}
struct Lru {
    limit: u64,
    used: u64,
    /// Oldest first.
    entries: VecDeque<(Key, Value, u64)>,
}
impl Lru {
    fn new(limit: u64) -> Self {
        Self {
            limit,
            used: 0,
            entries: VecDeque::new(),
        }
    }
    fn get(&mut self, key: Key) -> Option<Value> {
        let at = self.entries.iter().position(|(k, ..)| *k == key)?;
        let entry = self.entries.remove(at)?;
        let value = entry.1.clone();
        self.entries.push_back(entry);
        Some(value)
    }
    fn remove_where(&mut self, keep: impl Fn(&Key) -> bool) {
        let used = &mut self.used;
        self.entries.retain(|(k, _, bytes)| {
            keep(k) || {
                *used -= bytes;
                false
            }
        });
    }
    fn fit(&mut self, extra: u64, slots: usize) {
        while self.used + extra > self.limit || self.entries.len() + slots > MAX_ENTRIES {
            let Some((_, _, bytes)) = self.entries.pop_front() else {
                break;
            };
            self.used -= bytes;
        }
    }
    fn put(&mut self, key: Key, value: Value, bytes: u64) {
        self.remove_where(|k| *k != key);
        // A frame larger than the whole cache is returned to its caller but not retained.
        if bytes > self.limit {
            return;
        }
        self.fit(bytes, 1);
        self.used += bytes;
        self.entries.push_back((key, value, bytes));
    }
    fn set_limit(&mut self, limit: u64) {
        self.limit = limit;
        self.fit(0, 0);
    }
}

static CACHE: Mutex<Option<Lru>> = Mutex::new(None);
static OWNERS: AtomicU64 = AtomicU64::new(1);

fn with<R>(f: impl FnOnce(&mut Lru) -> R) -> R {
    // A panic elsewhere cannot leave the LRU half-updated: every method restores `used`.
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(|| Lru::new(crate::runtime::source_budget())))
}
/// A unique id for one source's entries.
pub(crate) fn owner() -> u64 {
    OWNERS.fetch_add(1, Ordering::Relaxed)
}
pub(crate) fn get<T: Any + Send + Sync>(key: Key) -> Option<Arc<T>> {
    with(|lru| lru.get(key)).and_then(|value| value.downcast().ok())
}
pub(crate) fn put<T: Any + Send + Sync>(key: Key, value: Arc<T>, bytes: u64) {
    with(|lru| lru.put(key, value, bytes));
}
pub(crate) fn remove(key: Key) {
    with(|lru| lru.remove_where(|k| *k != key));
}
/// Drop one owner's entries, all of them or only one kind.
pub(crate) fn forget(owner: u64, kind: Option<u8>) {
    with(|lru| lru.remove_where(|k| k.owner != owner || kind.is_some_and(|kind| k.kind != kind)));
}
/// Bytes retained across all sources.
pub fn retained_bytes() -> u64 {
    with(|lru| lru.used)
}
/// The shared limit; starts at `runtime::source_budget()`.
pub fn limit() -> u64 {
    with(|lru| lru.limit)
}
/// Change the shared limit, evicting the oldest frames to fit at once.
pub fn set_limit(bytes: u64) {
    with(|lru| lru.set_limit(bytes));
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(owner: u64, index: usize) -> Key {
        Key {
            owner,
            kind: 0,
            index,
            variant: 0,
        }
    }
    #[test]
    fn evicts_least_recent_across_owners_and_never_exceeds_limit() {
        let mut lru = Lru::new(300);
        for (owner, index) in [(1, 0), (2, 0), (3, 0)] {
            lru.put(key(owner, index), Arc::new(index), 100);
        }
        assert!(
            lru.get(key(1, 0)).is_some(),
            "touch owner 1 so owner 2 is oldest"
        );
        lru.put(key(4, 0), Arc::new(0usize), 100);
        assert!(lru.get(key(2, 0)).is_none());
        assert!(lru.get(key(1, 0)).is_some() && lru.get(key(4, 0)).is_some());
        assert_eq!(lru.used, 300);
        lru.put(key(5, 0), Arc::new(0usize), 301);
        assert!(
            lru.get(key(5, 0)).is_none(),
            "oversize frames are not retained"
        );
        assert_eq!(lru.used, 300);
        lru.put(key(1, 0), Arc::new(7usize), 50);
        assert_eq!(lru.used, 250, "replacing a key releases its old bytes");
        lru.remove_where(|k| k.owner != 1);
        assert_eq!(lru.used, 200);
        lru.set_limit(100);
        assert!(lru.used <= 100 && lru.entries.len() == 1);
    }
    #[test]
    fn global_values_round_trip_by_type() {
        let owner = owner();
        put(key(owner, 3), Arc::new(42u32), 4);
        assert_eq!(get::<u32>(key(owner, 3)).as_deref(), Some(&42));
        assert!(get::<u64>(key(owner, 3)).is_none());
        forget(owner, None);
        assert!(get::<u32>(key(owner, 3)).is_none());
    }
}
