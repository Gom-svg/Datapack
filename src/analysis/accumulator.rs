use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use super::model::{CardinalityEstimate, ColumnFacts, ColumnNameStatus, CARDINALITY_LOWER_BOUND};

const UNIQUE_TRACKING_LIMIT: usize = 8_192;

/// Fixed passive scalar state added per column during accumulation.
///
/// This excludes the pre-existing cardinality map and name allocation, result
/// DTOs, allocator overhead, and the transient duplicate-name index. It is not
/// a claim about process RSS.
#[derive(Debug, Clone)]
struct PassiveColumnMetrics {
    empty_values: u64,
    min_value_len_bytes: u64,
    max_value_len_bytes: u64,
}

impl PassiveColumnMetrics {
    fn new() -> Self {
        Self {
            empty_values: 0,
            min_value_len_bytes: u64::MAX,
            max_value_len_bytes: 0,
        }
    }

    fn observe(&mut self, value_len: u64, is_empty: bool) {
        if is_empty {
            self.empty_values = self.empty_values.saturating_add(1);
        }
        self.min_value_len_bytes = self.min_value_len_bytes.min(value_len);
        self.max_value_len_bytes = self.max_value_len_bytes.max(value_len);
    }
}

#[derive(Debug, Clone)]
pub(super) struct AnalysisAccumulator {
    columns: Vec<ColumnAccumulator>,
    cardinality_budget: CardinalityBudget,
    cardinality_memory_limited: bool,
}

impl AnalysisAccumulator {
    pub(super) fn new(headers: &[String], max_cardinality_entries: usize) -> Self {
        let mut first_indices = HashMap::with_capacity(headers.len());
        let columns = headers
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let duplicate_of = first_indices.get(name.as_str()).copied();
                if duplicate_of.is_none() {
                    first_indices.insert(name.as_str(), index);
                }
                let name_status = ColumnNameStatus {
                    is_empty: name.is_empty(),
                    duplicate_of,
                };
                ColumnAccumulator::new(index, name, name_status)
            })
            .collect();
        Self {
            columns,
            cardinality_budget: CardinalityBudget::new(max_cardinality_entries),
            cardinality_memory_limited: false,
        }
    }

    pub(super) fn observe(&mut self, fields: &[&str]) {
        debug_assert_eq!(self.columns.len(), fields.len());
        for (column, value) in self.columns.iter_mut().zip(fields.iter().copied()) {
            self.cardinality_memory_limited |= column.observe(value, &mut self.cardinality_budget);
        }
    }

    pub(super) fn finish(self) -> (Vec<ColumnFacts>, bool) {
        let columns = self
            .columns
            .into_iter()
            .map(ColumnAccumulator::finish)
            .collect();
        (columns, self.cardinality_memory_limited)
    }
}

#[derive(Debug, Clone)]
struct CardinalityBudget {
    available: usize,
    capacity: usize,
}

impl CardinalityBudget {
    fn new(capacity: usize) -> Self {
        Self {
            available: capacity,
            capacity,
        }
    }

    fn reserve(&mut self) -> bool {
        if self.available == 0 {
            return false;
        }
        self.available -= 1;
        true
    }

    fn release(&mut self, entries: usize) {
        self.available = self.available.saturating_add(entries).min(self.capacity);
    }
}

#[derive(Debug, Clone)]
struct ColumnAccumulator {
    index: usize,
    name: String,
    name_status: ColumnNameStatus,
    unique_hashes: HashMap<u64, ()>,
    mean_len: f64,
    observed_values: u64,
    passive: PassiveColumnMetrics,
    total_value_bytes: u64,
    numeric_values: u64,
    cardinality_lower_bound: Option<u64>,
}

impl ColumnAccumulator {
    fn new(index: usize, name: &str, name_status: ColumnNameStatus) -> Self {
        Self {
            index,
            name: name.to_owned(),
            name_status,
            unique_hashes: HashMap::new(),
            mean_len: 0.0,
            observed_values: 0,
            passive: PassiveColumnMetrics::new(),
            total_value_bytes: 0,
            numeric_values: 0,
            cardinality_lower_bound: None,
        }
    }

    fn observe(&mut self, value: &str, budget: &mut CardinalityBudget) -> bool {
        // Keep the legacy mean update and numeric counters in precisely the same
        // order and numeric types used by PlannerPolicyV1 before extraction.
        self.observed_values += 1;
        let value_len = value.len() as u64;
        let len = value_len as f64;
        self.mean_len += (len - self.mean_len) / self.observed_values as f64;
        self.total_value_bytes = self.total_value_bytes.saturating_add(value_len);
        if value.parse::<i64>().is_ok() || value.parse::<f64>().is_ok() {
            self.numeric_values += 1;
        }

        // Empty and length metrics intentionally describe the physical field
        // representation emitted by the legacy parser. In particular, `""` is
        // two bytes rather than a logical empty value.
        self.passive.observe(value_len, value.is_empty());

        if self.cardinality_lower_bound.is_some() {
            return false;
        }
        let hash = stable_hash(&value);
        if self.unique_hashes.contains_key(&hash) {
            return false;
        }
        let retained = self.unique_hashes.len();
        if retained >= UNIQUE_TRACKING_LIMIT {
            self.cardinality_lower_bound = Some(CARDINALITY_LOWER_BOUND);
            self.unique_hashes = HashMap::new();
            budget.release(retained);
            return false;
        }
        if !budget.reserve() {
            let retained_lower_bound = u64::try_from(retained)
                .unwrap_or(u64::MAX)
                .saturating_add(1);
            self.cardinality_lower_bound = Some(retained_lower_bound);
            self.unique_hashes = HashMap::new();
            budget.release(retained);
            return true;
        }
        self.unique_hashes.insert(hash, ());
        false
    }

    fn finish(self) -> ColumnFacts {
        let cardinality = match self.cardinality_lower_bound {
            Some(lower_bound) => CardinalityEstimate::AtLeast(lower_bound),
            None => CardinalityEstimate::Exact(
                u64::try_from(self.unique_hashes.len()).unwrap_or(u64::MAX),
            ),
        };
        ColumnFacts {
            index: self.index,
            name: self.name,
            name_status: self.name_status,
            observed_values: self.observed_values,
            empty_values: self.passive.empty_values,
            min_value_len_bytes: (self.observed_values > 0)
                .then_some(self.passive.min_value_len_bytes),
            max_value_len_bytes: (self.observed_values > 0)
                .then_some(self.passive.max_value_len_bytes),
            mean_value_len_bytes: self.mean_len,
            total_value_bytes: self.total_value_bytes,
            numeric_values: self.numeric_values,
            cardinality,
        }
    }
}

fn stable_hash<T: Hash>(value: &T) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passive_empty_counter_saturates() {
        let mut column = ColumnAccumulator::new(0, "value", ColumnNameStatus::named());
        let mut budget = CardinalityBudget::new(1);
        column.passive.empty_values = u64::MAX;

        column.observe("", &mut budget);

        assert_eq!(column.passive.empty_values, u64::MAX);
    }

    #[test]
    fn passive_scalar_overhead_is_three_u64_values_per_column() {
        assert_eq!(
            std::mem::size_of::<PassiveColumnMetrics>(),
            3 * std::mem::size_of::<u64>()
        );
    }

    #[test]
    fn censored_tracker_releases_entries_and_allocated_capacity() {
        let mut column = ColumnAccumulator::new(0, "value", ColumnNameStatus::named());
        let mut budget = CardinalityBudget::new(2);

        assert!(!column.observe("a", &mut budget));
        assert!(!column.observe("b", &mut budget));
        assert!(column.observe("c", &mut budget));

        assert_eq!(column.unique_hashes.len(), 0);
        assert_eq!(column.unique_hashes.capacity(), 0);
        assert_eq!(budget.available, 2);

        let mut next_column = ColumnAccumulator::new(1, "next", ColumnNameStatus::named());
        assert!(!next_column.observe("d", &mut budget));
        assert_eq!(budget.available, 1);
    }
}
