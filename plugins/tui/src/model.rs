//! In-memory state shared between the pipeline (which writes measurements) and the interactive
//! TUI thread (which renders it).
//!
//! The model keeps the *latest* value of every series it has seen, so the displayed table stays
//! consistent regardless of which source flushed or how often. Series that stop being updated are
//! evicted after a configurable delay (see [`Model::evict`]), which keeps memory bounded and makes
//! short-lived series — e.g. the many per-process series produced by the procfs plugin — disappear
//! once the process is gone.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use regex::{Regex, RegexBuilder};

/// A single recorded sample for a graphed series.
pub type Sample = (Instant, f64);

/// Number of recent values kept per series for the inline trend sparkline.
pub const SPARK_CAP: usize = 16;

/// A small fixed-capacity ring of a series' most recent values, feeding the inline trend sparkline.
/// Fixed-size (no heap) so keeping one per series stays cheap even with thousands of short-lived
/// series — unlike [`Model::history`], which is per-watched-series and time-windowed.
#[derive(Debug, Clone)]
struct Spark {
    buf: [f32; SPARK_CAP],
    /// Number of valid entries (≤ [`SPARK_CAP`]).
    len: usize,
    /// Index of the oldest entry.
    head: usize,
}

impl Default for Spark {
    fn default() -> Self {
        Self {
            buf: [0.0; SPARK_CAP],
            len: 0,
            head: 0,
        }
    }
}

impl Spark {
    /// Appends `v`, dropping the oldest value once full.
    fn push(&mut self, v: f32) {
        if self.len < SPARK_CAP {
            self.buf[(self.head + self.len) % SPARK_CAP] = v;
            self.len += 1;
        } else {
            self.buf[self.head] = v;
            self.head = (self.head + 1) % SPARK_CAP;
        }
    }

    /// The retained values, oldest first, as a fixed array plus its valid length. Returns by value
    /// (the array is `Copy`) so snapshotting a row allocates nothing.
    fn snapshot(&self) -> ([f32; SPARK_CAP], usize) {
        let mut out = [0.0; SPARK_CAP];
        for (i, slot) in out.iter_mut().enumerate().take(self.len) {
            *slot = self.buf[(self.head + i) % SPARK_CAP];
        }
        (out, self.len)
    }
}

/// A stored series: its latest value plus the recent-values ring behind the sparkline. Keeping both
/// in one map means `upsert` updates them together in place (no key clone, no second lookup) and
/// eviction drops them together.
#[derive(Debug, Clone)]
struct Entry {
    value: SeriesValue,
    spark: Spark,
}

/// Identity of a measurement series: everything but the value and timestamp.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SeriesKey {
    pub metric: String,
    pub unit: String,
    pub resource: String,
    pub consumer: String,
    pub attributes: String,
}

/// The varying part of a series: its latest value and when it was seen.
#[derive(Debug, Clone)]
pub struct SeriesValue {
    /// Displayable value (already formatted).
    pub value: String,
    /// Numeric value, kept for numeric sorting.
    pub value_num: f64,
    /// Measurement timestamp, formatted as `HH:MM:SS`.
    pub updated: String,
    /// Wall-clock instant at which this series was last updated, used for eviction.
    pub last_seen: Instant,
}

/// A fully resolved table row (a [`SeriesKey`] joined with its [`SeriesValue`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub metric: String,
    pub resource: String,
    pub consumer: String,
    pub value: String,
    pub value_num: f64,
    pub unit: String,
    pub updated: String,
    pub attributes: String,
    /// Recent values (oldest first) for the inline trend sparkline; `spark_len` are valid.
    pub spark: [f32; SPARK_CAP],
    pub spark_len: usize,
}

impl Row {
    /// The identity of the series this row belongs to.
    pub fn key(&self) -> SeriesKey {
        SeriesKey {
            metric: self.metric.clone(),
            unit: self.unit.clone(),
            resource: self.resource.clone(),
            consumer: self.consumer.clone(),
            attributes: self.attributes.clone(),
        }
    }
}

/// The column a sort is performed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Metric,
    Resource,
    Consumer,
    Value,
    Unit,
    Updated,
    Attributes,
}

impl SortColumn {
    /// Sortable columns in the order they appear in the table, left to right (the `trend` sparkline
    /// is skipped — it has no meaningful order). This is the order the sort cursor moves through.
    pub const ALL: [SortColumn; 7] = [
        SortColumn::Metric,
        SortColumn::Resource,
        SortColumn::Consumer,
        SortColumn::Value,
        SortColumn::Unit,
        SortColumn::Updated,
        SortColumn::Attributes,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SortColumn::Metric => "metric",
            SortColumn::Resource => "resource",
            SortColumn::Consumer => "consumer",
            SortColumn::Value => "value",
            SortColumn::Unit => "unit",
            SortColumn::Updated => "updated",
            SortColumn::Attributes => "attributes",
        }
    }

    /// The natural initial direction when first sorting by this column: descending for the numeric
    /// value (largest first), ascending for the text columns.
    pub fn default_desc(self) -> bool {
        matches!(self, SortColumn::Value)
    }
}

/// A column rows can be grouped by. Grouping nests these in a user-chosen order (see
/// [`build_grouped_view`]): e.g. `[Metric, Resource]` yields a metric group, each holding its
/// resource subgroups, each holding the matching series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupColumn {
    Metric,
    Resource,
    Consumer,
}

impl GroupColumn {
    pub fn label(self) -> &'static str {
        match self {
            GroupColumn::Metric => "metric",
            GroupColumn::Resource => "resource",
            GroupColumn::Consumer => "consumer",
        }
    }

    /// The value of `row` in this grouping dimension.
    pub fn value(self, row: &Row) -> &str {
        match self {
            GroupColumn::Metric => &row.metric,
            GroupColumn::Resource => &row.resource,
            GroupColumn::Consumer => &row.consumer,
        }
    }
}

/// One step of the path from the tree root to a group node: a dimension and the value rows share
/// at that level (e.g. `(Resource, "cpu_package(0)")`).
pub type GroupStep = (GroupColumn, String);

/// Identity of an item shown in the table: a leaf series, or a group node addressed by its full
/// path from the root (so the same value at different nesting levels stays distinct).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ItemId {
    Series(SeriesKey),
    Group(String),
}

/// A collapsible node in the grouping tree.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupNode {
    /// Stable id derived from [`path`](Self::path); also the fold-state key.
    pub key: String,
    /// This level's grouping value (the shared metric/resource/consumer).
    pub label: String,
    /// This level's grouping dimension.
    pub col: GroupColumn,
    /// Nesting depth, 0 at the root level.
    pub depth: usize,
    /// Number of leaf series anywhere under this node.
    pub count: usize,
    pub collapsed: bool,
    /// Full path from the root, used to recover the node's members when (un)marking it.
    pub path: Vec<GroupStep>,
}

/// An item shown in the table: a group node, or a leaf row at the given nesting depth.
#[derive(Debug, Clone, PartialEq)]
pub enum ViewItem {
    Group(GroupNode),
    Row { row: Row, depth: usize },
}

impl ViewItem {
    pub fn id(&self) -> ItemId {
        match self {
            ViewItem::Group(g) => ItemId::Group(g.key.clone()),
            ViewItem::Row { row, .. } => ItemId::Series(row.key()),
        }
    }
}

/// Whether `row` belongs to the group reached by following `path`.
pub fn row_in_path(row: &Row, path: &[GroupStep]) -> bool {
    path.iter().all(|(col, label)| col.value(row) == label)
}

/// The fold keys of every group node that must be expanded for `row` to be visible under
/// `group_by` — i.e. the keys of all of `row`'s ancestors in the tree.
pub fn ancestor_keys(row: &Row, group_by: &[GroupColumn]) -> Vec<String> {
    let mut path = Vec::new();
    let mut keys = Vec::with_capacity(group_by.len());
    for &col in group_by {
        path.push((col, col.value(row).to_owned()));
        keys.push(path_key(&path));
    }
    keys
}

/// The stable id / fold key for a group node, built from its path so equal values at different
/// depths (or in different branches) never collide.
fn path_key(path: &[GroupStep]) -> String {
    let mut key = String::new();
    for (col, label) in path {
        if !key.is_empty() {
            key.push('\u{1}');
        }
        key.push_str(col.label());
        key.push('=');
        key.push_str(label);
    }
    key
}

/// The column a text filter is matched against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterColumn {
    All,
    Metric,
    Resource,
    Consumer,
    Attributes,
}

impl FilterColumn {
    /// The fields of a row that this filter column looks at.
    fn fields(self, row: &Row) -> Vec<&str> {
        match self {
            FilterColumn::All => vec![&row.metric, &row.resource, &row.consumer, &row.unit, &row.attributes],
            FilterColumn::Metric => vec![&row.metric],
            FilterColumn::Resource => vec![&row.resource],
            FilterColumn::Consumer => vec![&row.consumer],
            FilterColumn::Attributes => vec![&row.attributes],
        }
    }

    /// The column named by a `$column=` filter scope prefix (`metric`, `resource`, `consumer`,
    /// `attributes`, `all`), if recognized.
    pub fn from_scope_name(name: &str) -> Option<FilterColumn> {
        match name {
            "metric" => Some(FilterColumn::Metric),
            "resource" => Some(FilterColumn::Resource),
            "consumer" => Some(FilterColumn::Consumer),
            "attributes" => Some(FilterColumn::Attributes),
            "all" => Some(FilterColumn::All),
            _ => None,
        }
    }

    /// The fields of a series key that this filter column looks at (mirrors [`fields`](Self::fields)).
    fn key_fields(self, key: &SeriesKey) -> Vec<&str> {
        match self {
            FilterColumn::All => vec![&key.metric, &key.resource, &key.consumer, &key.unit, &key.attributes],
            FilterColumn::Metric => vec![&key.metric],
            FilterColumn::Resource => vec![&key.resource],
            FilterColumn::Consumer => vec![&key.consumer],
            FilterColumn::Attributes => vec![&key.attributes],
        }
    }
}

/// Resolves a filter input into the column to match against and the remaining regex pattern, or
/// `None` when the filter is not active yet.
///
/// A `$column=` prefix scopes the regex to one column, overriding `default`: `$metric=cpu.*` runs
/// `cpu.*` against the metric column only. Recognized columns are `metric`, `resource`, `consumer`,
/// `attributes` and `all`. Attribute matching is an unanchored regex over the joined attributes, so
/// `$attributes=core=2` matches a series carrying that attribute whatever its position.
///
/// Returns `None` while a `$column=` scope is still being composed — i.e. the input starts with `$`
/// but has no `=` yet, or names an unknown column — so a half-typed scope like `$resou` does not
/// filter as if it were a pattern. Input without a `$` prefix always matches as a pattern on
/// `default` (including the empty string, which compiles to an inactive filter).
pub fn parse_filter_scope(input: &str, default: FilterColumn) -> Option<(FilterColumn, &str)> {
    let Some(rest) = input.strip_prefix('$') else {
        return Some((default, input));
    };
    // A `$…` input is a column scope: active only once `=` and a recognized column are present.
    let (name, pattern) = rest.split_once('=')?;
    let col = FilterColumn::from_scope_name(name)?;
    Some((col, pattern))
}

/// Compiles a filter pattern into a case-insensitive regex. Returns `None` for an empty pattern *or*
/// one that fails to compile — in both cases the filter is treated as inactive (matches everything),
/// so a half-typed pattern (e.g. `(`) never makes the whole view vanish. Use [`valid_filter`] to
/// tell the two apart for display.
pub fn compile_filter(pattern: &str) -> Option<Regex> {
    if pattern.is_empty() {
        return None;
    }
    RegexBuilder::new(pattern).case_insensitive(true).build().ok()
}

/// Whether a filter pattern is usable: empty (inactive) or a valid regex. A non-empty pattern that
/// fails to compile returns `false`, letting the UI flag it.
pub fn valid_filter(pattern: &str) -> bool {
    pattern.is_empty() || RegexBuilder::new(pattern).case_insensitive(true).build().is_ok()
}

/// Holds the latest known state of every series.
#[derive(Debug)]
pub struct Model {
    series: HashMap<SeriesKey, Entry>,
    /// Series not updated within this delay are dropped. `None` disables eviction.
    stale_after: Option<Duration>,
    /// Series for which we record history (those with an open graph tab).
    watched: HashSet<SeriesKey>,
    /// Recent samples per watched series, kept within `history_window`.
    history: HashMap<SeriesKey, VecDeque<Sample>>,
    /// How far back history is retained.
    history_window: Duration,
}

impl Model {
    pub fn new(stale_after: Option<Duration>, history_window: Duration) -> Self {
        Self {
            series: HashMap::new(),
            stale_after,
            watched: HashSet::new(),
            history: HashMap::new(),
            history_window,
        }
    }

    /// Inserts or updates a series, recording a history sample if the series is watched, and always
    /// appending to its trend sparkline.
    pub fn upsert(&mut self, key: SeriesKey, value: SeriesValue) {
        let v = value.value_num as f32;
        // Record history before moving `value`, only when the series is watched (a graph is open).
        if self.watched.contains(&key) {
            let samples = self.history.entry(key.clone()).or_default();
            samples.push_back((value.last_seen, value.value_num));
            if let Some(cutoff) = value.last_seen.checked_sub(self.history_window) {
                while let Some(&(t, _)) = samples.front() {
                    if t < cutoff {
                        samples.pop_front();
                    } else {
                        break;
                    }
                }
            }
        }
        // Update the entry in place when it exists — the common case — so the pipeline's hot path
        // pays neither a key clone nor a second map lookup; only a brand-new series allocates.
        match self.series.get_mut(&key) {
            Some(entry) => {
                entry.spark.push(v);
                entry.value = value;
            }
            None => {
                let mut spark = Spark::default();
                spark.push(v);
                self.series.insert(key, Entry { value, spark });
            }
        }
    }

    /// Starts recording history for a series (called when a graph tab opens).
    pub fn watch(&mut self, key: SeriesKey) {
        self.watched.insert(key);
    }

    /// Stops recording history for a series and discards its samples (graph tab closed).
    pub fn unwatch(&mut self, key: &SeriesKey) {
        self.watched.remove(key);
        self.history.remove(key);
    }

    /// Returns the recorded samples for a series, if any.
    pub fn history(&self, key: &SeriesKey) -> Option<&VecDeque<Sample>> {
        self.history.get(key)
    }

    /// How far back history is retained — the fixed time span graphs plot over.
    pub fn history_window(&self) -> Duration {
        self.history_window
    }

    /// Changes the retained history span on the fly. Shrinking immediately drops samples that now
    /// fall outside the window; growing simply lets future samples accumulate further back (already
    /// discarded samples cannot be recovered).
    pub fn set_history_window(&mut self, window: Duration) {
        self.history_window = window;
        if let Some(cutoff) = Instant::now().checked_sub(window) {
            for samples in self.history.values_mut() {
                while let Some(&(t, _)) = samples.front() {
                    if t < cutoff {
                        samples.pop_front();
                    } else {
                        break;
                    }
                }
            }
        }
    }

    /// Drops series that have not been updated within `stale_after`.
    pub fn evict(&mut self, now: Instant) {
        if let Some(max_age) = self.stale_after {
            self.series
                .retain(|_, e| now.duration_since(e.value.last_seen) <= max_age);
        }
    }

    pub fn len(&self) -> usize {
        self.series.len()
    }

    /// Materializes the current state as a list of rows (unsorted, unfiltered).
    pub fn rows(&self) -> Vec<Row> {
        self.series
            .iter()
            .map(|(k, e)| {
                let (spark, spark_len) = e.spark.snapshot();
                Row {
                    metric: k.metric.clone(),
                    resource: k.resource.clone(),
                    consumer: k.consumer.clone(),
                    value: e.value.value.clone(),
                    value_num: e.value.value_num,
                    unit: k.unit.clone(),
                    updated: e.value.updated.clone(),
                    attributes: k.attributes.clone(),
                    spark,
                    spark_len,
                }
            })
            .collect()
    }
}

/// Whether a series key matches the compiled filter `re` in the given column. A `None` regex (empty
/// or invalid pattern, see [`compile_filter`]) matches everything. Mirrors [`apply_filter`] for
/// places that hold a [`SeriesKey`] rather than a [`Row`] (e.g. the graph raw-data table).
pub fn key_matches(key: &SeriesKey, re: Option<&Regex>, column: FilterColumn) -> bool {
    match re {
        None => true,
        Some(re) => column.key_fields(key).iter().any(|field| re.is_match(field)),
    }
}

/// Keeps only the rows matching the compiled filter `re` in the given column. A `None` regex (empty
/// or invalid pattern, see [`compile_filter`]) keeps everything.
pub fn apply_filter(rows: &mut Vec<Row>, re: Option<&Regex>, column: FilterColumn) {
    let Some(re) = re else { return };
    rows.retain(|row| column.fields(row).iter().any(|field| re.is_match(field)));
}

/// Sorts rows in place by the given column and direction.
///
/// Sorts `rows` by an ordered list of `(column, descending)` keys: the first key is primary, each
/// later one breaks ties of the previous. `Value` is compared numerically, the other columns as
/// text. Whatever the keys, a final tiebreak by full series identity (metric, resource, consumer,
/// attributes — ascending) gives a fully determined order, so the table never jitters: rows only
/// move when a value they are actually sorted on changes.
pub fn apply_sort(rows: &mut [Row], keys: &[(SortColumn, bool)]) {
    rows.sort_by(|a, b| {
        keys.iter()
            .fold(std::cmp::Ordering::Equal, |acc, &(column, descending)| {
                acc.then_with(|| {
                    let ordering = compare_column(a, b, column);
                    if descending { ordering.reverse() } else { ordering }
                })
            })
            .then_with(|| {
                (&a.metric, &a.resource, &a.consumer, &a.attributes).cmp(&(
                    &b.metric,
                    &b.resource,
                    &b.consumer,
                    &b.attributes,
                ))
            })
    });
}

/// Compares two rows on a single column (ascending). `Value` is numeric; the rest are text.
fn compare_column(a: &Row, b: &Row, column: SortColumn) -> std::cmp::Ordering {
    match column {
        SortColumn::Value => a
            .value_num
            .partial_cmp(&b.value_num)
            .unwrap_or(std::cmp::Ordering::Equal),
        SortColumn::Metric => a.metric.cmp(&b.metric),
        SortColumn::Resource => a.resource.cmp(&b.resource),
        SortColumn::Consumer => a.consumer.cmp(&b.consumer),
        SortColumn::Unit => a.unit.cmp(&b.unit),
        SortColumn::Updated => a.updated.cmp(&b.updated),
        SortColumn::Attributes => a.attributes.cmp(&b.attributes),
    }
}

/// Arranges already-filtered, already-sorted `rows` into a collapsible tree, nesting one level per
/// entry of `group_by` (e.g. `[Metric, Resource]` → metric groups, each split by resource).
///
/// The result is the tree flattened in display (pre-order) order: each group node is immediately
/// followed by its descendants, unless it is collapsed, in which case its whole subtree is omitted.
/// Both groups and rows keep their incoming (sorted) order, so an active value-sort naturally
/// surfaces the busiest groups first. An empty `group_by` yields the flat list of rows.
pub fn build_grouped_view(rows: Vec<Row>, group_by: &[GroupColumn], collapsed: &HashSet<String>) -> Vec<ViewItem> {
    let mut items = Vec::new();
    build_level(rows, group_by, 0, &[], collapsed, &mut items);
    items
}

/// Recursively expands one nesting level (`group_by[level]`) of the grouping tree into `out`.
fn build_level(
    rows: Vec<Row>,
    group_by: &[GroupColumn],
    level: usize,
    parent_path: &[GroupStep],
    collapsed: &HashSet<String>,
    out: &mut Vec<ViewItem>,
) {
    // Past the last grouping dimension: emit the leaf rows at this depth.
    let Some(&col) = group_by.get(level) else {
        out.extend(rows.into_iter().map(|row| ViewItem::Row { row, depth: level }));
        return;
    };

    // Bucket rows by this level's value, preserving first-seen (sorted) order of groups and rows.
    let mut order: Vec<String> = Vec::new();
    let mut buckets: HashMap<String, Vec<Row>> = HashMap::new();
    for row in rows {
        let label = col.value(&row).to_owned();
        if !buckets.contains_key(&label) {
            order.push(label.clone());
        }
        buckets.entry(label).or_default().push(row);
    }

    for label in order {
        let members = buckets.remove(&label).expect("bucket exists for every label");
        let mut path = parent_path.to_vec();
        path.push((col, label.clone()));
        let key = path_key(&path);
        let collapsed_here = collapsed.contains(&key);
        out.push(ViewItem::Group(GroupNode {
            label,
            col,
            depth: level,
            count: members.len(),
            collapsed: collapsed_here,
            key,
            path: path.clone(),
        }));
        // A collapsed node hides its whole subtree; an expanded one recurses into the next level.
        if !collapsed_here {
            build_level(members, group_by, level + 1, &path, collapsed, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(metric: &str, resource: &str, consumer: &str, value_num: f64) -> Row {
        Row {
            metric: metric.to_owned(),
            resource: resource.to_owned(),
            consumer: consumer.to_owned(),
            value: value_num.to_string(),
            value_num,
            unit: String::new(),
            updated: String::new(),
            attributes: String::new(),
            spark: [0.0; SPARK_CAP],
            spark_len: 0,
        }
    }

    fn val(value_num: f64, last_seen: Instant) -> SeriesValue {
        SeriesValue {
            value: value_num.to_string(),
            value_num,
            updated: String::new(),
            last_seen,
        }
    }

    fn key(metric: &str, consumer: &str) -> SeriesKey {
        SeriesKey {
            metric: metric.to_owned(),
            unit: String::new(),
            resource: String::new(),
            consumer: consumer.to_owned(),
            attributes: String::new(),
        }
    }

    #[test]
    fn upsert_replaces_same_series() {
        let mut m = Model::new(None, Duration::from_secs(60));
        let now = Instant::now();
        m.upsert(key("cpu", "p1"), val(1.0, now));
        m.upsert(key("cpu", "p1"), val(2.0, now));
        assert_eq!(m.len(), 1);
        assert_eq!(m.rows()[0].value_num, 2.0);
    }

    #[test]
    fn evict_drops_stale_series() {
        let mut m = Model::new(Some(Duration::from_secs(10)), Duration::from_secs(60));
        let now = Instant::now();
        let old = now - Duration::from_secs(30);
        m.upsert(key("cpu", "alive"), val(1.0, now));
        m.upsert(key("cpu", "dead"), val(1.0, old));
        m.evict(now);
        let rows = m.rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].consumer, "alive");
    }

    #[test]
    fn history_recorded_only_for_watched_series() {
        let mut m = Model::new(None, Duration::from_secs(60));
        let now = Instant::now();
        let k = key("cpu", "p1");
        // Not watched yet: no history.
        m.upsert(k.clone(), val(1.0, now));
        assert!(m.history(&k).is_none());
        // Watched: samples accumulate.
        m.watch(k.clone());
        m.upsert(k.clone(), val(2.0, now));
        m.upsert(k.clone(), val(3.0, now));
        let hist: Vec<f64> = m.history(&k).unwrap().iter().map(|(_, v)| *v).collect();
        assert_eq!(hist, vec![2.0, 3.0]);
        // Unwatched: history discarded.
        m.unwatch(&k);
        assert!(m.history(&k).is_none());
    }

    #[test]
    fn history_drops_samples_outside_window() {
        let mut m = Model::new(None, Duration::from_secs(10));
        let now = Instant::now();
        let k = key("cpu", "p1");
        m.watch(k.clone());
        m.upsert(k.clone(), val(1.0, now - Duration::from_secs(30)));
        m.upsert(k.clone(), val(2.0, now));
        let hist: Vec<f64> = m.history(&k).unwrap().iter().map(|(_, v)| *v).collect();
        assert_eq!(hist, vec![2.0]);
    }

    #[test]
    fn spark_ring_keeps_the_last_values_in_order() {
        let mut s = Spark::default();
        for i in 1..=20 {
            s.push(i as f32);
        }
        let (buf, len) = s.snapshot();
        // Capacity is 16, so the 20 pushes leave the most recent 16 (5..=20), oldest first.
        assert_eq!(len, SPARK_CAP);
        assert_eq!(buf[0], 5.0);
        assert_eq!(buf[len - 1], 20.0);
    }

    #[test]
    fn upsert_records_spark_for_every_series() {
        let mut m = Model::new(None, Duration::from_secs(60));
        let now = Instant::now();
        let k = key("cpu", "p1");
        // Sparklines are recorded even though the series is never watched.
        m.upsert(k.clone(), val(1.0, now));
        m.upsert(k.clone(), val(2.0, now));
        let rows = m.rows();
        assert_eq!(&rows[0].spark[..rows[0].spark_len], &[1.0, 2.0]);
    }

    #[test]
    fn evict_disabled_keeps_everything() {
        let mut m = Model::new(None, Duration::from_secs(60));
        let now = Instant::now();
        m.upsert(key("cpu", "dead"), val(1.0, now - Duration::from_secs(9999)));
        m.evict(now);
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn filter_is_case_insensitive_and_scoped() {
        let mut rows = vec![
            row("cpu_usage", "local_machine", "Firefox", 1.0),
            row("cpu_usage", "local_machine", "chrome", 2.0),
        ];
        let re = compile_filter("fire");
        apply_filter(&mut rows, re.as_ref(), FilterColumn::Consumer);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].consumer, "Firefox");
    }

    #[test]
    fn filter_column_scope_excludes_other_fields() {
        let mut rows = vec![row("cpu_usage", "local_machine", "chrome", 1.0)];
        // "cpu" is in the metric, but we only look at the consumer column.
        let re = compile_filter("cpu");
        apply_filter(&mut rows, re.as_ref(), FilterColumn::Consumer);
        assert!(rows.is_empty());
    }

    #[test]
    fn filter_supports_regex_patterns() {
        let mut rows = vec![
            row("cpu_usage", "local_machine", "firefox", 1.0),
            row("cpu_time", "local_machine", "chrome", 2.0),
            row("ram_usage", "local_machine", "chrome", 3.0),
        ];
        // Anchors and alternation: metric starting with "cpu" and ending in "usage" or "time".
        let re = compile_filter("^cpu.*(usage|time)$");
        apply_filter(&mut rows, re.as_ref(), FilterColumn::Metric);
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn parse_filter_scope_overrides_the_column() {
        // A recognized `$column=` prefix scopes to that column with the rest as the pattern.
        assert_eq!(
            parse_filter_scope("$metric=cpu.*", FilterColumn::All),
            Some((FilterColumn::Metric, "cpu.*"))
        );
        // split_once keeps later '=' in the pattern, so attribute values survive.
        assert_eq!(
            parse_filter_scope("$attributes=core=2", FilterColumn::All),
            Some((FilterColumn::Attributes, "core=2"))
        );
        // No prefix: the whole input is the pattern on the default column.
        assert_eq!(
            parse_filter_scope("cpu.*", FilterColumn::Consumer),
            Some((FilterColumn::Consumer, "cpu.*"))
        );
    }

    #[test]
    fn parse_filter_scope_is_inactive_while_composing() {
        // Starts with `$` but no `=` yet: composing a scope, not a pattern → inactive.
        assert_eq!(parse_filter_scope("$resou", FilterColumn::All), None);
        assert_eq!(parse_filter_scope("$resource", FilterColumn::All), None);
        // `=` present but the column name is unknown → still inactive.
        assert_eq!(parse_filter_scope("$nope=x", FilterColumn::All), None);
        // Completed and recognized, even with an empty pattern.
        assert_eq!(
            parse_filter_scope("$resource=", FilterColumn::All),
            Some((FilterColumn::Resource, ""))
        );
    }

    #[test]
    fn filter_scope_targets_a_single_column() {
        let mut rows = vec![
            row("cpu_usage", "local_machine", "firefox", 1.0),
            row("ram_usage", "local_machine", "cpu_helper", 2.0),
        ];
        // `$metric=` matches "cpu" in the metric only — not the consumer that also contains "cpu".
        let (col, pat) = parse_filter_scope("$metric=cpu", FilterColumn::All).unwrap();
        let re = compile_filter(pat);
        apply_filter(&mut rows, re.as_ref(), col);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].metric, "cpu_usage");
    }

    #[test]
    fn invalid_filter_pattern_is_inactive_not_empty() {
        let mut rows = vec![row("cpu_usage", "local_machine", "chrome", 1.0)];
        // A half-typed pattern fails to compile; the filter is treated as inactive (keeps all rows)
        // rather than matching nothing.
        let re = compile_filter("(unclosed");
        assert!(re.is_none());
        assert!(!valid_filter("(unclosed"));
        apply_filter(&mut rows, re.as_ref(), FilterColumn::All);
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn sort_value_descending() {
        let mut rows = vec![
            row("a", "r", "c", 1.0),
            row("b", "r", "c", 3.0),
            row("c", "r", "c", 2.0),
        ];
        apply_sort(&mut rows, &[(SortColumn::Value, true)]);
        let values: Vec<f64> = rows.iter().map(|r| r.value_num).collect();
        assert_eq!(values, vec![3.0, 2.0, 1.0]);
    }

    #[test]
    fn sort_metric_ascending() {
        let mut rows = vec![row("zeta", "r", "c", 1.0), row("alpha", "r", "c", 1.0)];
        apply_sort(&mut rows, &[(SortColumn::Metric, false)]);
        assert_eq!(rows[0].metric, "alpha");
    }

    #[test]
    fn multi_key_sort_breaks_ties_with_later_keys() {
        // Same metric: the secondary key (resource ascending) decides; value is the tiebreak below.
        let mut rows = vec![
            row("cpu", "r2", "c", 5.0),
            row("cpu", "r1", "c", 1.0),
            row("mem", "r9", "c", 9.0),
        ];
        apply_sort(&mut rows, &[(SortColumn::Metric, false), (SortColumn::Resource, false)]);
        let order: Vec<(&str, &str)> = rows.iter().map(|r| (r.metric.as_str(), r.resource.as_str())).collect();
        assert_eq!(order, vec![("cpu", "r1"), ("cpu", "r2"), ("mem", "r9")]);
    }

    #[test]
    fn sort_is_stable_by_identity_when_no_keys_match() {
        // No sort keys at all → ordered purely by the identity tiebreak (metric, then resource…).
        let mut rows = vec![
            row("cpu", "r2", "c", 1.0),
            row("cpu", "r1", "c", 1.0),
            row("acpi", "r1", "c", 1.0),
        ];
        apply_sort(&mut rows, &[]);
        let order: Vec<(&str, &str)> = rows.iter().map(|r| (r.metric.as_str(), r.resource.as_str())).collect();
        assert_eq!(order, vec![("acpi", "r1"), ("cpu", "r1"), ("cpu", "r2")]);
    }

    #[test]
    fn shrinking_history_window_drops_samples_outside_it() {
        let mut m = Model::new(None, Duration::from_secs(60));
        let now = Instant::now();
        let k = key("cpu", "p1");
        m.watch(k.clone());
        m.upsert(k.clone(), val(1.0, now - Duration::from_secs(40)));
        m.upsert(k.clone(), val(2.0, now));
        // Both samples are within the 60s window.
        assert_eq!(m.history(&k).unwrap().len(), 2);
        // Shrink to 10s: the 40s-old sample falls outside and is dropped.
        m.set_history_window(Duration::from_secs(10));
        let hist: Vec<f64> = m.history(&k).unwrap().iter().map(|(_, v)| *v).collect();
        assert_eq!(hist, vec![2.0]);
    }

    /// A terse outline of a view: one `(depth, marker, label)` per item, where the marker is `#`
    /// for a group node and `-` for a leaf row, so nesting is easy to assert.
    fn outline(items: &[ViewItem]) -> Vec<(usize, char, String)> {
        items
            .iter()
            .map(|i| match i {
                ViewItem::Group(g) => (g.depth, '#', g.label.clone()),
                ViewItem::Row { row, depth } => (*depth, '-', row.metric.clone()),
            })
            .collect()
    }

    #[test]
    fn empty_group_by_yields_the_flat_row_list() {
        let rows = vec![row("cpu", "m", "p1", 1.0), row("mem", "m", "p2", 2.0)];
        let items = build_grouped_view(rows, &[], &HashSet::new());
        assert_eq!(
            outline(&items),
            vec![(0, '-', "cpu".to_owned()), (0, '-', "mem".to_owned())]
        );
    }

    #[test]
    fn nested_grouping_builds_a_tree() {
        // Group by metric, then by consumer within each metric.
        let rows = vec![
            row("cpu", "m", "p1", 1.0),
            row("cpu", "m", "p2", 2.0),
            row("mem", "m", "p1", 3.0),
        ];
        let items = build_grouped_view(rows, &[GroupColumn::Metric, GroupColumn::Consumer], &HashSet::new());
        assert_eq!(
            outline(&items),
            vec![
                (0, '#', "cpu".to_owned()), // metric=cpu
                (1, '#', "p1".to_owned()),  //   consumer=p1
                (2, '-', "cpu".to_owned()), //     leaf
                (1, '#', "p2".to_owned()),  //   consumer=p2
                (2, '-', "cpu".to_owned()), //     leaf
                (0, '#', "mem".to_owned()), // metric=mem
                (1, '#', "p1".to_owned()),  //   consumer=p1
                (2, '-', "mem".to_owned()), //     leaf
            ]
        );
        let ViewItem::Group(cpu) = &items[0] else {
            panic!("expected node")
        };
        assert_eq!(cpu.count, 2);
    }

    #[test]
    fn collapsing_a_node_hides_its_whole_subtree() {
        let rows = vec![
            row("cpu", "m", "p1", 1.0),
            row("cpu", "m", "p2", 2.0),
            row("mem", "m", "p1", 3.0),
        ];
        let group_by = [GroupColumn::Metric, GroupColumn::Consumer];
        let cpu_key = path_key(&[(GroupColumn::Metric, "cpu".to_owned())]);
        let collapsed: HashSet<String> = [cpu_key].into_iter().collect();
        let items = build_grouped_view(rows, &group_by, &collapsed);
        assert_eq!(
            outline(&items),
            vec![
                (0, '#', "cpu".to_owned()), // collapsed: no children shown
                (0, '#', "mem".to_owned()),
                (1, '#', "p1".to_owned()),
                (2, '-', "mem".to_owned()),
            ]
        );
        let ViewItem::Group(cpu) = &items[0] else {
            panic!("expected node")
        };
        assert!(cpu.collapsed);
        assert_eq!(cpu.count, 2); // count still reflects the hidden members
    }

    #[test]
    fn ancestor_keys_match_the_built_node_keys() {
        let rows = vec![row("cpu", "m", "p1", 1.0)];
        let group_by = [GroupColumn::Metric, GroupColumn::Consumer];
        let items = build_grouped_view(rows.clone(), &group_by, &HashSet::new());
        let node_keys: Vec<String> = items
            .iter()
            .filter_map(|i| match i {
                ViewItem::Group(g) => Some(g.key.clone()),
                _ => None,
            })
            .collect();
        // The ancestor keys of the (only) leaf are exactly the two group nodes above it.
        assert_eq!(ancestor_keys(&rows[0], &group_by), node_keys);
    }

    #[test]
    fn row_in_path_matches_only_members() {
        let r = row("cpu", "machine", "p1", 1.0);
        let path = vec![
            (GroupColumn::Metric, "cpu".to_owned()),
            (GroupColumn::Consumer, "p1".to_owned()),
        ];
        assert!(row_in_path(&r, &path));
        let other = vec![
            (GroupColumn::Metric, "cpu".to_owned()),
            (GroupColumn::Consumer, "p2".to_owned()),
        ];
        assert!(!row_in_path(&r, &other));
    }
}
