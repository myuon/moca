/// Inline Cache for property access optimization.
///
/// Inline caches store type information observed at runtime to speed up
/// repeated property accesses on objects with the same shape/type.

/// Cache state for inline caches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheState {
    /// No type information recorded yet
    Uninitialized,
    /// Single type observed (fastest path)
    Monomorphic,
    /// 2-4 types observed
    Polymorphic,
    /// More than 4 types observed (cache disabled)
    Megamorphic,
}

/// Inline cache for a single property access site.
#[derive(Debug, Clone)]
pub struct InlineCache {
    /// Cached type ID (shape of the object)
    pub cached_type_id: u32,
    /// Cached field offset within the object
    pub cached_offset: u16,
    /// Current cache state
    pub state: CacheState,
    /// For polymorphic cache: additional type/offset pairs
    pub poly_entries: Vec<(u32, u16)>,
}

impl InlineCache {
    pub fn new() -> Self {
        Self {
            cached_type_id: 0,
            cached_offset: 0,
            state: CacheState::Uninitialized,
            poly_entries: Vec::new(),
        }
    }

    /// Check if this cache hits for the given type ID.
    /// Returns Some(offset) if hit, None if miss.
    #[inline]
    pub fn check(&self, type_id: u32) -> Option<u16> {
        match self.state {
            CacheState::Uninitialized => None,
            CacheState::Monomorphic => {
                if self.cached_type_id == type_id {
                    Some(self.cached_offset)
                } else {
                    None
                }
            }
            CacheState::Polymorphic => {
                if self.cached_type_id == type_id {
                    return Some(self.cached_offset);
                }
                for &(tid, offset) in &self.poly_entries {
                    if tid == type_id {
                        return Some(offset);
                    }
                }
                None
            }
            CacheState::Megamorphic => None, // Cache disabled
        }
    }

    /// Update the cache with a new type/offset observation.
    pub fn update(&mut self, type_id: u32, offset: u16) {
        match self.state {
            CacheState::Uninitialized => {
                self.cached_type_id = type_id;
                self.cached_offset = offset;
                self.state = CacheState::Monomorphic;
            }
            CacheState::Monomorphic => {
                if self.cached_type_id != type_id {
                    // Transition to polymorphic
                    self.poly_entries.push((type_id, offset));
                    self.state = CacheState::Polymorphic;
                }
            }
            CacheState::Polymorphic => {
                // Check if we already have this type
                if self.cached_type_id == type_id {
                    return;
                }
                for &(tid, _) in &self.poly_entries {
                    if tid == type_id {
                        return;
                    }
                }
                // Add new entry
                if self.poly_entries.len() < 3 {
                    self.poly_entries.push((type_id, offset));
                } else {
                    // Too many types, become megamorphic
                    self.state = CacheState::Megamorphic;
                    self.poly_entries.clear();
                }
            }
            CacheState::Megamorphic => {
                // Do nothing, cache is disabled
            }
        }
    }

    /// Reset the cache to uninitialized state.
    pub fn reset(&mut self) {
        self.state = CacheState::Uninitialized;
        self.cached_type_id = 0;
        self.cached_offset = 0;
        self.poly_entries.clear();
    }

    /// Check if the cache is in a usable state.
    #[inline]
    pub fn is_usable(&self) -> bool {
        self.state != CacheState::Megamorphic
    }
}

impl Default for InlineCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Storage for inline caches associated with a function.
#[derive(Debug, Clone, Default)]
pub struct InlineCacheTable {
    /// Map from bytecode PC to inline cache
    caches: Vec<Option<InlineCache>>,
}

impl InlineCacheTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ensure the table can hold a cache at the given PC.
    pub fn ensure_capacity(&mut self, pc: usize) {
        if pc >= self.caches.len() {
            self.caches.resize(pc + 1, None);
        }
    }

    /// Get or create a cache at the given PC.
    pub fn get_or_create(&mut self, pc: usize) -> &mut InlineCache {
        self.ensure_capacity(pc);
        if self.caches[pc].is_none() {
            self.caches[pc] = Some(InlineCache::new());
        }
        self.caches[pc].as_mut().unwrap()
    }

    /// Get a cache at the given PC (if it exists).
    pub fn get(&self, pc: usize) -> Option<&InlineCache> {
        self.caches.get(pc).and_then(|c| c.as_ref())
    }

    /// Get a mutable cache at the given PC (if it exists).
    pub fn get_mut(&mut self, pc: usize) -> Option<&mut InlineCache> {
        self.caches.get_mut(pc).and_then(|c| c.as_mut())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ic_uninitialized() {
        let ic = InlineCache::new();
        assert_eq!(ic.state, CacheState::Uninitialized);
        assert_eq!(ic.check(1), None);
    }

    #[test]
    fn test_ic_monomorphic() {
        let mut ic = InlineCache::new();
        ic.update(42, 8);

        assert_eq!(ic.state, CacheState::Monomorphic);
        assert_eq!(ic.check(42), Some(8));
        assert_eq!(ic.check(99), None);
    }

    #[test]
    fn test_ic_polymorphic() {
        let mut ic = InlineCache::new();
        ic.update(1, 0);
        ic.update(2, 4);
        ic.update(3, 8);

        assert_eq!(ic.state, CacheState::Polymorphic);
        assert_eq!(ic.check(1), Some(0));
        assert_eq!(ic.check(2), Some(4));
        assert_eq!(ic.check(3), Some(8));
    }

    #[test]
    fn test_ic_megamorphic() {
        let mut ic = InlineCache::new();
        ic.update(1, 0);
        ic.update(2, 4);
        ic.update(3, 8);
        ic.update(4, 12);
        ic.update(5, 16); // This should trigger megamorphic

        assert_eq!(ic.state, CacheState::Megamorphic);
        assert_eq!(ic.check(1), None); // Cache is disabled
    }
}
