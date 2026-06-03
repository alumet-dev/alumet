# TUI plugin — user stories

A catalogue of everything the `tui` plugin does, written as user stories with **testable acceptance
criteria**. Use it as a manual test script: each `- [ ]` is one thing to verify by hand.

The plugin runs an interactive, `htop`-style terminal UI over Alumet's measurements, plus real-time
graph tabs and a captured-logs view. It only shows when stdout is an interactive terminal.

**How to run it for testing.** Add the plugin to an agent config and run the agent in a real
terminal. A high-cardinality source (e.g. `procfs`) is the best stress test; a couple of low-rate
sources (e.g. `rapl`, a CPU source) are best for reading graphs and logs.

```toml
[plugins.tui]
stale_after_seconds = 30
graph_history_seconds = 120
log_buffer_lines = 5000
print_unit = true
use_unit_display_name = true
```

Legend: **MT** = measurements table · **G** = graph tab · **R** = raw-data table · **L** = logs.

---

## 1. Startup & lifecycle

- **US-1.1** — *As an operator, I want the UI to appear automatically when I run the agent in a
  terminal, so I can watch measurements live.*
  - [ ] Running the agent in an interactive terminal shows the TUI on an alternate screen.
  - [ ] On quit, the terminal is fully restored (no leftover raw mode, cursor visible, scrollback intact).

- **US-1.2** — *As an operator, I want the UI to stay out of the way when output is piped, so logs/files
  are not polluted with escape codes.*
  - [ ] Running with stdout redirected to a file/pipe shows **no** TUI; a warning line notes stdout is not a terminal.

- **US-1.3** — *As an operator, I want quitting the view to stop the agent, like `htop`.*
  - [ ] `q` exits the UI and the agent shuts down gracefully.
  - [ ] `Ctrl-C` does the same (even though raw mode normally swallows it).

- **US-1.4** — *As an operator, I want the agent's own shutdown to also close the UI.*
  - [ ] Stopping the agent externally (SIGINT) tears down the UI and restores the terminal.

---

## 2. Configuration

- **US-2.1** — *As an operator, I want stale series to disappear so the table stays bounded.*
  - [ ] A series that stops updating is removed after `stale_after_seconds`.
  - [ ] `stale_after_seconds = 0` keeps every series forever (verify a stopped series stays).

- **US-2.2** — *As an operator, I want to set the initial graph history window.*
  - [ ] `graph_history_seconds` sets how much history graphs start with.

- **US-2.3** — *As an operator, I want to bound the log buffer's memory.*
  - [ ] `log_buffer_lines` caps retained log lines; the oldest are dropped past the cap.
  - [ ] A small value (e.g. `50`) is observably limited when scrolling the logs table.

- **US-2.4** — *As an operator, I want to control unit display.*
  - [ ] `print_unit = false` hides the unit column/text.
  - [ ] `use_unit_display_name` toggles between display name (`J`) and unique name (`joule`).

- **US-2.5** — *As an operator, I want sane defaults if I omit keys.*
  - [ ] An empty `[plugins.tui]` (or omitted keys) uses the documented defaults.

---

## 3. Measurements table — viewing

- **US-3.1** — *As a user, I want to see the latest value of every series, regardless of which source
  flushed.*
  - [ ] Every active series appears as a row: metric, resource, consumer, value, unit, trend, updated, attributes.
  - [ ] Values update in place as new measurements arrive.

- **US-3.2** — *As a user, I want an at-a-glance trend per series without opening a graph.*
  - [ ] The `trend` column shows an inline sparkline of each series' recent values (newest on the right).
  - [ ] Each sparkline is autoscaled to its own range (a tiny and a huge series are both legible).
  - [ ] The sparkline is present even for series that are not graphed.

- **US-3.3** — *As a user, I want the footer to summarize the table state.*
  - [ ] Footer shows shown/total counts, marked count, the active sort, the filter, log mode, and key hints.

---

## 4. Measurements table — navigation

- **US-4.1** — *As a user, I want to move the selection.*
  - [ ] `↑` / `↓` move the selection by one row.
  - [ ] `PgUp` / `PgDn` move by a page.
  - [ ] `Ctrl-D` / `Ctrl-U` move the selection **10 rows** down / up.
  - [ ] `g` / `Home` jump to the top; `G` / `End` jump to the bottom.
  - [ ] The mouse wheel moves the selection.

- **US-4.2** — *As a user, I want the selection to track a series by identity, so it survives
  reordering/regrouping.*
  - [ ] After a re-sort or value change, the selection stays on the same series (not the same row index).

---

## 5. Filtering (MT, R, L)

- **US-5.1** — *As a user, I want to filter rows live with a regular expression.*
  - [ ] `/` opens the filter prompt; typing filters the table live; `Enter` applies (closes prompt, keeps filter); `Esc` clears it.
  - [ ] Filtering is case-insensitive; patterns like `cpu|ram`, `^rapl`, `core=\d+` work.

- **US-5.2** — *As a user, I don't want a half-typed regex to blank the table.*
  - [ ] An invalid/incomplete pattern is flagged (`invalid regex`) and leaves the view unfiltered rather than emptying it.

- **US-5.3** — *As a user, I want to scope the filter to one column.*
  - [ ] `$metric=…`, `$resource=…`, `$consumer=…`, `$attributes=…`, `$all=…` scope the regex to that column.
  - [ ] While typing the scope the table is **not** filtered yet (it waits for `=`).
  - [ ] In the prompt, a valid `$column` token is **bold gold**; an unknown one is **ember/red**; the `=` and pattern stay default-colored.
  - [ ] An unscoped pattern (no `$`) matches against all columns.

- **US-5.4** — *As a user, I want attribute filtering to be order-independent.*
  - [ ] `$attributes=core=2` matches a series carrying that attribute regardless of its position in the displayed attribute list.

---

## 6. Sorting (MT, R, L) — multi-column priority

- **US-6.1** — *As a user, I want a stable default order so rows don't reshuffle as values change.*
  - [ ] With no explicit sort, rows are ordered by identity (metric → resource → consumer → attributes, ascending) and **hold their position** as values update.
  - [ ] The footer shows `sort:default`.

- **US-6.2** — *As a user, I want to choose a sort column.*
  - [ ] `<` / `>` move the sort cursor across columns (highlighted/reversed in the header).
  - [ ] `s` adds the cursor column as a sort key.

- **US-6.3** — *As a user, I want true multi-column sort with predictable priority.*
  - [ ] Pressing `s` on a not-yet-sorted column **appends** it as the next-lowest priority (earlier columns keep their priority).
  - [ ] Pressing `s` again on that column **flips** its direction (▲/▼); a third press **drops** it.
  - [ ] The header shows ▲/▼ per sorted column, plus a small priority number (¹²³…) when more than one column is sorted.
  - [ ] The footer shows the primary column and `+N` for the remaining keys.

- **US-6.4** — *As a user, I want sorting by a live column to stay readable.*
  - [ ] Sorting by `value` keeps equal-valued rows in a stable order (identity is always the final tiebreak), so only rows whose value actually changes move.

---

## 7. Grouping / folding (MT)

- **US-7.1** — *As a user, I want to fold the table into groups.*
  - [ ] `1`/`2`/`3` set the **group** (outer) dimension: metric / consumer / resource.
  - [ ] `4`/`5`/`6` set the **subgroup** (inner) dimension nested in each group.
  - [ ] Each is a single choice: another key in the range switches it; the same key clears it.
  - [ ] `Esc` clears both grouping and subgrouping.

- **US-7.2** — *As a user, I want new groups to stay folded so the view stays compact.*
  - [ ] Groups start folded; groups appearing as new series stream in are also folded.
  - [ ] Groups I've already expanded stay expanded.

- **US-7.3** — *As a user, I want to expand/collapse nodes.*
  - [ ] `Enter` on a group header folds/unfolds that node.
  - [ ] `c` collapses all groups, or expands all if any is collapsed.
  - [ ] Group headers show `dimension=value (count)` and a `*` when any series under them is marked.

- **US-7.4** — *As a user, I want a huge table to auto-group once so I'm not buried in rows.*
  - [ ] When the table first grows past a few dozen series and I haven't grouped/filtered, it auto-groups by metric → consumer (folded), exactly once.

---

## 8. Marking & opening graphs (MT)

- **US-8.1** — *As a user, I want to mark series to graph several at once.*
  - [ ] `Space` marks/unmarks the selected series.
  - [ ] `Space` on a group header marks/unmarks every series under it (including folded-away ones).
  - [ ] `d` clears all marks; the footer marked-count reflects changes.

- **US-8.2** — *As a user, I want to open a graph.*
  - [ ] `Enter` on a leaf series opens a graph tab of it (when nothing is marked).
  - [ ] With marks, `Enter` opens one graph tab containing all marked series.
  - [ ] Re-opening the same exact set of series focuses the existing tab instead of duplicating it.

---

## 9. Graph tabs — charts

- **US-9.1** — *As a user, I want graphs to plot recent history and refresh live.*
  - [ ] A graph tab plots each series' recent values and updates in real time.
  - [ ] The time axis spans the full history window (line starts at the right edge, grows leftward as history fills).

- **US-9.2** — *As a user, I want to control how series are split into charts.*
  - [ ] `1`–`8` choose the facet key; the footer shows the mode (e.g. `group:name/resource`).
  - [ ] The metric name is always part of the key (two metrics never share a Y axis).
  - [ ] Series differing only in the *remaining* dimensions overlay as separate lines with a legend.

- **US-9.3** — *As a user, I want busy charts to stay legible.*
  - [ ] A chart shows at most 12 lines (the highest current values); the rest are noted as `+N hidden` in the title.

- **US-9.4** — *As a user, I want stable, distinct line colors and legends.*
  - [ ] Each line in a chart has a **unique** color.
  - [ ] A line keeps its color across frames (no flickering/recoloring over time).
  - [ ] The legend order is stable (doesn't shuffle frame to frame).
  - [ ] The legend is visible even with many charts/series.

- **US-9.5** — *As a user, I want the Y axis to make sense at zero.*
  - [ ] When data is non-negative and at/near 0, the Y axis anchors at 0 (no confusing negative tick).

- **US-9.6** — *As a user, I want to know when detail is being lost.*
  - [ ] When there are more samples than chart columns, the title shows a red `⚠ … [+] zoom in` hint; zooming in with `+` makes points distinct again.

- **US-9.7** — *As a user, I want to focus and full-screen a chart.*
  - [ ] With several charts, one is focused (bright border); `Tab` / `Shift-Tab` move focus.
  - [ ] `f` full-screens the focused chart; in full-screen the title notes position (e.g. `[2/5]`); `Tab` still cycles which chart is shown.
  - [ ] `f` again, or `Esc`, returns to the grid.

- **US-9.8** — *As a user, I want to zoom the history window live.*
  - [ ] `+` zooms in (shorter window, more detail); `-` zooms out; the footer shows the current window in seconds.

- **US-9.9** — *As a user, I want to close a graph tab.*
  - [ ] `Esc` (when not full-screen and not in the raw table) closes the graph tab.

---

## 10. Graph tabs — raw-data table (R)

- **US-10.1** — *As a user, I want to inspect the raw samples behind a graph for debugging.*
  - [ ] `r` toggles a raw-data table: one row per stored sample — time (UTC, ms), metric, value, resource, consumer, attributes.
  - [ ] `r` or `Esc` returns to the charts.

- **US-10.2** — *As a user, I want the raw table to work while paused.*
  - [ ] With `p` (paused), the raw table reads the same frozen snapshot as the charts.

- **US-10.3** — *As a user, I want to scroll, sort and filter the raw table.*
  - [ ] Scroll with `↑`/`↓`/`PgUp`/`PgDn`, `Ctrl-D`/`Ctrl-U` (10 rows), `g`/`G` (top/bottom).
  - [ ] `/` filters live (regex + `$col=` scoping, same rules as the measurements table).
  - [ ] `<`/`>` + `s` give multi-column sort (same priority behavior); default is newest-first (time ▼).
  - [ ] `+`/`-` widen/narrow the history window so the table shows more/fewer samples.

---

## 11. Logs (L)

- **US-11.1** — *As an operator, I want Alumet's logs captured instead of bleeding over the UI.*
  - [ ] stderr log lines do not corrupt the display; they appear in the log view instead.

- **US-11.2** — *As an operator, I want to cycle log visibility.*
  - [ ] `l` cycles: small tail pane → full-screen table → off → (back to pane).
  - [ ] The footer reflects the current log mode.

- **US-11.3** — *As an operator, I want a quick tail of recent logs.*
  - [ ] The small pane shows the most recent lines as a simple tail (`HH:MM:SS LEVEL message`).

- **US-11.4** — *As an operator, I want a full logs table to retrieve old entries.*
  - [ ] The full table has columns **time · level · module · message**.
  - [ ] Levels are color-coded by severity (error ember, warn orange, info plain, debug/trace dim).
  - [ ] Scroll with `↑`/`↓`/`PgUp`/`PgDn`, `Ctrl-D`/`Ctrl-U` (10 rows), `g`/`G` (top/bottom).
  - [ ] `Esc` returns to the small pane.

- **US-11.5** — *As an operator, I want to filter and sort logs.*
  - [ ] `/` filters live (regex); `$time=`, `$level=`, `$module=`, `$message=` (and `$all=`) scope it — e.g. `$level=ERROR`, `$module=rapl`.
  - [ ] `<`/`>` + `s` give multi-column sort over time/level/module/message; default is newest-first.

- **US-11.6** — *As an operator, I want multi-line log messages kept intact.*
  - [ ] A multi-line message (e.g. a panic/backtrace) is folded into a single entry rather than scattered across timestamp-less rows.

- **US-11.7** — *As an operator, I want the log buffer bounded.*
  - [ ] With many log lines, only the most recent `log_buffer_lines` are retained (older ones drop off the top when sorted oldest-first).

---

## 12. Pause

- **US-12.1** — *As a user, I want to freeze the view to read it.*
  - [ ] `p` freezes the table/charts on their current values; the footer turns red and shows `PAUSED`.
  - [ ] `p` again resumes live updates.
  - [ ] Pause also freezes graphs and the raw table (snapshot consistency).

---

## 13. Tabs & overlays

- **US-13.1** — *As a user, I want to move between tabs.*
  - [ ] `←` / `→` switch tabs; tab `0` is always the measurements table; graph tabs follow.
  - [ ] The active tab is highlighted (black-on-gold); idle tabs are muted.

- **US-13.2** — *As a user, I want context-sensitive help.*
  - [ ] `h` toggles a keybindings overlay tailored to the current view (measurements / graph / logs).
  - [ ] `h` or `Esc` closes it; while open, only its close keys act (and `Ctrl-C` still quits).

- **US-13.3** — *As a user, I want an about screen with versions.*
  - [ ] `?` shows the Alumet logo, the **Alumet framework version** (prominent), and the **TUI plugin version** (secondary).
  - [ ] `?` or `Esc` closes it.

---

## 14. Branding / theme

- **US-14.1** — *As a user, I want the UI to feel like part of Alumet.*
  - [ ] Colors are drawn from the Alumet logo (flame gold/orange/ember + circuit cyan) over a dark base.
  - [ ] Cyan marks live data/interaction (sparklines, sort arrows, selection, focus); gold carries the brand and marks; ember flags alerts and the paused state.
  - [ ] A small Alumet brand chip (flame tip + gradient wordmark) is pinned to the left of the tab bar in every view.
  - [ ] The full logo appears in the about overlay.

---

## 15. Cross-cutting / regression checks

- [ ] Filter, sort and grouping interact sanely (e.g. filter hides rows without breaking the sort or selection).
- [ ] High-cardinality source (e.g. procfs, thousands of series): the UI stays responsive and readable.
- [ ] Switching between tabs preserves each tab's state (filter/sort/scroll/focus).
- [ ] Resizing the terminal reflows the layout without panicking.
- [ ] No log lines or stderr leak onto the screen during normal operation.
