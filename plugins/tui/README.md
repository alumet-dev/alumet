# TUI plugin

It runs an interactive, **`htop`-style terminal UI**: a live table of every series — each with an
inline trend sparkline — which you can filter, sort, group/fold and explore, plus real-time
**graph tabs** for the series you care about.

It requires an interactive terminal on stdout; when stdout is not a terminal (e.g. piped to a
file), the UI is not displayed.

## Configuration

Put the following in the configuration file of the Alumet agent (usually `alumet-config.toml`).
All keys are optional; the values below are the defaults.

```toml
[plugins.tui]
# Drop a series that has not been updated within this many seconds. Set to 0 to keep
# every series forever (not recommended with sources that emit many short-lived series,
# such as procfs producing one series per process).
stale_after_seconds = 30
# How many seconds of history each graph keeps in memory. This is just the initial value;
# you can zoom the window live from the UI with `+` (zoom in) / `-` (zoom out).
graph_history_seconds = 120
# How many captured log lines (stderr) to keep for the scrollable log table. The oldest line is
# dropped once this is reached. At a few hundred bytes per line this costs ~1-2 MB of RAM at the
# default; raise it for deeper scrollback (e.g. 50000 ≈ ~12 MB), lower it to save memory.
log_buffer_lines = 5000
# Show the metric unit.
print_unit = true
# Use the unit display name (e.g. "J") instead of its unique name (e.g. "joule").
use_unit_display_name = true
```

## Interactive UI

The UI keeps the latest value of every series it has seen, so the table stays consistent regardless
of which source flushed or how often. Series that stop updating are removed after
`stale_after_seconds`, which keeps the view (and memory) bounded.

Its colors are drawn from Alumet's logo — the flame's gold/orange/ember and the circuit traces'
electric cyan — over a dark base: cyan marks live data and interaction (sparklines, sort arrows,
the selection, focus), gold carries the brand and your marks, and ember flags alerts and the paused
state. The palette lives in `src/theme.rs`. A small Alumet brand chip (a flame tip and the wordmark
in the flame gradient) is pinned to the left of the tab bar in every view; the full logo shows in
the about overlay (`?`).

### Measurements table

The **`trend`** column shows an inline sparkline of each series' last few values (most recent on the
right), so you can spot what is rising, spiking or flat across the whole table at a glance — without
opening a graph. Each sparkline is scaled to its own range, so a tiny series and a huge one are both
legible. This history is kept for *every* series (not only graphed ones), bounded to a handful of
recent values, so it stays cheap even with thousands of series.

| Key            | Action                                                        |
| -------------- | ------------------------------------------------------------- |
| `↑` / `↓`      | Move the selection                                            |
| `PgUp`/`PgDn`  | Move the selection by a page                                  |
| `Ctrl-D`/`Ctrl-U` | Move the selection 10 rows down / up                       |
| `g` / `G`      | Jump to the top / bottom (vim-style; `Home` / `End` also work) |
| `/`            | Edit the filter (live, case-insensitive **regex**); scope it to a column with `$col=` (see below). `Enter` applies, `Esc` clears |
| `<` / `>`      | Move the sort cursor to the previous / next column (highlighted in the header) |
| `s`            | Add the highlighted column as the next sort key; press again to flip ▲/▼, then again to drop it. Earlier columns keep their priority (multi-column sort) |
| `1` / `2` / `3`| Group by metric / consumer / resource (the outer level; replaces the current group, press again to clear) |
| `4` / `5` / `6`| Subgroup by metric / consumer / resource (nested inside each group; same toggle rules) |
| `Esc`          | Clear grouping and subgrouping                                |
| `c`            | Collapse all groups (or expand all if any is collapsed)       |
| `Space`        | Mark / unmark the selected series — or, on a group header, every series under it |
| `d`            | Unmark everything (clear all marks)                           |
| `Enter`        | On a group header: fold / unfold it. Otherwise: open a graph of the marked series (or the selected one) |
| `+` / `-`      | Zoom the graph history window in (shorter, more detail) / out (longer) |
| `p`            | Pause: freeze the view on its current values (the footer turns red). Press again to resume |
| `l`            | Cycle the logs: small tail pane → full-screen table (scroll/sort/filter) → off |
| `h`            | Toggle the keybindings help overlay for the current view; `h` or `Esc` closes it |
| `?`            | Show the about overlay (the Alumet logo and plugin version); `?` or `Esc` closes it |
| `←` / `→`      | Switch tabs                                                   |
| `q` / `Ctrl-C` | Quit (stops the agent)                                        |

The mouse wheel also moves the selection.

The filter is a **case-insensitive regular expression** matched against the targeted column(s) — so
`cpu|ram`, `^rapl`, or `core=\d+` all work. While you are typing, a pattern that does not compile
yet is flagged as `invalid regex` in the footer and simply leaves the view unfiltered (it never
makes everything vanish mid-keystroke). The same filter (`/`) is available in the graph tab's
raw-data table.

By default the filter matches every column; a `$column=` prefix scopes the regex to one column:
`$metric=…`, `$resource=…`, `$consumer=…`, `$attributes=…` (and `$all=…`). For example
`$metric=cpu.*` matches the pattern against the metric only. Because attribute matching is an
unanchored regex over the joined attributes, `$attributes=core=2` matches a series carrying that
attribute whatever its order in the displayed list. Input without a `$` prefix is taken whole as the
pattern and matched against all columns.

#### Sorting

Sorting is **multi-column**: it is an ordered list of columns, the first primary and each later one
breaking the previous one's ties, **in the order you add them**. Move the sort cursor with `<`/`>`
(the column under it is highlighted in the header), then press `s`:

- on a column not yet sorted → it is **appended** as the next (lowest-priority) key, with its natural
  direction (descending for `value`, ascending for the text columns), so columns already in the sort
  keep their priority;
- on a column already in the sort → its direction **flips** (▲/▼), and pressing once more **drops** it.

So pressing `s` on metric, then on resource, gives metric (primary) then resource (secondary). The
header marks each sorted column with an ▲/▼ arrow and, when more than one column is sorted, a small
priority number (¹²³…); the footer shows the primary column and `+N` for the rest.

When **no column is explicitly sorted** (the default), rows fall back to a stable order by series
identity — metric, then resource, consumer and attributes, all ascending (the footer shows
`sort:default`). Because that fully determines the order, **rows keep their place as values update**
instead of reshuffling every refresh. This identity order is also applied as the final tiebreak under
any explicit sort, so even sorting by a live column like `value` only moves a row when its value
actually changes relative to its neighbours.

#### Grouping

Grouping has two levels. `1`/`2`/`3` set the **group** (outer) dimension — metric, consumer or
resource — and `4`/`5`/`6` set the **subgroup** (inner) dimension nested inside each group. Each is a
single choice: pressing another key in the range switches it, pressing the same one clears it, and
`Esc` clears both. So `1` then `5` gives metric groups with a consumer subgroup inside each, regardless
of the order you press them. Groups **start folded** — both when you first group and as new groups
appear while series stream in — so the view stays a compact overview instead of popping open under
you; groups you have already opened stay open. Expand a node with `Enter` (or `c` to fold/unfold
everything) to drill down. `Space` on a group header marks its whole subtree at once — convenient for
opening a multi-series graph.

When the table first grows past a few dozen series, the UI groups it **automatically** by
metric → consumer (folded), so a high-cardinality source — e.g. procfs emitting one series per
process — opens as a handful of metric groups instead of lot of rows. This happens once and
only if you have not already grouped or filtered; afterwards grouping is entirely yours to drive
(press `Esc` to ungroup).

### Graph tabs

Selecting a row and pressing `Enter` opens a graph tab that plots the series' recent history and
refreshes in real time. To watch several series at once, mark them with `Space` first, then press
`Enter`: the tab shows one chart per group in a grid.

| Key            | Action                                                        |
| -------------- | ------------------------------------------------------------- |
| `1`–`8`        | Choose how series are grouped into charts (see below)         |
| `Tab`/`Shift-Tab` | Move focus to the next / previous chart (highlighted border) |
| `f`            | Full-screen the focused chart; press again (or `Esc`) to return to the grid |
| `r`            | Toggle a raw-data table of the tab's series (debug); `↑`/`↓`/`PgUp`/`PgDn` (or `Ctrl-D`/`Ctrl-U` for 10 rows) scroll it |
| `h`            | Toggle the keybindings help overlay                           |
| `+` / `-`      | Zoom the history window in (shorter, more detail) / out (longer) |
| `p`            | Pause / resume (freezes the charts on their current data)     |
| `?`            | Show the about overlay (the Alumet logo and plugin version)   |
| `←` / `→`      | Switch tabs                                                   |
| `Esc`          | Return to the grid if full-screen, otherwise close the graph tab |
| `q` / `Ctrl-C` | Quit (stops the agent)                                        |

When a graph tab holds several charts, one is **focused** (shown with a bright border). Move the
focus with `Tab` / `Shift-Tab`, then press `f` to blow that chart up to the full tab — useful for
reading a busy chart in detail. In full-screen, `Tab` still cycles which chart is shown (its title
notes the position, e.g. `[2/5]`), and `f` or `Esc` drops back to the grid.

Press `r` to swap the charts for a **raw-data table** of the same series — one row per stored
sample, with its timestamp (UTC, millisecond precision), metric, value and full identity
(resource, consumer, attributes). It's a debugging view: it reads the same frozen snapshot as the
charts, so it works under pause (`p`) too. Scroll with `↑`/`↓`/`PgUp`/`PgDn` (or `Ctrl-D`/`Ctrl-U` for
10 rows, `g`/`G` to jump to the top / bottom), and press `r` or `Esc` to return to the charts. The
table also supports the same controls as the measurements table: `/` filters live (case-insensitive
regex, with `$col=` scoping),
`<`/`>` move a sort cursor across the columns and `s` sorts by it (toggling asc/descending), and
`+`/`-` widen or narrow the history window so the table shows more or fewer samples.

#### Grouping charts

`1`–`8` choose which dimensions define a chart. The chosen dimensions form the **facet key**: there
is one chart per distinct combination of their values, and series that differ only in the
*remaining* dimensions are overlaid as separate lines inside that chart (the legend shows those
remaining dimensions). The metric name is always part of the key, so two metrics — with different
units and scales — never share a chart's Y axis.

| Key | Groups one chart per…           | Lines within a chart differ by   |
| --- | ------------------------------- | -------------------------------- |
| `1` | metric                          | resource, consumer, attributes   |
| `2` | metric + resource               | consumer, attributes             |
| `3` | metric + resource + consumer    | attributes *(the default)*       |
| `4` | metric + resource + consumer + attributes | nothing (one line per chart) |
| `5` | metric + consumer               | resource, attributes             |
| `6` | metric + consumer + attributes  | resource                         |
| `7` | metric + attributes             | resource, consumer               |
| `8` | metric + attributes + resource  | consumer                         |

The current mode is shown in the footer (e.g. `group:name/resource`). Coarser grouping (e.g. `1`)
can put many series on one chart; each chart keeps at most twelve lines — the ones with the highest
current values — and notes the rest as `+N hidden` in its title, so a high-cardinality metric stays
legible.

Each chart's time axis always spans the full history window, so the view does not stretch while
history fills up: the line starts at the right edge and grows leftward as samples accumulate.

When a series carries more samples than the chart has columns to draw them, the canvas collapses
points together and fine detail (e.g. brief spikes) can be lost. The affected chart's title then
shows a red **⚠ … [+] zoom in** hint — zooming in with `+` shortens the window so the remaining
points fit and stay distinct.

### Logs

Alumet's own logs (written to stderr) would normally bleed over the UI; while the interactive UI is
active they are captured into a bounded ring buffer (`log_buffer_lines`, default 5000) and shown
without disturbing the display. Press `l` to cycle the logs through a small bottom **tail pane** (the
most recent lines), a **full-screen table**, and off.

Each captured line is parsed into **time · level · module · message** (from `env_logger`'s default
format; lines that do not parse, such as multi-line panics, are folded into the previous entry). The
full-screen table behaves like the measurements table: scroll with `↑`/`↓`/`PgUp`/`PgDn` (or
`Ctrl-D`/`Ctrl-U` for 10 rows, `g`/`G`
to jump to the top / bottom), filter live with `/` (case-insensitive regex, scoped with
`$time=`/`$level=`/`$module=`/`$message=`, e.g. `$level=ERROR` or `$module=rapl`), and sort with
`<`/`>` to move the column cursor then `s` to add it as a sort key (multi-column, same rules as the
measurements table). It defaults to newest-first, and the level is colored by severity. `Esc` returns
to the small pane.

The buffer keeps the most recent `log_buffer_lines` entries (the oldest are dropped), so memory stays
bounded — roughly a few hundred bytes per line, on the order of 1–2 MB at the default. Raise it for
deeper scrollback or lower it to save memory.
