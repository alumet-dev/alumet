//! Interactive, htop-style terminal UI.
//!
//! Runs on its own thread (see [`crate::TuiPlugin::start`]). It owns the terminal, periodically
//! snapshots the shared [`Model`], applies the current filter/sort, and draws the rows that fit on
//! screen. The pipeline never touches the terminal — it only updates the model — which keeps the
//! display consistent no matter which source flushed.
//!
//! Two kinds of tabs exist: the measurements table (always present) and graph tabs. A graph tab can
//! plot one or several series at once: mark rows with Space (or just select one), then press Enter.
//! Graph tabs render as a grid of charts, or overlaid on a single chart (toggle with `o`), and
//! refresh in real time. While a graph tab is open the model records its series' samples (see
//! [`Model::watch`]); closing the tab stops recording for series no longer shown anywhere.

use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use time::OffsetDateTime;
use time::macros::format_description;

use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
        MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Axis, Block, Borders, Cell, Chart, Dataset, GraphType, Paragraph, Row as TableRow, Table, TableState, Tabs,
};
use regex::Regex;

use crate::logcap::{LogBuffer, LogEntry, StderrCapture, new_buffer};
use crate::logo;
use crate::model::{
    FilterColumn, GroupColumn, GroupNode, ItemId, Model, Row, SPARK_CAP, Sample, SeriesKey, SortColumn, ViewItem,
    ancestor_keys, apply_filter, apply_sort, build_grouped_view, compile_filter, key_matches, parse_filter_scope,
    row_in_path, valid_filter,
};
use crate::theme;

/// How often the UI redraws when idle (also the input-poll timeout).
const TICK: Duration = Duration::from_millis(150);
/// How often the row *order* is recomputed. Values still refresh every [`TICK`], but re-sorting the
/// table only this often keeps it from reshuffling several times a second when many live series
/// cross each other — far easier to read and navigate. Changing the sort column/direction reorders
/// immediately rather than waiting for the next tick.
const SORT_INTERVAL: Duration = Duration::from_secs(2);
/// When the table first grows past this many series, it auto-groups by metric → consumer (folded),
/// so a firehose opens as a compact overview instead of thousands of rows. Happens at
/// most once, and only if the user hasn't already grouped or filtered — afterwards it's their call.
const AUTO_GROUP_THRESHOLD: usize = 50;
/// Bounds for the on-the-fly graph history window (seconds).
const MIN_HISTORY_SECS: u64 = 1;
const MAX_HISTORY_SECS: u64 = 3600;
/// Timestamp format for the graph raw-data table (UTC, millisecond precision for debugging).
const RAW_TIME_FORMAT: &[time::format_description::FormatItem] =
    format_description!("[hour]:[minute]:[second].[subsecond digits:3]");
/// Number of log lines shown in the log pane (excluding its border).
const LOG_PANE_LINES: u16 = 6;
/// Colors cycled through for graphed series, from the Alumet palette (at least as many as
/// [`MAX_LINES_PER_CHART`] so every line in a single chart gets a distinct color).
const SERIES_COLORS: [Color; 16] = theme::SERIES;

/// How the stderr log pane is shown in the measurements tab, cycled with `l`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogView {
    /// No log pane; the table uses the whole area.
    Hidden,
    /// A small pane at the bottom, below the table.
    Pane,
    /// Logs fill the whole content area (the table is hidden).
    Full,
}

impl LogView {
    fn next(self) -> LogView {
        match self {
            LogView::Pane => LogView::Full,
            LogView::Full => LogView::Hidden,
            LogView::Hidden => LogView::Pane,
        }
    }

    fn label(self) -> &'static str {
        match self {
            LogView::Hidden => "off",
            LogView::Pane => "pane",
            LogView::Full => "full",
        }
    }
}

/// A dimension a graph can facet or label its series by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dim {
    Name,
    Resource,
    Consumer,
    Attributes,
}

impl Dim {
    /// All dimensions, used to compute the legend (whatever the facet key leaves out).
    const ALL: [Dim; 4] = [Dim::Name, Dim::Resource, Dim::Consumer, Dim::Attributes];

    /// This dimension's value for a series, with a readable placeholder for empty attributes.
    fn value(self, k: &SeriesKey) -> String {
        match self {
            Dim::Name => k.metric.clone(),
            Dim::Resource => k.resource.clone(),
            Dim::Consumer => k.consumer.clone(),
            Dim::Attributes if k.attributes.is_empty() => "(no attributes)".to_string(),
            Dim::Attributes => k.attributes.clone(),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Dim::Name => "name",
            Dim::Resource => "resource",
            Dim::Consumer => "consumer",
            Dim::Attributes => "attributes",
        }
    }
}

/// How a graph tab's grid facets series into charts (keys `1`–`8`). The listed dimensions form the
/// facet key: one chart per distinct combination of their values. Series that differ only in the
/// remaining dimensions overlay as separate lines within a chart (labelled by those dimensions).
/// `Name` is always part of the key so two metrics — with different units/scales — never share a
/// chart's Y axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum GraphGroup {
    /// `1`: one chart per metric.
    Name,
    /// `2`: per metric + resource.
    NameResource,
    /// `3`: per metric + resource + consumer (the default).
    #[default]
    NameResourceConsumer,
    /// `4`: per full series (one line each).
    NameResourceConsumerAttributes,
    /// `5`: per metric + consumer.
    NameConsumer,
    /// `6`: per metric + consumer + attributes.
    NameConsumerAttributes,
    /// `7`: per metric + attributes.
    NameAttributes,
    /// `8`: per metric + attributes + resource.
    NameAttributesResource,
}

impl GraphGroup {
    /// The dimensions that form the facet key: one chart per distinct combination of their values.
    fn facets(self) -> &'static [Dim] {
        use Dim::*;
        match self {
            GraphGroup::Name => &[Name],
            GraphGroup::NameResource => &[Name, Resource],
            GraphGroup::NameResourceConsumer => &[Name, Resource, Consumer],
            GraphGroup::NameResourceConsumerAttributes => &[Name, Resource, Consumer, Attributes],
            GraphGroup::NameConsumer => &[Name, Consumer],
            GraphGroup::NameConsumerAttributes => &[Name, Consumer, Attributes],
            GraphGroup::NameAttributes => &[Name, Attributes],
            GraphGroup::NameAttributesResource => &[Name, Attributes, Resource],
        }
    }

    /// Maps a digit key (`1`–`8`) to a grouping, or `None` for any other digit.
    fn from_digit(n: u8) -> Option<GraphGroup> {
        Some(match n {
            1 => GraphGroup::Name,
            2 => GraphGroup::NameResource,
            3 => GraphGroup::NameResourceConsumer,
            4 => GraphGroup::NameResourceConsumerAttributes,
            5 => GraphGroup::NameConsumer,
            6 => GraphGroup::NameConsumerAttributes,
            7 => GraphGroup::NameAttributes,
            8 => GraphGroup::NameAttributesResource,
            _ => return None,
        })
    }

    /// Footer label, e.g. `name/resource/consumer`.
    fn label(self) -> String {
        self.facets().iter().map(|d| d.label()).collect::<Vec<_>>().join("/")
    }
}

/// A column of the graph raw-data table, used as its sort key (cursor moved with `<`/`>`, applied
/// with `s`). The order matches the table's columns left-to-right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum RawColumn {
    #[default]
    Time,
    Metric,
    Value,
    Resource,
    Consumer,
    Attributes,
}

impl RawColumn {
    const ALL: [RawColumn; 6] = [
        RawColumn::Time,
        RawColumn::Metric,
        RawColumn::Value,
        RawColumn::Resource,
        RawColumn::Consumer,
        RawColumn::Attributes,
    ];

    fn label(self) -> &'static str {
        match self {
            RawColumn::Time => "time",
            RawColumn::Metric => "metric",
            RawColumn::Value => "value",
            RawColumn::Resource => "resource",
            RawColumn::Consumer => "consumer",
            RawColumn::Attributes => "attributes",
        }
    }

    /// Natural initial direction: time and value default to descending (newest/largest first).
    fn default_desc(self) -> bool {
        matches!(self, RawColumn::Time | RawColumn::Value)
    }
}

/// A column of the full-screen logs table (used for both sorting and `$column=` filter scoping). The
/// order matches the table's columns left-to-right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum LogColumn {
    #[default]
    Time,
    Level,
    Module,
    Message,
}

impl LogColumn {
    const ALL: [LogColumn; 4] = [LogColumn::Time, LogColumn::Level, LogColumn::Module, LogColumn::Message];

    fn label(self) -> &'static str {
        match self {
            LogColumn::Time => "time",
            LogColumn::Level => "level",
            LogColumn::Module => "module",
            LogColumn::Message => "message",
        }
    }

    /// Natural initial direction: time defaults to descending (newest first); the others ascending
    /// (for `level`, ascending puts the most severe — error — first, matching [`log::Level`]'s order).
    fn default_desc(self) -> bool {
        matches!(self, LogColumn::Time)
    }

    /// The `$column=` filter-scope name, if any, that selects this column.
    fn from_scope_name(name: &str) -> Option<LogColumn> {
        match name {
            "time" => Some(LogColumn::Time),
            "level" => Some(LogColumn::Level),
            "module" => Some(LogColumn::Module),
            "message" => Some(LogColumn::Message),
            _ => None,
        }
    }
}

/// A tab in the UI.
#[derive(Debug, Clone)]
enum Tab {
    /// The measurements table.
    Table,
    /// A live graph of one or several series.
    Graph(Vec<SeriesKey>),
}

/// Mutable state owned by the UI thread (the measurements live in the shared [`Model`]).
struct UiState {
    filter: String,
    editing_filter: bool,
    /// Ordered sort keys: `(column, descending)`, the first primary and each later one breaking the
    /// previous one's ties, in the order the user added them (`s`, see [`UiState::apply_sort_cursor`]).
    /// Empty by default: with no explicit keys the rows fall back to the stable series-identity order
    /// (see [`apply_sort`]), so the table does not reshuffle as values change.
    sort: Vec<(SortColumn, bool)>,
    /// Column the sort cursor is on (moved with `<`/`>`, highlighted in the header); `s` sorts by it.
    sort_cursor: SortColumn,
    /// How the stderr log pane is shown (cycled with `l`).
    log_view: LogView,
    /// Regex filter for the full-screen logs table (separate from `filter`, since log columns differ
    /// from the measurements columns); `$column=` scopes it (see [`parse_log_filter_scope`]).
    log_filter: String,
    /// Ordered multi-column sort for the logs table; defaults to newest first (time, descending).
    log_sort: Vec<(LogColumn, bool)>,
    /// Column the logs-table sort cursor is on (moved with `<`/`>`); `s` sorts by it.
    log_sort_cursor: LogColumn,
    /// Scroll position (selected row) in the full-screen logs table.
    log_row: usize,
    /// Number of rows in the logs table after filtering, refreshed each frame so scrolling can clamp.
    log_count: usize,
    /// When set, the measurements table is frozen on its last snapshot (toggle with `p`).
    paused: bool,
    /// Whether the "about" overlay (logo + version) is showing.
    show_about: bool,
    /// Whether the keybindings help overlay is showing (toggled with `h`, context-sensitive).
    show_help: bool,
    /// Top-level grouping dimension (keys `1`/`2`/`3`), or `None` for a flat list.
    group: Option<GroupColumn>,
    /// Second-level dimension nested inside each group (keys `4`/`5`/`6`), or `None`.
    subgroup: Option<GroupColumn>,
    /// Path keys of the group nodes the user has collapsed (see [`GroupNode::key`]).
    collapsed: HashSet<String>,
    /// Currently selected item — a series or a group node — tracked by identity so it survives
    /// reordering, regrouping and folding.
    selected: Option<ItemId>,
    /// Series marked for multi-series graphs.
    marked: HashSet<SeriesKey>,
    /// How a graph tab's grid facets its series into charts (keys `1`–`8`).
    graph_group: GraphGroup,
    /// Index of the focused chart in a graph grid (moved with `Tab`/`Shift+Tab`), clamped to the
    /// current chart count each frame. It's the chart `f` full-screens.
    graph_focus: usize,
    /// Whether the focused chart is shown full-screen instead of in the grid (toggled with `f`).
    graph_fullscreen: bool,
    /// Number of charts the current graph grid holds, refreshed each frame so chart navigation can
    /// wrap/clamp without the key handler knowing the layout. Zero when not on a graph tab.
    graph_chart_count: usize,
    /// Whether a graph tab shows its series as a raw-sample table instead of charts (toggled `r`).
    graph_raw: bool,
    /// Scroll position (selected row) in the raw-data table.
    graph_raw_row: usize,
    /// Number of rows in the raw-data table, refreshed each frame so scrolling can clamp.
    graph_raw_count: usize,
    /// Ordered multi-column sort for the raw-data table (see [`UiState::sort`]); defaults to newest
    /// samples first (time, descending).
    raw_sort: Vec<(RawColumn, bool)>,
    /// Column the raw-table sort cursor is on (moved with `<`/`>`); `s` sorts by it.
    raw_sort_cursor: RawColumn,
    /// Open tabs; `tabs[0]` is always [`Tab::Table`].
    tabs: Vec<Tab>,
    active_tab: usize,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            filter: String::new(),
            editing_filter: false,
            sort: Vec::new(),
            sort_cursor: SortColumn::Metric,
            log_view: LogView::Pane,
            log_filter: String::new(),
            log_sort: vec![(LogColumn::Time, true)],
            log_sort_cursor: LogColumn::Time,
            log_row: 0,
            log_count: 0,
            paused: false,
            show_about: false,
            show_help: false,
            group: None,
            subgroup: None,
            collapsed: HashSet::new(),
            selected: None,
            marked: HashSet::new(),
            graph_group: GraphGroup::default(),
            graph_focus: 0,
            graph_fullscreen: false,
            graph_chart_count: 0,
            graph_raw: false,
            graph_raw_row: 0,
            graph_raw_count: 0,
            raw_sort: vec![(RawColumn::Time, true)],
            raw_sort_cursor: RawColumn::Time,
            tabs: vec![Tab::Table],
            active_tab: 0,
        }
    }
}

impl UiState {
    fn in_graph(&self) -> bool {
        matches!(self.tabs[self.active_tab], Tab::Graph(_))
    }

    /// The selected series, if a leaf row (not a group node) is selected.
    fn selected_series(&self) -> Option<SeriesKey> {
        match &self.selected {
            Some(ItemId::Series(k)) => Some(k.clone()),
            _ => None,
        }
    }

    /// The effective grouping levels, outermost first: the group dimension then the subgroup, each
    /// included only when set. A subgroup equal to the group is dropped (it would yield one trivial
    /// subgroup per group). Empty means a flat list.
    fn group_by(&self) -> Vec<GroupColumn> {
        let mut levels = Vec::new();
        if let Some(g) = self.group {
            levels.push(g);
        }
        if let Some(s) = self.subgroup
            && self.subgroup != self.group
        {
            levels.push(s);
        }
        levels
    }

    /// Sets the group dimension to `col`, or clears it if already set to `col`.
    fn toggle_group(&mut self, col: GroupColumn) {
        self.group = (self.group != Some(col)).then_some(col);
    }

    /// Sets the subgroup dimension to `col`, or clears it if already set to `col`.
    fn toggle_subgroup(&mut self, col: GroupColumn) {
        self.subgroup = (self.subgroup != Some(col)).then_some(col);
    }

    /// Moves the graph chart focus by `delta`, wrapping around the current chart count. A no-op when
    /// there are no charts.
    fn focus_chart(&mut self, delta: isize) {
        let n = self.graph_chart_count;
        if n == 0 {
            return;
        }
        let i = self.graph_focus as isize + delta;
        self.graph_focus = i.rem_euclid(n as isize) as usize;
    }

    /// Scrolls the raw-data table by `delta` rows, clamped to its bounds.
    fn scroll_raw(&mut self, delta: isize) {
        let n = self.graph_raw_count;
        if n == 0 {
            self.graph_raw_row = 0;
            return;
        }
        self.graph_raw_row = (self.graph_raw_row as isize + delta).clamp(0, n as isize - 1) as usize;
    }

    /// Moves the raw-table sort cursor `delta` columns along [`RawColumn::ALL`], clamped to the ends.
    fn move_raw_cursor(&mut self, delta: isize) {
        move_cursor(&RawColumn::ALL, &mut self.raw_sort_cursor, delta);
    }

    /// Whether the full-screen logs table is the focused surface (so filter/sort/scroll keys drive
    /// it instead of the measurements table).
    fn logs_focused(&self) -> bool {
        !self.in_graph() && self.log_view == LogView::Full
    }

    /// The filter string of the focused surface: the logs filter when the logs table is up, otherwise
    /// the shared measurements/raw-table filter.
    fn active_filter_mut(&mut self) -> &mut String {
        if self.logs_focused() {
            &mut self.log_filter
        } else {
            &mut self.filter
        }
    }

    fn move_log_cursor(&mut self, delta: isize) {
        move_cursor(&LogColumn::ALL, &mut self.log_sort_cursor, delta);
    }

    /// Cycles the logs table's sort around the cursor column (append → flip → drop, see
    /// [`cycle_sort_key`]).
    fn apply_log_cursor(&mut self) {
        let col = self.log_sort_cursor;
        cycle_sort_key(&mut self.log_sort, col, col.default_desc());
    }

    /// Scrolls the logs table by `delta` rows, clamped to the filtered row count.
    fn scroll_log(&mut self, delta: isize) {
        let n = self.log_count;
        if n == 0 {
            self.log_row = 0;
            return;
        }
        self.log_row = (self.log_row as isize + delta).clamp(0, n as isize - 1) as usize;
    }

    /// Cycles the raw table's sort around the cursor column (append → flip → drop, see
    /// [`cycle_sort_key`]).
    fn apply_raw_cursor(&mut self) {
        let col = self.raw_sort_cursor;
        cycle_sort_key(&mut self.raw_sort, col, col.default_desc());
    }

    /// Moves the sort cursor `delta` columns along [`SortColumn::ALL`], clamped to the ends. Only
    /// the highlight moves — the table is not re-sorted until `s` commits it.
    fn move_sort_cursor(&mut self, delta: isize) {
        move_cursor(&SortColumn::ALL, &mut self.sort_cursor, delta);
    }

    /// Cycles the cursor's column through the sort list on repeated presses: not sorted by it →
    /// appended as the next (lowest-priority) key with its natural direction, so columns already in
    /// the sort keep their priority; already sorted → its direction flips; flipped → it is dropped.
    /// Earlier keys stay as higher-priority tiebreakers, and an empty list falls back to the stable
    /// identity order (see [`apply_sort`]).
    fn apply_sort_cursor(&mut self) {
        let col = self.sort_cursor;
        cycle_sort_key(&mut self.sort, col, col.default_desc());
    }

    fn prev_tab(&mut self) {
        self.active_tab = if self.active_tab == 0 {
            self.tabs.len() - 1
        } else {
            self.active_tab - 1
        };
    }

    fn next_tab(&mut self) {
        self.active_tab = (self.active_tab + 1) % self.tabs.len();
    }
}

/// What a key press resulted in. Variants that need the current data (rows/model) are applied by
/// the event loop; the rest are handled directly in [`handle_key`].
enum Action {
    None,
    Quit,
    SelectBy(isize),
    SelectFirst,
    SelectLast,
    OpenGraph,
    CloseTab,
    /// Mark/unmark the selection (a single series, or every series under a selected group node).
    Mark,
    /// Collapse every group, or expand them all if any is already collapsed.
    ToggleFoldAll,
    /// Grow (`true`) or shrink (`false`) the graph history window on the fly.
    AdjustHistory(bool),
}

/// Snapshot of the series shown in a graph tab, for one frame.
struct GraphData {
    series: Vec<(SeriesKey, Vec<Sample>)>,
}

/// Restores the terminal to a sane state on drop (also covers panics).
struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
    }
}

/// Runs the interactive UI until either the stop flag is set (agent shutting down) or the user quits.
///
/// When the user quits (`q`/Ctrl-C), the terminal is restored and a graceful agent shutdown is
/// requested, so quitting the view stops Alumet — like quitting `htop`.
pub fn run(shared: Arc<Mutex<Model>>, stop: Arc<AtomicBool>, log_buffer_lines: usize) {
    match event_loop(&shared, &stop, log_buffer_lines) {
        Ok(user_quit) => {
            if user_quit {
                request_shutdown();
            }
        }
        Err(e) => {
            // The guard has restored the terminal by now, so this line is visible.
            log::error!("tui plugin: interactive UI error: {e}");
        }
    }
}

fn event_loop(shared: &Arc<Mutex<Model>>, stop: &Arc<AtomicBool>, log_buffer_lines: usize) -> io::Result<bool> {
    // Capture stderr first, so Alumet's logs land in the log pane instead of over the UI.
    let logs = new_buffer(log_buffer_lines);
    let _capture = StderrCapture::start(logs.clone());
    let _guard = TerminalGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut state = UiState::default();
    let mut table_state = TableState::default();
    // Snapshots kept across iterations so they can stay frozen while paused.
    let mut rows: Vec<Row> = Vec::new();
    let mut total = 0usize;
    let mut graph_snap: Option<GraphData> = None;
    // Which tab `graph_snap` was taken for, and the instant it was taken (the chart's "now").
    let mut graph_snap_tab = usize::MAX;
    let mut snap_now = Instant::now();
    // The time span graphs plot over; re-read each frame since it can be changed on the fly.
    let mut window_secs = 0.0f64;
    // Cached row order (by series key) and when it was last computed; re-sorting is throttled to
    // SORT_INTERVAL so a table of many live series stays readable instead of reshuffling each frame.
    let mut order: Vec<SeriesKey> = Vec::new();
    let mut last_sort: Option<Instant> = None;
    let mut sort_state = state.sort.clone();
    // Whether the one-time auto-grouping of a large table has already been applied.
    let mut auto_grouped = false;
    // Group keys seen on the previous frame; any key not in here is newly appeared and folds by
    // default (covers both grouping changes and new series streaming in), keeping the view compact.
    let mut known_group_keys: HashSet<String> = HashSet::new();

    while !stop.load(Ordering::Relaxed) {
        let live_now = Instant::now();
        let active = state.tabs[state.active_tab].clone();

        // Snapshot under one lock, then release it before rendering. Pausing freezes everything by
        // reusing the last snapshot; the only exception is moving to a graph tab we have no frozen
        // data for, which takes one fresh snapshot of that tab and then freezes it too.
        {
            let model = shared.lock().expect("model mutex poisoned");
            if !state.paused {
                rows = model.rows();
                total = model.len();
                snap_now = live_now;
            }
            window_secs = model.history_window().as_secs_f64();
            match &active {
                Tab::Graph(keys) => {
                    let stale = graph_snap.is_none() || graph_snap_tab != state.active_tab;
                    if !state.paused || stale {
                        graph_snap = Some(GraphData {
                            series: keys
                                .iter()
                                .map(|k| {
                                    let samples = model
                                        .history(k)
                                        .map(|h| h.iter().copied().collect())
                                        .unwrap_or_default();
                                    (k.clone(), samples)
                                })
                                .collect(),
                        });
                        graph_snap_tab = state.active_tab;
                        snap_now = live_now;
                    }
                }
                // Drop any frozen graph when on the table, so returning to a graph re-snapshots it.
                Tab::Table => {
                    graph_snap = None;
                    graph_snap_tab = usize::MAX;
                }
            }
        }

        // Track the chart count so chart focus/full-screen navigation can wrap and clamp without the
        // key handler knowing the layout. Keep the focus within range as grouping or the series set
        // changes the number of charts.
        state.graph_chart_count = match (&active, graph_snap.as_ref()) {
            (Tab::Graph(_), Some(snap)) => group_series(&snap.series, state.graph_group).len(),
            _ => 0,
        };
        state.graph_focus = state.graph_focus.min(state.graph_chart_count.saturating_sub(1));
        // Resolve the filter once per frame: an optional `$column=` prefix overrides the column, and
        // the remaining pattern compiles to a regex (None when empty or invalid → matches
        // everything). A scope still being composed (`$resou`, unknown column) yields `None` here, so
        // it doesn't filter as if it were a pattern. Reused for the raw-table count and the view.
        let (filter_col, filter_re) = match parse_filter_scope(&state.filter, FilterColumn::All) {
            Some((col, pat)) => (col, compile_filter(pat)),
            None => (FilterColumn::All, None),
        };
        // Raw-table row count (samples across the tab's series matching the filter) for scroll
        // clamping; the filter hides whole series, so it counts only matching ones.
        state.graph_raw_count = match (&active, graph_snap.as_ref()) {
            (Tab::Graph(_), Some(snap)) => snap
                .series
                .iter()
                .filter(|(k, _)| key_matches(k, filter_re.as_ref(), filter_col))
                .map(|(_, s)| s.len())
                .sum(),
            _ => 0,
        };
        state.graph_raw_row = state.graph_raw_row.min(state.graph_raw_count.saturating_sub(1));

        // Logs-table row count (entries matching the logs filter) for scroll clamping, only when the
        // full-screen logs table is up.
        state.log_count = if state.logs_focused() {
            filtered_log_count(&logs, &state.log_filter)
        } else {
            0
        };
        state.log_row = state.log_row.min(state.log_count.saturating_sub(1));

        // The first time the firehose is clearly too large to scroll, group by metric → consumer and
        // fold it, so the user lands on a compact overview rather than thousands of rows. Done once,
        // and only if they haven't grouped or filtered already; afterwards grouping is entirely theirs.
        if !auto_grouped
            && state.group.is_none()
            && state.subgroup.is_none()
            && state.filter.is_empty()
            && total > AUTO_GROUP_THRESHOLD
        {
            state.group = Some(GroupColumn::Metric);
            state.subgroup = Some(GroupColumn::Consumer);
            auto_grouped = true;
        }

        // Fold every group by default: any group key not seen on the previous frame — whether it
        // appeared because grouping changed or because new series streamed in (e.g. procfs adding
        // processes) — starts collapsed. Keys already seen are left untouched, so groups you opened
        // stay open. This keeps a high-cardinality firehose folded as it fills, instead of new metric
        // and consumer subgroups popping open as their series arrive. Costs nothing extra when nothing
        // appeared (the key set is unchanged) or when ungrouped (the set is empty).
        let group_keys_now = group_keys(&rows, &state.group_by());
        for key in &group_keys_now {
            if !known_group_keys.contains(key) {
                state.collapsed.insert(key.clone());
            }
        }
        known_group_keys = group_keys_now;

        // Derive the displayed view from the (possibly frozen) snapshot. The row *order* is only
        // recomputed on a throttle (or right away when the sort changes), so the table doesn't
        // reshuffle every frame as live values cross each other; between re-sorts, rows hold their
        // positions and series that appeared since settle into place at the next one. Filtering
        // stays live since it only hides rows — it never moves the ones that remain.
        let force_resort = sort_state != state.sort;
        sort_state = state.sort.clone();
        let mut view = rows.clone();
        if force_resort || last_sort.is_none_or(|t| t.elapsed() >= SORT_INTERVAL) {
            apply_sort(&mut view, &state.sort);
            order = view.iter().map(Row::key).collect();
            last_sort = Some(live_now);
        } else {
            reorder_to_cache(&mut view, &order);
        }
        apply_filter(&mut view, filter_re.as_ref(), filter_col);
        let shown = view.len();
        let items = build_items(&state, &view);

        // Resolve the selection by identity; default to (or fall back to) the first item.
        let mut sel_index = state
            .selected
            .as_ref()
            .and_then(|id| items.iter().position(|it| it.id() == *id));
        if sel_index.is_none() && !items.is_empty() {
            state.selected = Some(items[0].id());
            sel_index = Some(0);
        }
        table_state.select(sel_index);

        // Group keys holding at least one marked series — including under collapsed nodes, hence
        // computed from the full rows rather than the visible items — so their headers can show the
        // mark too. Skipped entirely when nothing is marked.
        let marked_groups: HashSet<String> = if state.marked.is_empty() {
            HashSet::new()
        } else {
            let group_by = state.group_by();
            view.iter()
                .filter(|r| state.marked.contains(&r.key()))
                .flat_map(|r| ancestor_keys(r, &group_by))
                .collect()
        };

        terminal.draw(|frame| {
            draw(
                frame,
                &state,
                &items,
                &mut table_state,
                total,
                shown,
                &logs,
                graph_snap.as_ref(),
                snap_now,
                window_secs,
                &marked_groups,
            );
        })?;

        if event::poll(TICK)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => match handle_key(&mut state, key) {
                    Action::Quit => return Ok(true),
                    Action::None => {}
                    Action::SelectBy(delta) => move_selection(&mut state, &items, delta),
                    Action::SelectFirst => state.selected = items.first().map(ViewItem::id),
                    Action::SelectLast => state.selected = items.last().map(ViewItem::id),
                    Action::OpenGraph => open_graph(&mut state, shared),
                    Action::CloseTab => close_tab(&mut state, shared),
                    Action::Mark => mark_selection(&mut state, &view, &items),
                    Action::ToggleFoldAll => toggle_fold_all(&mut state, &items),
                    Action::AdjustHistory(grow) => adjust_history(shared, grow),
                },
                Event::Mouse(me) => handle_mouse(&mut state, &items, me),
                _ => {}
            }
        }
    }
    Ok(false)
}

/// Builds the visible items from the filtered, sorted `rows`: a flat list when no grouping is
/// active, or the collapsible grouping tree otherwise.
fn build_items(state: &UiState, rows: &[Row]) -> Vec<ViewItem> {
    build_grouped_view(rows.to_vec(), &state.group_by(), &state.collapsed)
}

/// The full set of group-node keys `group_by` produces over `rows`, at every depth. Computed
/// directly from each row's ancestor keys (no tree build) since it runs every frame to detect which
/// groups are newly appeared. Because a node's key is its path prefix, shallower levels keep identical
/// keys across nestings, so adding a level only introduces deeper keys.
fn group_keys(rows: &[Row], group_by: &[GroupColumn]) -> HashSet<String> {
    rows.iter().flat_map(|r| ancestor_keys(r, group_by)).collect()
}

/// Reorders `rows` to match the cached key `order`, keeping each row where it was at the last
/// re-sort so the table doesn't reshuffle between throttled sorts. Series missing from the cache
/// (those that appeared since) are placed after the known ones in a stable metric/resource/consumer
/// order, until the next re-sort folds them into place.
fn reorder_to_cache(rows: &mut Vec<Row>, order: &[SeriesKey]) {
    let rank: HashMap<SeriesKey, usize> = order.iter().cloned().enumerate().map(|(i, k)| (k, i)).collect();
    let mut decorated: Vec<(usize, Row)> = std::mem::take(rows)
        .into_iter()
        .map(|row| (rank.get(&row.key()).copied().unwrap_or(usize::MAX), row))
        .collect();
    decorated.sort_by(|(ra, a), (rb, b)| {
        ra.cmp(rb)
            .then_with(|| (&a.metric, &a.resource, &a.consumer).cmp(&(&b.metric, &b.resource, &b.consumer)))
    });
    *rows = decorated.into_iter().map(|(_, row)| row).collect();
}

/// Moves the selection by `delta` items (group nodes included), clamped to the available range.
fn move_selection(state: &mut UiState, items: &[ViewItem], delta: isize) {
    if items.is_empty() {
        state.selected = None;
        return;
    }
    let current = state
        .selected
        .as_ref()
        .and_then(|id| items.iter().position(|it| it.id() == *id))
        .unwrap_or(0) as isize;
    let next = (current + delta).clamp(0, items.len() as isize - 1) as usize;
    state.selected = Some(items[next].id());
}

/// Handles a mouse event over the table: the wheel scrolls the selection. Other buttons are
/// ignored, and the wheel does nothing in graph tabs (there is nothing to select there).
fn handle_mouse(state: &mut UiState, items: &[ViewItem], me: MouseEvent) {
    if state.in_graph() {
        return;
    }
    match me.kind {
        MouseEventKind::ScrollDown => move_selection(state, items, 1),
        MouseEventKind::ScrollUp => move_selection(state, items, -1),
        _ => {}
    }
}

/// Marks/unmarks the selection: a single series, or every series under a selected group node (the
/// whole subtree), toggling the group off when all its members are already marked.
fn mark_selection(state: &mut UiState, rows: &[Row], items: &[ViewItem]) {
    let members: Vec<SeriesKey> = match &state.selected {
        Some(ItemId::Series(k)) => vec![k.clone()],
        Some(ItemId::Group(key)) => {
            // Recover the node's path to find the leaf series under it (including hidden ones).
            let Some(node) = group_node(items, key) else { return };
            rows.iter()
                .filter(|r| row_in_path(r, &node.path))
                .map(Row::key)
                .collect()
        }
        None => return,
    };
    let all_marked = !members.is_empty() && members.iter().all(|k| state.marked.contains(k));
    for k in members {
        if all_marked {
            state.marked.remove(&k);
        } else {
            state.marked.insert(k);
        }
    }
}

/// Finds the group node with the given fold key among the visible items.
fn group_node<'a>(items: &'a [ViewItem], key: &str) -> Option<&'a GroupNode> {
    items.iter().find_map(|it| match it {
        ViewItem::Group(g) if g.key == key => Some(g),
        _ => None,
    })
}

/// Collapses every visible group node, or expands them all when at least one is already collapsed.
fn toggle_fold_all(state: &mut UiState, items: &[ViewItem]) {
    let keys: Vec<String> = items
        .iter()
        .filter_map(|it| match it {
            ViewItem::Group(g) => Some(g.key.clone()),
            ViewItem::Row { .. } => None,
        })
        .collect();
    if keys.is_empty() {
        return;
    }
    let any_expanded = keys.iter().any(|k| !state.collapsed.contains(k));
    for k in keys {
        if any_expanded {
            state.collapsed.insert(k);
        } else {
            state.collapsed.remove(&k);
        }
    }
}

/// Opens (or focuses) a graph tab for the marked series, or the selected one if none are marked.
fn open_graph(state: &mut UiState, shared: &Arc<Mutex<Model>>) {
    let mut keys: Vec<SeriesKey> = if !state.marked.is_empty() {
        state.marked.iter().cloned().collect()
    } else if let Some(k) = state.selected_series() {
        vec![k]
    } else {
        return;
    };
    keys.sort_by(|a, b| (&a.metric, &a.resource, &a.consumer).cmp(&(&b.metric, &b.resource, &b.consumer)));

    // Focus an existing tab showing exactly the same set of series.
    let wanted: HashSet<&SeriesKey> = keys.iter().collect();
    if let Some(idx) = state.tabs.iter().position(|t| match t {
        Tab::Graph(ks) => ks.iter().collect::<HashSet<_>>() == wanted,
        Tab::Table => false,
    }) {
        state.active_tab = idx;
        return;
    }

    {
        let mut model = shared.lock().expect("model mutex poisoned");
        for k in &keys {
            model.watch(k.clone());
        }
    }
    state.tabs.push(Tab::Graph(keys));
    state.active_tab = state.tabs.len() - 1;
}

/// Closes the active graph tab and stops recording history for series no longer shown anywhere.
fn close_tab(state: &mut UiState, shared: &Arc<Mutex<Model>>) {
    let Tab::Graph(keys) = state.tabs[state.active_tab].clone() else {
        return;
    };
    state.tabs.remove(state.active_tab);
    if state.active_tab >= state.tabs.len() {
        state.active_tab = state.tabs.len() - 1;
    }

    // Series still referenced by another graph tab must keep their history.
    let still_used: HashSet<SeriesKey> = state
        .tabs
        .iter()
        .filter_map(|t| match t {
            Tab::Graph(ks) => Some(ks.clone()),
            Tab::Table => None,
        })
        .flatten()
        .collect();

    let mut model = shared.lock().expect("model mutex poisoned");
    for k in keys {
        if !still_used.contains(&k) {
            model.unwatch(&k);
        }
    }
}

/// Grows or shrinks the graph history window, by a step scaled to the current size (so it feels
/// responsive whether the window is seconds or minutes), clamped to a sane range.
fn adjust_history(shared: &Arc<Mutex<Model>>, grow: bool) {
    let mut model = shared.lock().expect("model mutex poisoned");
    let current = model.history_window().as_secs();
    let step = if current < 60 {
        5
    } else if current < 300 {
        15
    } else {
        60
    };
    let next = if grow {
        current + step
    } else {
        current.saturating_sub(step)
    };
    let next = next.clamp(MIN_HISTORY_SECS, MAX_HISTORY_SECS);
    model.set_history_window(Duration::from_secs(next));
}

/// Moves a sort cursor `delta` columns along `all`, clamped to the ends. Shared by every sortable
/// table (measurements, raw data, logs); only the highlight moves — `s` commits the actual sort.
fn move_cursor<C: Copy + PartialEq>(all: &[C], cursor: &mut C, delta: isize) {
    let i = all.iter().position(|c| c == cursor).unwrap_or(0) as isize;
    let j = (i + delta).clamp(0, all.len() as isize - 1) as usize;
    *cursor = all[j];
}

/// Cycles `col` through a multi-column sort list on repeated `s` presses, shared by every sortable
/// table: not present → appended as the next (lowest-priority) key with its `natural` direction, so
/// earlier keys keep their priority; present at its natural direction → flipped; flipped → dropped.
fn cycle_sort_key<C: Copy + PartialEq>(sort: &mut Vec<(C, bool)>, col: C, natural: bool) {
    match sort.iter().position(|(c, _)| *c == col) {
        None => sort.push((col, natural)),
        Some(i) if sort[i].1 == natural => sort[i].1 = !natural,
        Some(i) => {
            sort.remove(i);
        }
    }
}

fn handle_key(state: &mut UiState, key: KeyEvent) -> Action {
    if state.editing_filter {
        match key.code {
            KeyCode::Enter => state.editing_filter = false,
            KeyCode::Esc => {
                state.active_filter_mut().clear();
                state.editing_filter = false;
            }
            KeyCode::Backspace => {
                state.active_filter_mut().pop();
            }
            KeyCode::Char(c) => state.active_filter_mut().push(c),
            _ => {}
        }
        return Action::None;
    }

    // Ctrl-C quits, since raw mode prevents the terminal from turning it into SIGINT.
    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
        return Action::Quit;
    }

    // The overlays are modal: while one is up, only its close keys do anything (Ctrl-C above still
    // quits). This also stops the toggle key from immediately reopening it.
    if state.show_help {
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('h') | KeyCode::Char('q')) {
            state.show_help = false;
        }
        return Action::None;
    }
    if state.show_about {
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')) {
            state.show_about = false;
        }
        return Action::None;
    }

    // Keys shared by every view.
    match key.code {
        KeyCode::Char('q') => return Action::Quit,
        KeyCode::Char('?') => {
            state.show_about = true;
            return Action::None;
        }
        KeyCode::Char('h') => {
            state.show_help = true;
            return Action::None;
        }
        KeyCode::Char('p') => {
            state.paused = !state.paused;
            return Action::None;
        }
        // Zoom the graph history window: `+` zooms in (shorter span, more detail), `-` zooms out.
        // (`=` doubles as `+` without Shift.)
        KeyCode::Char('+') | KeyCode::Char('=') => return Action::AdjustHistory(false),
        KeyCode::Char('-') | KeyCode::Char('_') => return Action::AdjustHistory(true),
        KeyCode::Left => {
            state.prev_tab();
            return Action::None;
        }
        KeyCode::Right => {
            state.next_tab();
            return Action::None;
        }
        _ => {}
    }

    // Graph view: close, chart grouping, focus, full-screen and the raw-data table.
    if state.in_graph() {
        return match key.code {
            // Esc backs out of the raw table, then full-screen, then closes the tab.
            KeyCode::Esc => {
                if state.graph_raw {
                    state.graph_raw = false;
                } else if state.graph_fullscreen {
                    state.graph_fullscreen = false;
                } else {
                    return Action::CloseTab;
                }
                Action::None
            }
            // Toggle the raw-data table view of the tab's series.
            KeyCode::Char('r') => {
                state.graph_raw = !state.graph_raw;
                state.graph_raw_row = 0;
                Action::None
            }
            // Raw table: filter (live) and per-column sort, mirroring the measurements table.
            KeyCode::Char('/') if state.graph_raw => {
                state.editing_filter = true;
                Action::None
            }
            KeyCode::Char('<') if state.graph_raw => {
                state.move_raw_cursor(-1);
                Action::None
            }
            KeyCode::Char('>') if state.graph_raw => {
                state.move_raw_cursor(1);
                Action::None
            }
            KeyCode::Char('s') if state.graph_raw => {
                state.apply_raw_cursor();
                Action::None
            }
            // Scroll the raw table (no-op in chart mode, where the count is irrelevant).
            KeyCode::Up => {
                state.scroll_raw(-1);
                Action::None
            }
            KeyCode::Down => {
                state.scroll_raw(1);
                Action::None
            }
            KeyCode::PageUp => {
                state.scroll_raw(-10);
                Action::None
            }
            KeyCode::PageDown => {
                state.scroll_raw(10);
                Action::None
            }
            // Ctrl-D / Ctrl-U scroll the raw table 10 rows down / up.
            KeyCode::Char('d') if state.graph_raw && key.modifiers.contains(KeyModifiers::CONTROL) => {
                state.scroll_raw(10);
                Action::None
            }
            KeyCode::Char('u') if state.graph_raw && key.modifiers.contains(KeyModifiers::CONTROL) => {
                state.scroll_raw(-10);
                Action::None
            }
            // Vim-style jumps to the top / bottom of the raw table.
            KeyCode::Char('g') => {
                state.graph_raw_row = 0;
                Action::None
            }
            KeyCode::Char('G') => {
                state.graph_raw_row = state.graph_raw_count.saturating_sub(1);
                Action::None
            }
            // Tab moves the chart focus (filter columns are scoped with `$col=` instead).
            KeyCode::Tab => {
                state.focus_chart(1);
                Action::None
            }
            KeyCode::BackTab => {
                state.focus_chart(-1);
                Action::None
            }
            // Full-screen the focused chart (toggle).
            KeyCode::Char('f') => {
                state.graph_fullscreen = !state.graph_fullscreen;
                Action::None
            }
            // 1-8 pick how the grid facets series into charts.
            KeyCode::Char(c @ '1'..='8') => {
                if let Some(g) = GraphGroup::from_digit(c as u8 - b'0') {
                    state.graph_group = g;
                }
                Action::None
            }
            _ => Action::None,
        };
    }

    // Full-screen logs table: filter, sort and scroll it (the measurements table is hidden).
    if state.logs_focused() {
        match key.code {
            KeyCode::Char('l') => state.log_view = state.log_view.next(),
            KeyCode::Esc => state.log_view = LogView::Pane,
            KeyCode::Char('/') => state.editing_filter = true,
            KeyCode::Char('<') => state.move_log_cursor(-1),
            KeyCode::Char('>') => state.move_log_cursor(1),
            KeyCode::Char('s') => state.apply_log_cursor(),
            KeyCode::Up => state.scroll_log(-1),
            KeyCode::Down => state.scroll_log(1),
            KeyCode::PageUp => state.scroll_log(-10),
            KeyCode::PageDown => state.scroll_log(10),
            // Ctrl-D / Ctrl-U scroll 10 rows down / up.
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => state.scroll_log(10),
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => state.scroll_log(-10),
            KeyCode::Char('g') | KeyCode::Home => state.log_row = 0,
            KeyCode::Char('G') | KeyCode::End => state.log_row = state.log_count.saturating_sub(1),
            _ => {}
        }
        return Action::None;
    }

    // Table view.
    match key.code {
        KeyCode::Char('/') => state.editing_filter = true,
        // Sort: move the column cursor with < / >, then `s` sorts by it (toggling asc/desc).
        KeyCode::Char('<') => state.move_sort_cursor(-1),
        KeyCode::Char('>') => state.move_sort_cursor(1),
        KeyCode::Char('s') => state.apply_sort_cursor(),
        KeyCode::Char('l') => state.log_view = state.log_view.next(),
        // Grouping: 1/2/3 set the group dimension, 4/5/6 the subgroup; Esc clears both, `c` folds all.
        KeyCode::Char('1') => state.toggle_group(GroupColumn::Metric),
        KeyCode::Char('2') => state.toggle_group(GroupColumn::Consumer),
        KeyCode::Char('3') => state.toggle_group(GroupColumn::Resource),
        KeyCode::Char('4') => state.toggle_subgroup(GroupColumn::Metric),
        KeyCode::Char('5') => state.toggle_subgroup(GroupColumn::Consumer),
        KeyCode::Char('6') => state.toggle_subgroup(GroupColumn::Resource),
        KeyCode::Esc => {
            state.group = None;
            state.subgroup = None;
        }
        KeyCode::Char('c') => return Action::ToggleFoldAll,
        KeyCode::Char(' ') => return Action::Mark,
        // Ctrl-D / Ctrl-U jump the selection 10 rows down / up (must precede the plain `d` = unmark).
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => return Action::SelectBy(10),
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => return Action::SelectBy(-10),
        KeyCode::Char('d') => state.marked.clear(),
        KeyCode::Up => return Action::SelectBy(-1),
        KeyCode::Down => return Action::SelectBy(1),
        KeyCode::PageUp => return Action::SelectBy(-10),
        KeyCode::PageDown => return Action::SelectBy(10),
        // Vim-style jumps: `g`/Home to the top, `G`/End to the bottom.
        KeyCode::Char('g') | KeyCode::Home => return Action::SelectFirst,
        KeyCode::Char('G') | KeyCode::End => return Action::SelectLast,
        // Enter folds a selected group node, or opens a graph of a selected series.
        KeyCode::Enter => return enter_action(state),
        _ => {}
    }
    Action::None
}

/// Handles `Enter` in the table: toggles the fold of a selected group, or opens a graph otherwise.
fn enter_action(state: &mut UiState) -> Action {
    match &state.selected {
        Some(ItemId::Group(key)) => {
            let key = key.clone();
            if !state.collapsed.remove(&key) {
                state.collapsed.insert(key);
            }
            Action::None
        }
        _ => Action::OpenGraph,
    }
}

#[allow(clippy::too_many_arguments)]
fn draw(
    frame: &mut Frame,
    state: &UiState,
    items: &[ViewItem],
    table_state: &mut TableState,
    total: usize,
    shown: usize,
    logs: &LogBuffer,
    graph: Option<&GraphData>,
    now: Instant,
    window_secs: f64,
    marked_groups: &HashSet<String>,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // tab bar
            Constraint::Min(0),    // content
            Constraint::Length(1), // footer
        ])
        .split(frame.area());

    draw_tab_bar(frame, chunks[0], state);

    if state.show_help {
        draw_help(frame, chunks[1], state.in_graph(), state.logs_focused());
    } else if state.show_about {
        draw_about(frame, chunks[1]);
    } else {
        match graph {
            Some(graph) if state.graph_raw => draw_graph_table(frame, chunks[1], graph, state),
            Some(graph) => draw_graph(
                frame,
                chunks[1],
                graph,
                state.graph_group,
                state.graph_focus,
                state.graph_fullscreen,
                now,
                window_secs,
            ),
            None => draw_table(frame, chunks[1], state, items, table_state, logs, marked_groups),
        }
    }

    // Count only top-level groups, so the footer's "N groups" stays meaningful when nested.
    let groups = items
        .iter()
        .filter(|it| matches!(it, ViewItem::Group(g) if g.depth == 0))
        .count();
    draw_footer(frame, chunks[2], state, total, shown, groups, window_secs);
}

/// Draws the "about" overlay: the Alumet logo with the plugin name and version, centered in `area`.
fn draw_about(frame: &mut Frame, area: Rect) {
    let mut content = logo::lines();
    content.push(Line::from(""));
    content.push(Line::from(Span::styled("Alumet", theme::brand())));
    // The Alumet framework version is the headline; the plugin's own version is secondary.
    content.push(Line::from(Span::styled(
        format!("v{}", alumet::VERSION),
        Style::default().fg(theme::TEXT).add_modifier(Modifier::BOLD),
    )));
    content.push(Line::from(""));
    content.push(Line::from(Span::styled(
        format!("TUI plugin v{}", env!("CARGO_PKG_VERSION")),
        Style::default().fg(theme::MUTED),
    )));
    content.push(Line::from(""));
    content.push(Line::from(Span::styled(
        "press ? or Esc to close",
        Style::default().fg(theme::FAINT),
    )));

    // Vertically center the block; horizontal centering is handled per line by the alignment.
    let height = (content.len() as u16).min(area.height);
    let y = area.y + area.height.saturating_sub(height) / 2;
    let rect = Rect {
        x: area.x,
        y,
        width: area.width,
        height,
    };
    frame.render_widget(Paragraph::new(content).alignment(Alignment::Center), rect);
}

/// Keybindings shown in the help overlay for the measurements table.
const TABLE_HELP: &[(&str, &str)] = &[
    ("\u{2191}/\u{2193}", "Move the selection"),
    ("PgUp/PgDn", "Move the selection by a page"),
    ("Ctrl-D/Ctrl-U", "Move the selection 10 rows down / up"),
    ("g / G", "Jump to the top / bottom"),
    (
        "/",
        "Edit the filter (live); Enter applies, Esc clears. $col= scopes it",
    ),
    ("< / >", "Move the sort cursor to the previous / next column"),
    (
        "s",
        "Add the cursor column as the next sort key; again flips asc/desc, then drops it. Earlier columns keep priority",
    ),
    ("1 / 2 / 3", "Group by metric / consumer / resource"),
    ("4 / 5 / 6", "Subgroup by metric / consumer / resource"),
    ("Esc", "Clear grouping and subgrouping"),
    ("c", "Collapse all groups (or expand all)"),
    ("Space", "Mark / unmark the selection (or a whole group)"),
    ("d", "Unmark everything"),
    ("Enter", "Fold a group, or open a graph of the selection"),
    ("+ / -", "Zoom the graph history window in / out"),
    ("p", "Pause / resume the view"),
    (
        "l",
        "Cycle the logs: pane \u{2192} full table (scroll/sort/filter) \u{2192} off",
    ),
    ("\u{2190}/\u{2192}", "Switch tabs"),
    ("?", "About (logo and version)"),
    ("h", "Toggle this help"),
    ("q / Ctrl-C", "Quit (stops the agent)"),
];

/// Keybindings shown in the help overlay for a graph tab.
const GRAPH_HELP: &[(&str, &str)] = &[
    ("1\u{2013}8", "Choose how series are grouped into charts"),
    (
        "Tab / Shift-Tab",
        "Move focus to the next / previous chart (chart view)",
    ),
    ("f", "Full-screen the focused chart (toggle)"),
    ("r", "Toggle a raw-data table of the series (timestamps + values)"),
    ("\u{2191}/\u{2193}", "Scroll the raw-data table"),
    ("Ctrl-D/Ctrl-U", "Scroll the raw table 10 rows down / up"),
    ("g / G", "Jump to the top / bottom of the raw-data table"),
    (
        "/",
        "Filter the raw table (live); Enter applies, Esc clears. $col= scopes it",
    ),
    ("< / >", "Move the raw-table sort cursor"),
    ("s", "Sort the raw table by the cursor column (toggle asc/desc)"),
    (
        "+ / -",
        "Zoom the history window in / out (raw table: more / fewer samples)",
    ),
    ("p", "Pause / resume the charts"),
    ("\u{2190}/\u{2192}", "Switch tabs"),
    ("Esc", "Close the graph tab"),
    ("?", "About (logo and version)"),
    ("h", "Toggle this help"),
    ("q / Ctrl-C", "Quit (stops the agent)"),
];

/// Keybindings shown in the help overlay for the full-screen logs table.
const LOGS_HELP: &[(&str, &str)] = &[
    ("\u{2191}/\u{2193}", "Scroll the logs"),
    ("PgUp/PgDn", "Scroll by a page"),
    ("Ctrl-D/Ctrl-U", "Scroll 10 rows down / up"),
    ("g / G", "Jump to the top / bottom"),
    (
        "/",
        "Filter the logs (live regex); Enter applies, Esc clears. $time=/$level=/$module=/$message= scope it",
    ),
    (
        "< / >",
        "Move the sort cursor across columns (time/level/module/message)",
    ),
    (
        "s",
        "Add the cursor column as the next sort key; again flips asc/desc, then drops it",
    ),
    ("l", "Cycle the log view: pane \u{2192} full \u{2192} off"),
    ("Esc", "Back to the small log pane"),
    ("\u{2190}/\u{2192}", "Switch tabs"),
    ("p", "Pause / resume"),
    ("h", "Toggle this help"),
    ("q / Ctrl-C", "Quit (stops the agent)"),
];

/// Draws the keybindings help overlay for the current view, centered in `area`.
fn draw_help(frame: &mut Frame, area: Rect, in_graph: bool, logs: bool) {
    let (title, items) = if logs {
        (" Keybindings \u{2014} logs ", LOGS_HELP)
    } else if in_graph {
        (" Keybindings \u{2014} graph ", GRAPH_HELP)
    } else {
        (" Keybindings \u{2014} measurements ", TABLE_HELP)
    };

    let key_w = items.iter().map(|(k, _)| k.chars().count()).max().unwrap_or(0);
    let mut lines: Vec<Line> = items
        .iter()
        .map(|(k, d)| {
            Line::from(vec![
                Span::styled(format!("{k:>key_w$}"), theme::accent()),
                Span::raw("  "),
                Span::raw(*d),
            ])
        })
        .collect();
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "press h or Esc to close",
        Style::default().fg(theme::FAINT),
    )));

    // Size a centered box to fit the content (plus borders), clamped to the available area.
    let content_w = items
        .iter()
        .map(|(_, d)| key_w + 2 + d.chars().count())
        .max()
        .unwrap_or(0);
    let width = (content_w as u16 + 2).min(area.width);
    let height = (lines.len() as u16 + 2).min(area.height);
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    let rect = Rect { x, y, width, height };

    let block = Block::default().borders(Borders::ALL).title(title);
    frame.render_widget(Paragraph::new(lines).block(block), rect);
}

fn draw_tab_bar(frame: &mut Frame, area: Rect, state: &UiState) {
    // Pin the Alumet brand chip to the left; the tabs take the rest of the row.
    let [brand_area, tabs_area] =
        Layout::horizontal([Constraint::Length(logo::TAG_WIDTH), Constraint::Min(0)]).areas(area);
    frame.render_widget(Paragraph::new(logo::tag()), brand_area);

    let titles: Vec<Line> = state
        .tabs
        .iter()
        .map(|t| match t {
            Tab::Table => Line::from(" measurements "),
            Tab::Graph(ks) if ks.len() == 1 => Line::from(format!(" {} ", series_label(&ks[0]))),
            Tab::Graph(ks) => Line::from(format!(" graph ({} series) ", ks.len())),
        })
        .collect();
    let tabs = Tabs::new(titles)
        .select(state.active_tab)
        .style(Style::default().fg(theme::MUTED))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(theme::GOLD)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(tabs, tabs_area);
}

fn draw_table(
    frame: &mut Frame,
    area: Rect,
    state: &UiState,
    items: &[ViewItem],
    table_state: &mut TableState,
    logs: &LogBuffer,
    marked_groups: &HashSet<String>,
) {
    // Full-screen logs: the measurements table is hidden and the scrollable, sortable, filterable
    // logs table uses the whole content area.
    if state.log_view == LogView::Full {
        draw_logs_table(frame, area, logs, state);
        return;
    }

    // Otherwise optionally split off a small log pane at the bottom.
    let (table_area, log_area) = if state.log_view == LogView::Pane {
        let parts = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(LOG_PANE_LINES + 2)])
            .split(area);
        (parts[0], Some(parts[1]))
    } else {
        (area, None)
    };

    let header = TableRow::new(sort_header_cells(state)).style(Style::default().add_modifier(Modifier::BOLD));

    let body = items.iter().map(|item| match item {
        ViewItem::Group(g) => group_table_row(g, marked_groups.contains(&g.key)),
        ViewItem::Row { row, depth } => leaf_table_row(row, &state.marked, *depth),
    });

    let widths = [
        Constraint::Length(1),
        Constraint::Percentage(22),           // metric
        Constraint::Percentage(16),           // resource
        Constraint::Percentage(16),           // consumer
        Constraint::Length(12),               // value
        Constraint::Length(6),                // unit
        Constraint::Length(SPARK_CAP as u16), // trend
        Constraint::Length(8),                // updated
        Constraint::Percentage(14),           // attributes
    ];
    let table = Table::new(body, widths)
        .header(header)
        .column_spacing(2)
        .row_highlight_style(Style::default().bg(theme::SELECTION_BG).add_modifier(Modifier::BOLD))
        .highlight_symbol(Span::styled("\u{258c} ", Style::default().fg(theme::CYAN)));
    frame.render_stateful_widget(table, table_area, table_state);

    if let Some(log_area) = log_area {
        draw_logs(frame, log_area, logs);
    }
}

/// Indentation (in spaces) applied per nesting level so the tree reads as a hierarchy.
const INDENT: usize = 2;

/// Builds the table row for a group node: indented by depth, a fold arrow, the dimension and value,
/// and the number of series beneath it, plus a `*` when any series under it is marked. Its color
/// cycles with depth so levels are easy to tell apart.
fn group_table_row<'a>(g: &GroupNode, marked: bool) -> TableRow<'a> {
    let arrow = if g.collapsed { "\u{25b8}" } else { "\u{25be}" }; // ▸ collapsed / ▾ expanded
    let indent = " ".repeat(g.depth * INDENT);
    let label = format!("{indent}{arrow} {}={} ({})", g.col.label(), g.label, g.count);
    // A mark on the header when any series under this node (even a folded-away one) is marked.
    let mark = if marked { "*" } else { "" };
    let cells = vec![
        Cell::from(mark).style(Style::default().fg(theme::GOLD)),
        Cell::from(label),
        Cell::from(""), // resource
        Cell::from(""), // consumer
        Cell::from(""), // value
        Cell::from(""), // unit
        Cell::from(""), // trend
        Cell::from(""), // updated
        Cell::from(""), // attributes
    ];
    TableRow::new(cells).style(Style::default().fg(depth_color(g.depth)).add_modifier(Modifier::BOLD))
}

/// Builds the table row for a leaf series, indenting the name column to nest it under its group.
fn leaf_table_row<'a>(r: &Row, marked: &HashSet<SeriesKey>, depth: usize) -> TableRow<'a> {
    let mark = if marked.contains(&r.key()) { "*" } else { "" };
    let metric = format!("{}{}", " ".repeat(depth * INDENT), r.metric);
    TableRow::new(vec![
        Cell::from(mark).style(Style::default().fg(theme::GOLD)),
        Cell::from(metric),
        Cell::from(r.resource.clone()),
        Cell::from(r.consumer.clone()),
        Cell::from(Line::from(r.value.clone()).alignment(Alignment::Right)),
        Cell::from(r.unit.clone()),
        Cell::from(sparkline(&r.spark[..r.spark_len])).style(Style::default().fg(theme::CYAN)),
        Cell::from(r.updated.clone()),
        Cell::from(r.attributes.clone()),
    ])
}

/// Builds the measurements-table header cells (see [`sorted_header_cells`]). The two `None` columns
/// (the mark gutter and the `trend` sparkline) are not sortable. Order matches the body and `widths`.
fn sort_header_cells(state: &UiState) -> Vec<Cell<'static>> {
    const COLS: [(&str, Option<SortColumn>); 9] = [
        ("", None),
        ("metric", Some(SortColumn::Metric)),
        ("resource", Some(SortColumn::Resource)),
        ("consumer", Some(SortColumn::Consumer)),
        ("value", Some(SortColumn::Value)),
        ("unit", Some(SortColumn::Unit)),
        ("trend", None),
        ("updated", Some(SortColumn::Updated)),
        ("attributes", Some(SortColumn::Attributes)),
    ];
    sorted_header_cells(&COLS, &state.sort, state.sort_cursor)
}

/// Builds header cells for any sortable table: each sorted column gets an ▲/▼ arrow plus a small
/// priority number (¹²³…) when more than one column is sorted, so the multi-column order is visible;
/// the column under the sort cursor is reversed so it is clear what `s` will act on. `cols` lists the
/// `(label, sort-column)` in display order, with `None` for non-sortable columns.
fn sorted_header_cells<C: Copy + PartialEq>(
    cols: &[(&str, Option<C>)],
    sort: &[(C, bool)],
    cursor: C,
) -> Vec<Cell<'static>> {
    let multi = sort.len() > 1;
    cols.iter()
        .map(|(label, col)| {
            // Where this column sits in the sort order (if at all), and its direction.
            let rank = col.and_then(|c| sort.iter().position(|(sc, _)| *sc == c));
            let text = match rank {
                Some(i) => {
                    let arrow = if sort[i].1 { '\u{25bc}' } else { '\u{25b2}' };
                    let prio = if multi { superscript(i + 1) } else { String::new() };
                    format!("{label} {arrow}{prio}")
                }
                None => label.to_string(),
            };
            // Sorted columns read in accent cyan; the cursor column is reversed on top.
            let mut style = if rank.is_some() {
                theme::accent()
            } else {
                Style::default()
            };
            if *col == Some(cursor) {
                style = style.add_modifier(Modifier::REVERSED);
            }
            Cell::from(text).style(style)
        })
        .collect()
}

/// Renders a small positive integer as Unicode superscript digits (e.g. `12` → `¹²`).
fn superscript(mut n: usize) -> String {
    const SUP: [char; 10] = [
        '\u{2070}', '\u{00b9}', '\u{00b2}', '\u{00b3}', '\u{2074}', '\u{2075}', '\u{2076}', '\u{2077}', '\u{2078}',
        '\u{2079}',
    ];
    if n == 0 {
        return SUP[0].to_string();
    }
    let mut digits = Vec::new();
    while n > 0 {
        digits.push(SUP[n % 10]);
        n /= 10;
    }
    digits.iter().rev().collect()
}

/// A header color for the given nesting depth, cycling through the Alumet palette.
fn depth_color(depth: usize) -> Color {
    const PALETTE: [Color; 3] = [theme::CYAN, theme::GOLD, theme::ORANGE];
    PALETTE[depth % PALETTE.len()]
}

/// The block glyphs used for sparklines, from lowest to highest.
const SPARK_BLOCKS: [char; 8] = [
    '\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}', '\u{2588}',
];

/// Renders recent `values` as a block-glyph sparkline, autoscaled to their own min/max so each row's
/// trend stays legible whatever its magnitude. A flat series shows a steady mid-level line; no values
/// yields an empty string.
fn sparkline(values: &[f32]) -> String {
    let (min, max) = values.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &v| {
        (lo.min(v), hi.max(v))
    });
    let range = max - min;
    values
        .iter()
        .map(|&v| {
            let level = if range > f32::EPSILON {
                (((v - min) / range) * (SPARK_BLOCKS.len() - 1) as f32).round() as usize
            } else {
                SPARK_BLOCKS.len() / 2 // flat line: a steady mid-level bar
            };
            SPARK_BLOCKS[level.min(SPARK_BLOCKS.len() - 1)]
        })
        .collect()
}

fn draw_logs(frame: &mut Frame, area: Rect, logs: &LogBuffer) {
    let capacity = area.height.saturating_sub(2) as usize; // minus borders
    let lines: Vec<String> = {
        let store = logs.lock().expect("log buffer mutex poisoned");
        store
            .entries()
            .iter()
            .rev()
            .take(capacity)
            .rev()
            .map(fmt_log_oneline)
            .collect()
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" logs (stderr) — [l] full ");
    let pane = Paragraph::new(lines.join("\n"))
        .block(block)
        .style(Style::default().fg(theme::MUTED));
    frame.render_widget(pane, area);
}

/// Formats one log entry as a single tail line: `HH:MM:SS LEVEL message` (the module is dropped here
/// to keep the small pane readable; the full table shows every column).
fn fmt_log_oneline(e: &LogEntry) -> String {
    let level = e.level.map(|l| l.as_str()).unwrap_or("");
    format!("{} {:<5} {}", fmt_log_time(&e.time), level, e.message)
}

/// Shortens an RFC3339 timestamp to its `HH:MM:SS` time-of-day for display, falling back to the raw
/// string (or `-`) when it is not in the expected shape.
fn fmt_log_time(ts: &str) -> String {
    // `2026-06-03T14:22:01Z` → `14:22:01`
    match (ts.find('T'), ts.find(['Z', '+', '.'])) {
        (Some(t), Some(end)) if end > t + 1 => ts[t + 1..end].to_string(),
        _ if ts.is_empty() => "-".to_string(),
        _ => ts.to_string(),
    }
}

/// A log entry's text for one column, used for both filtering and sorting.
fn log_field(e: &LogEntry, col: LogColumn) -> &str {
    match col {
        LogColumn::Time => &e.time,
        LogColumn::Level => e.level.map(|l| l.as_str()).unwrap_or(""),
        LogColumn::Module => &e.module,
        LogColumn::Message => &e.message,
    }
}

/// Resolves the logs filter input: an optional `$column=` prefix scopes the regex to one column
/// (`$all=` or no prefix → every column); `None` while a scope is still being typed (e.g. `$mod`),
/// so a half-typed scope does not filter as if it were a pattern.
fn parse_log_filter_scope(input: &str) -> Option<(Option<LogColumn>, &str)> {
    let Some(rest) = input.strip_prefix('$') else {
        return Some((None, input));
    };
    let (name, pattern) = rest.split_once('=')?;
    if name == "all" {
        return Some((None, pattern));
    }
    LogColumn::from_scope_name(name).map(|col| (Some(col), pattern))
}

/// Whether a log entry matches the compiled filter `re` in `scope` (`None` = any column). A `None`
/// regex (empty or invalid pattern) matches everything.
fn log_matches(e: &LogEntry, re: Option<&Regex>, scope: Option<LogColumn>) -> bool {
    let Some(re) = re else {
        return true;
    };
    match scope {
        Some(col) => re.is_match(log_field(e, col)),
        None => LogColumn::ALL.iter().any(|&c| re.is_match(log_field(e, c))),
    }
}

/// Sorts log entries by the ordered multi-column `keys`. The sort is stable, so entries that tie on
/// every key keep their chronological (insertion) order.
fn sort_logs(entries: &mut [&LogEntry], keys: &[(LogColumn, bool)]) {
    entries.sort_by(|a, b| {
        keys.iter().fold(std::cmp::Ordering::Equal, |acc, &(col, desc)| {
            acc.then_with(|| {
                let o = match col {
                    LogColumn::Time => a.time.cmp(&b.time),
                    LogColumn::Level => a.level.cmp(&b.level),
                    LogColumn::Module => a.module.cmp(&b.module),
                    LogColumn::Message => a.message.cmp(&b.message),
                };
                if desc { o.reverse() } else { o }
            })
        })
    });
}

/// Number of log entries matching the current logs filter (for scroll clamping).
fn filtered_log_count(logs: &LogBuffer, log_filter: &str) -> usize {
    let (scope, re) = match parse_log_filter_scope(log_filter) {
        Some((scope, pat)) => (scope, compile_filter(pat)),
        None => (None, None),
    };
    let store = logs.lock().expect("log buffer mutex poisoned");
    store
        .entries()
        .iter()
        .filter(|e| log_matches(e, re.as_ref(), scope))
        .count()
}

/// A foreground color for a log level, echoing the rest of the UI: ember for errors, orange for
/// warnings, plain text for info, and progressively dimmer for debug/trace.
fn level_color(level: Option<log::Level>) -> Color {
    match level {
        Some(log::Level::Error) => theme::EMBER,
        Some(log::Level::Warn) => theme::ORANGE,
        Some(log::Level::Info) => theme::TEXT,
        Some(log::Level::Debug) => theme::MUTED,
        _ => theme::FAINT,
    }
}

/// Draws the full-screen logs table: one row per captured entry (time · level · module · message),
/// filtered and sorted live like the measurements table. Reads the shared buffer under its lock and
/// borrows entries (no per-frame copy), so it stays cheap even with a large buffer.
fn draw_logs_table(frame: &mut Frame, area: Rect, logs: &LogBuffer, state: &UiState) {
    let (scope, re) = match parse_log_filter_scope(&state.log_filter) {
        Some((scope, pat)) => (scope, compile_filter(pat)),
        None => (None, None),
    };
    let store = logs.lock().expect("log buffer mutex poisoned");
    let mut entries: Vec<&LogEntry> = store
        .entries()
        .iter()
        .filter(|e| log_matches(e, re.as_ref(), scope))
        .collect();
    sort_logs(&mut entries, &state.log_sort);

    let cols: Vec<(&str, Option<LogColumn>)> = LogColumn::ALL.iter().map(|&c| (c.label(), Some(c))).collect();
    let header = TableRow::new(sorted_header_cells(&cols, &state.log_sort, state.log_sort_cursor))
        .style(Style::default().add_modifier(Modifier::BOLD));
    let body = entries.iter().map(|e| {
        let level = e.level.map(|l| l.as_str()).unwrap_or("");
        TableRow::new(vec![
            Cell::from(fmt_log_time(&e.time)),
            Cell::from(level).style(Style::default().fg(level_color(e.level))),
            Cell::from(e.module.clone()),
            Cell::from(e.message.clone()),
        ])
    });
    let widths = [
        Constraint::Length(8),      // time HH:MM:SS
        Constraint::Length(5),      // level
        Constraint::Percentage(22), // module
        Constraint::Min(20),        // message
    ];
    let title = format!(
        " logs (stderr) \u{2014} {} lines · [/]filter [</>][s]sort · [l]/[Esc] back ",
        entries.len()
    );
    let table = Table::new(body, widths)
        .header(header)
        .column_spacing(2)
        .block(Block::default().borders(Borders::ALL).title(title))
        .row_highlight_style(Style::default().bg(theme::SELECTION_BG).add_modifier(Modifier::BOLD))
        .highlight_symbol(Span::styled("\u{258c} ", Style::default().fg(theme::CYAN)));

    let mut ts = TableState::default();
    if !entries.is_empty() {
        ts.select(Some(state.log_row.min(entries.len() - 1)));
    }
    frame.render_stateful_widget(table, area, &mut ts);
}

/// One series to plot on a chart: an optional legend name, a color, and its samples.
type ChartItem<'a> = (Option<String>, Color, &'a [Sample]);

/// Most lines a single chart will draw; beyond this the highest current values are kept and the
/// rest are summarised in the title, so a coarse grouping (e.g. one metric across hundreds of
/// consumers) stays legible instead of becoming a tangle of recoloured lines.
const MAX_LINES_PER_CHART: usize = 12;

#[allow(clippy::too_many_arguments)]
fn draw_graph(
    frame: &mut Frame,
    area: Rect,
    graph: &GraphData,
    group: GraphGroup,
    focus: usize,
    fullscreen: bool,
    now: Instant,
    window_secs: f64,
) {
    if graph.series.is_empty() {
        return;
    }

    // One chart per facet-key combination (see [`GraphGroup`]). Series that differ only in the
    // dimensions left out of the key overlay as lines within a chart, labelled by those dimensions.
    let groups = group_series(&graph.series, group);
    let n = groups.len();
    if n == 0 {
        return;
    }
    let focus = focus.min(n - 1);

    // Full-screen: just the focused chart, captioned with its position so navigation is legible.
    if fullscreen {
        let caption = format!("[{}/{}] ", focus + 1, n);
        draw_group(frame, area, &groups[focus], group, &caption, true, now, window_secs);
        return;
    }

    let cols = (n as f64).sqrt().ceil() as usize;
    let rows = n.div_ceil(cols);

    let row_areas = Layout::vertical(vec![Constraint::Ratio(1, rows as u32); rows]).split(area);
    for r in 0..rows {
        let col_areas = Layout::horizontal(vec![Constraint::Ratio(1, cols as u32); cols]).split(row_areas[r]);
        for (c, cell) in col_areas.iter().enumerate() {
            let idx = r * cols + c;
            if idx >= n {
                break;
            }
            // Highlight the focused chart so it's clear which one `f` will full-screen; only when
            // there's more than one, since a lone chart needs no pointer.
            let focused = n > 1 && idx == focus;
            draw_group(frame, *cell, &groups[idx], group, "", focused, now, window_secs);
        }
    }
}

/// Draws a graph tab's series as a raw-sample table for debugging: one row per stored sample, with
/// its timestamp, metric, value and full series identity (resource, consumer, attributes), newest
/// first. It reads the same (possibly frozen) snapshot as the charts, so it works under pause too.
fn draw_graph_table(frame: &mut Frame, area: Rect, graph: &GraphData, state: &UiState) {
    // Flatten the matching series' samples into rows, then sort by the chosen column/direction. The
    // filter hides whole series (it matches their identity), so all of a series' samples share its
    // fate.
    let (filter_col, filter_re) = match parse_filter_scope(&state.filter, FilterColumn::All) {
        Some((col, pat)) => (col, compile_filter(pat)),
        None => (FilterColumn::All, None),
    };
    let mut rows: Vec<(&SeriesKey, Instant, f64)> = graph
        .series
        .iter()
        .filter(|(k, _)| key_matches(k, filter_re.as_ref(), filter_col))
        .flat_map(|(k, samples)| samples.iter().map(move |(t, v)| (k, *t, *v)))
        .collect();
    sort_raw(&mut rows, &state.raw_sort);

    let header = TableRow::new(raw_header_cells(state)).style(Style::default().add_modifier(Modifier::BOLD));
    let body = rows.iter().map(|(k, t, v)| {
        TableRow::new(vec![
            Cell::from(fmt_sample_time(*t)),
            Cell::from(k.metric.clone()),
            Cell::from(format!("{v}")),
            Cell::from(k.resource.clone()),
            Cell::from(k.consumer.clone()),
            Cell::from(k.attributes.clone()),
        ])
    });
    let widths = [
        Constraint::Length(13),     // time (HH:MM:SS.mmm)
        Constraint::Percentage(22), // metric
        Constraint::Length(16),     // value
        Constraint::Percentage(20), // resource
        Constraint::Percentage(20), // consumer
        Constraint::Percentage(18), // attributes
    ];
    let title = format!(" raw data \u{2014} {} samples · [r]/[Esc] charts ", rows.len());
    let table = Table::new(body, widths)
        .header(header)
        .column_spacing(2)
        .block(Block::default().borders(Borders::ALL).title(title))
        .row_highlight_style(Style::default().bg(theme::SELECTION_BG).add_modifier(Modifier::BOLD))
        .highlight_symbol(Span::styled("\u{258c} ", Style::default().fg(theme::CYAN)));

    let mut ts = TableState::default();
    if !rows.is_empty() {
        ts.select(Some(state.graph_raw_row.min(rows.len() - 1)));
    }
    frame.render_stateful_widget(table, area, &mut ts);
}

/// Sorts raw-table rows by the ordered multi-column `keys`, breaking any remaining ties by time
/// (newest first) so equal keys keep a stable, sensible order.
fn sort_raw(rows: &mut [(&SeriesKey, Instant, f64)], keys: &[(RawColumn, bool)]) {
    rows.sort_by(|a, b| {
        keys.iter()
            .fold(std::cmp::Ordering::Equal, |acc, &(col, desc)| {
                acc.then_with(|| {
                    let o = match col {
                        RawColumn::Time => a.1.cmp(&b.1),
                        RawColumn::Value => a.2.total_cmp(&b.2),
                        RawColumn::Metric => a.0.metric.cmp(&b.0.metric),
                        RawColumn::Resource => a.0.resource.cmp(&b.0.resource),
                        RawColumn::Consumer => a.0.consumer.cmp(&b.0.consumer),
                        RawColumn::Attributes => a.0.attributes.cmp(&b.0.attributes),
                    };
                    if desc { o.reverse() } else { o }
                })
            })
            .then_with(|| b.1.cmp(&a.1))
    });
}

/// Builds the raw-table header cells (see [`sorted_header_cells`]).
fn raw_header_cells(state: &UiState) -> Vec<Cell<'static>> {
    let cols: Vec<(&str, Option<RawColumn>)> = RawColumn::ALL.iter().map(|&c| (c.label(), Some(c))).collect();
    sorted_header_cells(&cols, &state.raw_sort, state.raw_sort_cursor)
}

/// Formats the wall-clock time at which a sample was taken (UTC, millisecond precision). Derived
/// from the sample's monotonic `Instant`, so it remains correct even while the view is paused.
fn fmt_sample_time(t: Instant) -> String {
    let wall = SystemTime::now()
        .checked_sub(t.elapsed())
        .unwrap_or_else(SystemTime::now);
    OffsetDateTime::from(wall)
        .format(RAW_TIME_FORMAT)
        .unwrap_or_else(|_| String::from("?"))
}

/// Draws one chart for a facet group: the series sharing the same facet-key values. They overlay as
/// lines labelled by the dimensions the key leaves out, capped at [`MAX_LINES_PER_CHART`].
#[allow(clippy::too_many_arguments)]
fn draw_group(
    frame: &mut Frame,
    area: Rect,
    group: &[&(SeriesKey, Vec<Sample>)],
    gg: GraphGroup,
    caption: &str,
    focused: bool,
    now: Instant,
    window_secs: f64,
) {
    let first = &group[0].0;
    let unit = first.unit.clone();

    // Lines vary in whatever the facet key omits; that's what the legend shows.
    let legend_dims: Vec<Dim> = Dim::ALL.iter().copied().filter(|d| !gg.facets().contains(d)).collect();

    // Keep the highest current values when there are too many lines to draw legibly.
    let mut lines: Vec<&(SeriesKey, Vec<Sample>)> = group.to_vec();
    lines.sort_by(|a, b| {
        let av = a.1.last().map(|(_, v)| *v).unwrap_or(f64::NEG_INFINITY);
        let bv = b.1.last().map(|(_, v)| *v).unwrap_or(f64::NEG_INFINITY);
        bv.total_cmp(&av)
    });
    let hidden = lines.len().saturating_sub(MAX_LINES_PER_CHART);
    lines.truncate(MAX_LINES_PER_CHART);
    // Draw the survivors in a stable identity order so the legend keeps a fixed position instead of
    // reshuffling as values change. The value sort above only decides *which* lines clear the cap.
    lines.sort_by(|a, b| {
        let (x, y) = (&a.0, &b.0);
        (&x.metric, &x.resource, &x.consumer, &x.attributes).cmp(&(&y.metric, &y.resource, &y.consumer, &y.attributes))
    });

    let body = if group.len() == 1 {
        // Single series: show its latest value, and attributes if any.
        let label = series_label(first);
        match group[0].1.last() {
            Some((_, v)) => format!("{label}  —  {v} {unit}"),
            None => label,
        }
    } else {
        let facet_label: String = gg
            .facets()
            .iter()
            .map(|d| d.value(first))
            .collect::<Vec<_>>()
            .join(" · ");
        if hidden > 0 {
            format!("{facet_label} ({} series, +{hidden} hidden)", group.len())
        } else {
            format!("{facet_label} ({} series)", group.len())
        }
    };
    let title = format!(" {caption}{body} ");

    let multi = group.len() > 1;
    let items: Vec<ChartItem> = lines
        .iter()
        .enumerate()
        .map(|(i, (k, s))| {
            let name = if multi {
                Some(legend_label(k, &legend_dims))
            } else {
                None
            };
            (name, color_for(i), s.as_slice())
        })
        .collect();
    draw_chart(frame, area, title, &unit, &items, focused, now, window_secs);
}

/// Groups series by their facet-key values (see [`GraphGroup`]), preserving first-seen order.
fn group_series(series: &[(SeriesKey, Vec<Sample>)], group: GraphGroup) -> Vec<Vec<&(SeriesKey, Vec<Sample>)>> {
    let facets = group.facets();
    let mut groups: Vec<Vec<&(SeriesKey, Vec<Sample>)>> = Vec::new();
    let mut index: HashMap<Vec<String>, usize> = HashMap::new();
    for item in series {
        let fk: Vec<String> = facets.iter().map(|d| d.value(&item.0)).collect();
        match index.get(&fk) {
            Some(&i) => groups[i].push(item),
            None => {
                index.insert(fk, groups.len());
                groups.push(vec![item]);
            }
        }
    }
    groups
}

/// Legend label for a line within a chart: the values of the dimensions that vary inside it (those
/// the facet key omits). Empty when the key already pins every dimension (one line per chart).
fn legend_label(key: &SeriesKey, dims: &[Dim]) -> String {
    dims.iter().map(|d| d.value(key)).collect::<Vec<_>>().join(" · ")
}

/// Estimates whether any series in `items` has more samples than the chart can resolve, returning
/// the offending point count and the approximate column budget when so. The Braille canvas yields
/// about two dot-columns per terminal cell; we subtract the borders and a rough gutter for the
/// y-axis title/labels. Used to warn the user that detail is being lost (and to suggest zooming in).
fn detail_loss(area: Rect, items: &[ChartItem]) -> Option<(usize, usize)> {
    const Y_AXIS_GUTTER: u16 = 9;
    let inner = area.width.saturating_sub(2 + Y_AXIS_GUTTER);
    let cols = inner as usize * 2;
    let points = items.iter().map(|(_, _, s)| s.len()).max().unwrap_or(0);
    (cols > 0 && points > cols).then_some((points, cols))
}

/// Draws a chart plotting one or several series in `area`.
///
/// The x-axis is pinned to the whole `window_secs` span (the configured history window) rather than
/// fitted to the available samples, so the view does not shrink/grow while history fills up — the
/// line simply starts at the right and extends left as samples accumulate.
#[allow(clippy::too_many_arguments)]
fn draw_chart(
    frame: &mut Frame,
    area: Rect,
    title: String,
    unit: &str,
    items: &[ChartItem],
    focused: bool,
    now: Instant,
    window_secs: f64,
) {
    // The focused chart (in a multi-chart grid) gets a bright bold border so it's clear which one
    // `f` will full-screen.
    let border_style = if focused { theme::accent() } else { Style::default() };
    let bordered = || Block::default().borders(Borders::ALL).border_style(border_style);

    if items.iter().all(|(_, _, s)| s.is_empty()) {
        let block = bordered().title(title);
        let waiting = Paragraph::new("\n  collecting…")
            .block(block)
            .style(Style::default().fg(theme::FAINT));
        frame.render_widget(waiting, area);
        return;
    }

    // Warn when a series has more samples than the chart can resolve: the Braille canvas gives about
    // two dot-columns per cell, so beyond that, points collapse onto shared columns and detail is
    // lost. Prepend a hint suggesting to zoom in (`+`, a shorter window) so it shows even if the
    // title is long.
    let block = match detail_loss(area, items) {
        Some((points, cols)) => {
            let warn = Span::styled(
                format!("⚠ {points} pts>{cols} cols [+] zoom in · "),
                Style::default().fg(theme::EMBER).add_modifier(Modifier::BOLD),
            );
            bordered().title(Line::from(vec![warn, Span::raw(title)]))
        }
        None => bordered().title(title),
    };

    // Build the (x, y) data for every series first, so the datasets can borrow it.
    let data: Vec<Vec<(f64, f64)>> = items.iter().map(|(_, _, s)| to_xy(s, now)).collect();

    // Fixed time window: always span the full history, leaving the unfilled part blank.
    let x_min = -window_secs;
    let (mut y_min, mut y_max) = (f64::INFINITY, f64::NEG_INFINITY);
    for d in &data {
        for (_, y) in d {
            y_min = y_min.min(*y);
            y_max = y_max.max(*y);
        }
    }
    let (y_min, y_max) = padded_y_bounds(y_min, y_max);

    let datasets: Vec<Dataset> = items
        .iter()
        .zip(&data)
        .map(|((name, color, _), d)| {
            let mut ds = Dataset::default()
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(*color))
                .data(d);
            if let Some(name) = name {
                ds = ds.name(name.clone());
            }
            ds
        })
        .collect();

    let chart = Chart::new(datasets)
        .block(block)
        // By default ratatui hides the legend once it would exceed a quarter of the chart's width or
        // height, so it silently vanishes when cells shrink (many graphs) or the legend grows (many
        // series). Allow it the full area instead, so it always shows.
        .hidden_legend_constraints((Constraint::Percentage(100), Constraint::Percentage(100)))
        .x_axis(time_axis(x_min))
        .y_axis(value_axis(unit, y_min, y_max));
    frame.render_widget(chart, area);
}

/// Converts samples to (age-in-seconds, value) points, with "now" at x = 0.
fn to_xy(samples: &[Sample], now: Instant) -> Vec<(f64, f64)> {
    samples
        .iter()
        .map(|(t, v)| (-(now.duration_since(*t).as_secs_f64()), *v))
        .collect()
}

/// Pads the y range so the line is not glued to the chart border, handling empty/flat data.
///
/// When the data never goes negative the lower bound is clamped to 0, so a non-negative metric — in
/// particular a series sitting at 0 — anchors the axis at 0 instead of showing a confusing negative
/// tick from the padding.
fn padded_y_bounds(y_min: f64, y_max: f64) -> (f64, f64) {
    if !y_min.is_finite() || !y_max.is_finite() {
        return (0.0, 1.0);
    }
    let (mut lo, mut hi) = (y_min, y_max);
    if (hi - lo).abs() < f64::EPSILON {
        // Flat line: open a unit window around it.
        lo -= 1.0;
        hi += 1.0;
    } else {
        let pad = (hi - lo) * 0.05;
        lo -= pad;
        hi += pad;
    }
    if y_min >= 0.0 {
        lo = lo.max(0.0);
    }
    (lo, hi)
}

fn time_axis(x_min: f64) -> Axis<'static> {
    let mid = x_min / 2.0;
    Axis::default()
        .title("time (s ago)")
        .style(Style::default().fg(theme::MUTED))
        .bounds([x_min, 0.0])
        .labels([format!("{x_min:.0}"), format!("{mid:.0}"), "now".to_string()])
}

/// Number of labelled ticks on the value axis.
const Y_TICKS: usize = 5;

fn value_axis(unit: &str, y_min: f64, y_max: f64) -> Axis<'static> {
    let range = y_max - y_min;
    let labels: Vec<String> = (0..Y_TICKS)
        .map(|i| {
            let v = y_min + range * i as f64 / (Y_TICKS - 1) as f64;
            format_tick(v, range)
        })
        .collect();
    Axis::default()
        .title(unit.to_owned())
        .style(Style::default().fg(theme::MUTED))
        .bounds([y_min, y_max])
        .labels(labels)
}

/// Formats a tick value with a precision adapted to the axis range, so labels stay readable.
fn format_tick(v: f64, range: f64) -> String {
    let decimals = if range >= 100.0 {
        0
    } else if range >= 10.0 {
        1
    } else if range >= 1.0 {
        2
    } else {
        3
    };
    format!("{v:.decimals$}")
}

/// Color for the line drawn in slot `i` of a chart. The lines are drawn in a stable identity order
/// (see [`draw_group`]) and a chart holds at most [`MAX_LINES_PER_CHART`] of them, so this gives
/// each line a distinct color that also stays put across frames.
fn color_for(i: usize) -> Color {
    SERIES_COLORS[i % SERIES_COLORS.len()]
}

/// A short label for a series. Attributes are appended so series that share the same metric,
/// resource and consumer can still be told apart.
fn series_label(key: &SeriesKey) -> String {
    if key.attributes.is_empty() {
        format!("{} · {}", key.metric, key.consumer)
    } else {
        format!("{} · {} [{}]", key.metric, key.consumer, key.attributes)
    }
}

/// The marked-count chip for the footer, with a hint to clear when any series are marked.
fn marked_hint(n: usize) -> String {
    if n == 0 {
        "0 marked".to_string()
    } else {
        format!("{n} marked [d]clear")
    }
}

fn draw_footer(
    frame: &mut Frame,
    area: Rect,
    state: &UiState,
    total: usize,
    shown: usize,
    groups: usize,
    window_secs: f64,
) {
    let paused = if state.paused { "PAUSED · " } else { "" };
    // The status bar sits on a dark cyan-tinted ink (echoing the logo's circuit traces); it flips to
    // ember while paused so the frozen view is unmistakable.
    let bg = if state.paused { theme::EMBER } else { theme::BAR_BG };
    let base_style = Style::default().fg(theme::TEXT).bg(bg);

    // The filter prompt is rendered with colored spans (the `$column` scope token green/red), so it
    // is handled separately from the plain-string footers below.
    if state.editing_filter {
        let line = if state.logs_focused() {
            filter_prompt_line(&state.log_filter, |n| {
                n == "all" || LogColumn::from_scope_name(n).is_some()
            })
        } else {
            filter_prompt_line(&state.filter, |n| FilterColumn::from_scope_name(n).is_some())
        };
        frame.render_widget(Paragraph::new(line).style(base_style), area);
        return;
    }

    let footer = if state.show_help {
        " help · [h]/[Esc] close ".to_string()
    } else if state.show_about {
        " about · [?]/[Esc] close ".to_string()
    } else if state.in_graph() && state.graph_raw {
        let filter_display = if state.filter.is_empty() {
            "-".to_string()
        } else {
            format!("\"{}\"", state.filter)
        };
        format!(
            " {paused}RAW DATA · {} samples · [/]filter:{} [</>]col [s]sort:{} · [\u{2191}\u{2193}/g/G]scroll [+/-]hist:{}s · [r]/[Esc]charts [\u{2190}\u{2192}]tabs [p]pause [h]help [q]quit ",
            state.graph_raw_count,
            filter_display,
            sort_summary(&state.raw_sort),
            window_secs as u64,
        )
    } else if state.in_graph() {
        // Chart focus/full-screen only mean something with more than one chart.
        let focus = if state.graph_chart_count > 1 {
            if state.graph_fullscreen {
                format!("[Tab]chart {}/{} · ", state.graph_focus + 1, state.graph_chart_count)
            } else {
                "[Tab]focus [f]full · ".to_string()
            }
        } else {
            String::new()
        };
        let esc = if state.graph_fullscreen { "grid" } else { "close" };
        format!(
            " {paused}[\u{2190}/\u{2192}] tabs · [1-8]group:{} · {focus}[+] zoom in [-] out ({}s) · [p] pause · [h] help · [?] about · [Esc] {esc} · [q] quit ",
            state.graph_group.label(),
            window_secs as u64,
        )
    } else if state.logs_focused() {
        let filter_display = if state.log_filter.is_empty() {
            "-".to_string()
        } else {
            format!("\"{}\"", state.log_filter)
        };
        format!(
            " {paused}LOGS · {} lines · [/]filter:{} [</>]col [s]sort:{} · [\u{2191}\u{2193}/g/G]scroll · [l]/[Esc]back [\u{2190}\u{2192}]tabs [p]pause [h]help [q]quit ",
            state.log_count,
            filter_display,
            sort_summary(&state.log_sort),
        )
    } else if state.group_by().is_empty() {
        let filter_display = if state.filter.is_empty() {
            "-".to_string()
        } else {
            format!("\"{}\"", state.filter)
        };
        let marked = marked_hint(state.marked.len());
        format!(
            " {paused}{shown}/{total} · {marked} · [Space]mark [Enter]graph · [/]filter:{} [</>]col [s]sort:{} · [1m2c3r]group [4m5c6r]sub · [p]pause [+/-]hist:{}s [\u{2190}\u{2192}]tabs [l]logs:{} [h]help [?]about [q]quit ",
            filter_display,
            sort_summary(&state.sort),
            window_secs as u64,
            state.log_view.label(),
        )
    } else {
        let marked = marked_hint(state.marked.len());
        let nesting: Vec<&str> = state.group_by().iter().map(|c| c.label()).collect();
        format!(
            " {paused}{shown}/{total} · {groups} groups · {marked} · group:{} · [g/G]top/bot [Space]mark [Enter]fold/graph [c]fold-all [Esc]ungroup · [</>][s]sort:{} · [1m2c3r]group [4m5c6r]sub · [p]pause [+/-]hist:{}s [\u{2190}\u{2192}]tabs [h]help [?]about [q]quit ",
            nesting.join(">"),
            sort_summary(&state.sort),
            window_secs as u64,
        )
    };
    let footer_bar = Paragraph::new(footer).style(base_style);
    frame.render_widget(footer_bar, area);
}

/// A column that can label itself in the footer's sort summary. Implemented by every sortable
/// table's column enum so [`sort_summary`] is shared.
trait ColumnLabel: Copy {
    fn label(self) -> &'static str;
}
impl ColumnLabel for SortColumn {
    fn label(self) -> &'static str {
        SortColumn::label(self)
    }
}
impl ColumnLabel for RawColumn {
    fn label(self) -> &'static str {
        RawColumn::label(self)
    }
}
impl ColumnLabel for LogColumn {
    fn label(self) -> &'static str {
        LogColumn::label(self)
    }
}

/// A compact one-line summary of the multi-column sort for the footer: the primary column with its
/// ▲/▼ direction, plus `+N` when more columns break its ties (e.g. `metric▲ +3`). `default` when no
/// column is explicitly sorted (the stable identity order).
fn sort_summary<C: ColumnLabel>(sort: &[(C, bool)]) -> String {
    match sort.split_first() {
        None => "default".to_string(),
        Some(((col, desc), rest)) => {
            let arrow = if *desc { '\u{25bc}' } else { '\u{25b2}' };
            if rest.is_empty() {
                format!("{}{arrow}", col.label())
            } else {
                format!("{}{arrow} +{}", col.label(), rest.len())
            }
        }
    }
}

/// Builds the colored filter prompt shown while editing, shared by every filterable table. A
/// `$column` scope token shows bold gold when `valid_scope` accepts its name and ember otherwise;
/// the `=`, the pattern and the surrounding chrome stay the footer's default color. The hint flags a
/// pattern that does not compile yet.
fn filter_prompt_line(filter: &str, valid_scope: impl Fn(&str) -> bool) -> Line<'static> {
    // The pattern part to validate: after `$col=`, or the whole input when unscoped. While a scope
    // is still being typed (`$col` with no `=`), there is nothing to validate yet.
    let pattern = match filter.strip_prefix('$') {
        Some(rest) => rest.split_once('=').map(|(_, p)| p).unwrap_or(""),
        None => filter,
    };
    let hint = if valid_filter(pattern) {
        "$col= to scope · Enter: apply · Esc: clear"
    } else {
        "invalid regex · Esc: clear"
    };

    let mut spans = vec![Span::raw(" filter (regex): ")];
    // Color the `$column` scope token; leave `=`, the pattern and the cursor in the default color.
    if let Some(rest) = filter.strip_prefix('$') {
        let (name, tail) = match rest.split_once('=') {
            Some((name, pattern)) => (name, Some(pattern)),
            None => (rest, None),
        };
        let style = if valid_scope(name) {
            theme::brand() // bold gold
        } else {
            Style::default().fg(theme::EMBER)
        };
        spans.push(Span::styled(format!("${name}"), style));
        if let Some(pattern) = tail {
            spans.push(Span::raw(format!("={pattern}")));
        }
    } else {
        spans.push(Span::raw(filter.to_string()));
    }
    spans.push(Span::raw(format!("\u{2588}   ({hint}) ")));
    Line::from(spans)
}

/// Asks the process to shut down gracefully, as if the user had pressed Ctrl-C in cooked mode.
///
/// Alumet stops on SIGINT (`tokio::signal::ctrl_c`), but raw mode swallows the terminal's Ctrl-C,
/// so we deliver the signal ourselves once the terminal has been restored.
fn request_shutdown() {
    #[cfg(unix)]
    {
        // SAFETY: raising a signal is async-signal-safe and always valid for the current process.
        unsafe {
            libc::raise(libc::SIGINT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(metric: &str) -> Row {
        Row {
            metric: metric.to_owned(),
            resource: "r".to_owned(),
            consumer: "c".to_owned(),
            value: String::new(),
            value_num: 0.0,
            unit: String::new(),
            updated: String::new(),
            attributes: String::new(),
            spark: [0.0; SPARK_CAP],
            spark_len: 0,
        }
    }

    fn key(metric: &str, resource: &str, consumer: &str, attributes: &str) -> SeriesKey {
        SeriesKey {
            metric: metric.to_owned(),
            unit: "W".to_owned(),
            resource: resource.to_owned(),
            consumer: consumer.to_owned(),
            attributes: attributes.to_owned(),
        }
    }

    #[test]
    fn group_series_facets_by_the_selected_dimensions() {
        let series = vec![
            (key("power", "cpu(0)", "p1", ""), vec![]),
            (key("power", "cpu(0)", "p2", ""), vec![]),
            (key("power", "cpu(1)", "p3", ""), vec![]),
        ];

        // By name alone, every series shares one chart.
        assert_eq!(group_series(&series, GraphGroup::Name).len(), 1);
        // By name + resource, the two resources split into two charts.
        assert_eq!(group_series(&series, GraphGroup::NameResource).len(), 2);
        // By name + resource + consumer, each series is its own chart.
        assert_eq!(group_series(&series, GraphGroup::NameResourceConsumer).len(), 3);
    }

    #[test]
    fn raw_sort_cursor_walks_and_cycles_like_the_table() {
        let mut s = UiState {
            raw_sort: Vec::new(),
            raw_sort_cursor: RawColumn::Time,
            ..Default::default()
        };
        s.move_raw_cursor(2);
        assert_eq!(s.raw_sort_cursor, RawColumn::Value);
        // Clamp at the right end.
        s.move_raw_cursor(100);
        assert_eq!(s.raw_sort_cursor, RawColumn::Attributes);
        // Apply appends the cursor column with its natural direction (attributes → ascending).
        s.apply_raw_cursor();
        assert_eq!(s.raw_sort, vec![(RawColumn::Attributes, false)]);
        // Applying again on the same column flips the direction, then a third press drops it.
        s.apply_raw_cursor();
        assert_eq!(s.raw_sort, vec![(RawColumn::Attributes, true)]);
        s.apply_raw_cursor();
        assert_eq!(s.raw_sort, vec![]);
    }

    #[test]
    fn sort_raw_orders_by_value_and_time() {
        let k1 = key("m", "r", "c1", "");
        let k2 = key("m", "r", "c2", "");
        let base = Instant::now();
        let t_new = base + Duration::from_secs(1);
        let mut rows = vec![(&k1, base, 5.0), (&k2, t_new, 2.0)];
        // Value descending puts the larger value first.
        sort_raw(&mut rows, &[(RawColumn::Value, true)]);
        assert_eq!(rows[0].2, 5.0);
        // Time descending puts the newest sample first.
        sort_raw(&mut rows, &[(RawColumn::Time, true)]);
        assert_eq!(rows[0].1, t_new);
    }

    #[test]
    fn focus_chart_wraps_around_the_chart_count() {
        let mut state = UiState {
            graph_chart_count: 3,
            ..Default::default()
        };
        state.focus_chart(1);
        assert_eq!(state.graph_focus, 1);
        state.focus_chart(-1);
        assert_eq!(state.graph_focus, 0);
        // Wrap backwards from the first chart to the last.
        state.focus_chart(-1);
        assert_eq!(state.graph_focus, 2);
        // Wrap forwards from the last chart to the first.
        state.focus_chart(1);
        assert_eq!(state.graph_focus, 0);
        // No charts: focus stays put.
        state.graph_chart_count = 0;
        state.focus_chart(1);
        assert_eq!(state.graph_focus, 0);
    }

    #[test]
    fn y_bounds_anchor_at_zero_for_non_negative_data() {
        // A series sitting flat at 0 should anchor the axis at 0, not dip negative from padding.
        assert_eq!(padded_y_bounds(0.0, 0.0), (0.0, 1.0));
        // Range starting at 0: the 5% pad below is clamped away so no negative tick shows.
        assert_eq!(padded_y_bounds(0.0, 100.0), (0.0, 105.0));
        // Strictly positive data keeps its padded headroom (it never crosses 0 anyway).
        assert_eq!(padded_y_bounds(50.0, 100.0), (47.5, 102.5));
        // Genuinely negative data is left alone.
        assert_eq!(padded_y_bounds(-10.0, 10.0), (-11.0, 11.0));
    }

    #[test]
    fn legend_label_shows_only_the_dimensions_outside_the_facet_key() {
        let k = key("power", "cpu(0)", "p1", "core=2");
        // Grouping by name leaves resource/consumer/attributes to label the lines.
        let dims: Vec<Dim> = Dim::ALL
            .iter()
            .copied()
            .filter(|d| !GraphGroup::Name.facets().contains(d))
            .collect();
        assert_eq!(legend_label(&k, &dims), "cpu(0) · p1 · core=2");
    }

    #[test]
    fn reorder_keeps_cached_positions_and_appends_newcomers() {
        // Cached order from the previous re-sort.
        let order = vec![row("c").key(), row("a").key(), row("b").key()];
        // Fresh rows arrive jumbled (live values changed), plus a newcomer "z" not yet in the cache.
        let mut rows = vec![row("a"), row("z"), row("b"), row("c")];
        reorder_to_cache(&mut rows, &order);
        let got: Vec<&str> = rows.iter().map(|r| r.metric.as_str()).collect();
        // Known rows snap back to their cached order; the newcomer trails until the next re-sort.
        assert_eq!(got, vec!["c", "a", "b", "z"]);
    }

    #[test]
    fn sparkline_maps_extremes_to_lowest_and_highest_blocks() {
        let chars: Vec<char> = sparkline(&[0.0, 5.0, 10.0]).chars().collect();
        assert_eq!(chars.len(), 3);
        assert_eq!(chars[0], '\u{2581}'); // min → lowest block
        assert_eq!(*chars.last().unwrap(), '\u{2588}'); // max → highest block
    }

    #[test]
    fn sparkline_is_empty_without_values() {
        assert_eq!(sparkline(&[]), "");
    }

    #[test]
    fn sort_cursor_walks_columns_and_clamps_at_the_ends() {
        let mut s = UiState {
            sort_cursor: SortColumn::Metric,
            ..Default::default()
        };
        s.move_sort_cursor(-1); // already leftmost → clamps
        assert_eq!(s.sort_cursor, SortColumn::Metric);
        s.move_sort_cursor(1);
        assert_eq!(s.sort_cursor, SortColumn::Resource);
    }

    #[test]
    fn apply_sort_cursor_appends_flips_then_drops() {
        let mut s = UiState {
            sort: Vec::new(),
            sort_cursor: SortColumn::Value,
            ..Default::default()
        };
        s.apply_sort_cursor(); // not present → appended, natural (descending) direction
        assert_eq!(s.sort, vec![(SortColumn::Value, true)]);
        s.apply_sort_cursor(); // present at natural direction → flip
        assert_eq!(s.sort, vec![(SortColumn::Value, false)]);
        s.apply_sort_cursor(); // flipped already → dropped
        assert_eq!(s.sort, vec![]);
    }

    #[test]
    fn apply_sort_cursor_appends_a_new_key_at_lower_priority() {
        // Adding a second column keeps the first as the primary key and appends the new one after it.
        let mut s = UiState {
            sort: vec![(SortColumn::Metric, false)],
            sort_cursor: SortColumn::Resource,
            ..Default::default()
        };
        s.apply_sort_cursor();
        assert_eq!(s.sort, vec![(SortColumn::Metric, false), (SortColumn::Resource, false)]);
    }

    #[test]
    fn adding_a_nesting_level_only_introduces_deeper_keys() {
        let rows = vec![row("cpu"), row("mem")];
        let metric_only = group_keys(&rows, &[GroupColumn::Metric]);
        let nested = group_keys(&rows, &[GroupColumn::Metric, GroupColumn::Consumer]);
        // The metric-level keys are unchanged by adding the consumer level (a key is its path prefix),
        // so a regroup folds only the new (deeper) subgroups and leaves already-open metrics open.
        assert!(metric_only.iter().all(|k| nested.contains(k)));
        assert!(nested.len() > metric_only.len());
    }
}
