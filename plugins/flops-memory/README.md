# FLOPS and memory-volume plugin

The `flops-memory` source plugin reports all metrics in the `flops` section
defined by LIKWID for the detected CPU architecture. It reads architecture,
raw-event, formula, and unit definitions from a metrics file bundled into the
plugin binary, reads counters through Linux `perf_events`, and evaluates the
formulas on every poll.

## Requirements

- Linux with `perf_events`, accessible hardware raw counters, and the same
  privileges/capabilities required by the [`perf` plugin](../perf/README.md).
- The LIKWID metrics file bundled into the plugin at build time.
- Cgroup observation assumes cgroup v2 is mounted at `/sys/fs/cgroup`, as the
  `perf` plugin does.
- Hosts with heterogeneous CPU models, steppings, or vendors are not supported;
  startup reports an error rather than selecting metrics for only one of them.

## Metrics

The plugin reports all metrics in LIKWID's `flops` section for the detected CPU
architecture. The architecture, raw-event, formula, and unit definitions are
bundled into the binary, so the plugin does not need to access a LIKWID checkout
or download metric data at runtime.

Every distinct raw event in the selected architecture's `flops` section is
opened, including events shared by several formulas. Repeated raw events are
measured once per target and mapped back to each metric's counter names.

Each raw event has its own perf group, allowing Linux to multiplex the events
when there are more requested events than available hardware counters. Putting
all events in a single group would require them to fit on the PMU simultaneously
and can prevent the entire group from running.

On each poll, all raw counters for a process (or each CPU of a cgroup) are read
before any formula is evaluated. Perf's multiplexing scaling is enabled.
Formulas use cumulative counter values since the source started, not
per-interval differences or rates. Multiplexed values are estimates, and the
independently scheduled counters are not an atomic snapshot. Each measurement
has an `accuracy` attribute. A metric combining several counters reports the
least accurate input.

## Configuration

Enable `flops-memory` in the Alumet configuration. Its default polling interval
is one second, and its default flush interval is five seconds:

```toml
[plugins.flops-memory]
poll_interval = "1s"
flush_interval = "5s"
add_source_in_pause_state = false
```

In `exec` mode, measurements follow the observed process. In `watch` mode, the
plugin follows observed cgroups, opening raw perf counters on each online CPU
for each cgroup. The plugin skips source setup when the CPU family, model, and
supported stepping conditions have no matching entry in the bundled metrics.

### Update LIKWID metrics

The repository includes generated LIKWID metrics, so no download or extraction
is needed at runtime. To refresh the bundled metrics, run this from the
repository root:

```bash
cargo run -p plugin-flops-memory --bin likwid_extractor
```

This writes `plugins/flops-memory/data/likwid_metrics.json`, which is embedded
when the plugin is built. The extractor needs Git and network access by
default; see
[`src/bin/README.md`](src/bin/README.md) for running it against a local LIKWID
checkout, selecting a branch, and using a different output path.

The extractor can be built and tested independently of perf hardware:

```bash
cargo test -p plugin-flops-memory
```
