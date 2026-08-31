use std::collections::BTreeMap;

/// Identity-keyed collection that iterates in first-insertion order.
///
/// Display order must follow arrival, not identifier collation, so the order vector is the
/// authority and the map exists only to update an entry in place by identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct OrderedById<K, V> {
    order: Vec<K>,
    entries: BTreeMap<K, V>,
}

impl<K: Clone + Ord, V> OrderedById<K, V> {
    pub(super) fn contains(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }

    /// Appends a new entry, or replaces an existing one without moving its position.
    pub(super) fn upsert(&mut self, key: K, value: V) {
        if self.entries.insert(key.clone(), value).is_none() {
            self.order.push(key);
        }
    }

    pub(super) fn get(&self, key: &K) -> Option<&V> {
        self.entries.get(key)
    }

    pub(super) fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.entries.get_mut(key)
    }

    pub(super) fn len(&self) -> usize {
        self.order.len()
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = &V> {
        self.order.iter().filter_map(|key| self.entries.get(key))
    }
}

impl<K, V> Default for OrderedById<K, V> {
    fn default() -> Self {
        Self {
            order: Vec::new(),
            entries: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OrderedById;

    #[test]
    fn iteration_follows_arrival_rather_than_key_order() {
        let mut collection = OrderedById::default();
        collection.upsert("z", 1);
        collection.upsert("a", 2);

        assert_eq!(collection.iter().copied().collect::<Vec<_>>(), [1, 2]);
    }

    #[test]
    fn upsert_replaces_in_place_without_moving_or_duplicating() {
        let mut collection = OrderedById::default();
        collection.upsert("z", 1);
        collection.upsert("a", 2);
        collection.upsert("z", 3);

        assert_eq!(collection.iter().copied().collect::<Vec<_>>(), [3, 2]);
        assert_eq!(collection.len(), 2);
    }
}
