# LIKWID metrics extractor

The [`likwid_extractor`](src/bin/likwid_extractor.rs) binary extracts FLOPS and
memory-volume metric definitions from the LIKWID source tree into JSON. It does
not measure hardware counters or require a LIKWID installation.

## Requirements

- Rust and Cargo compatible with the workspace's Rust 2024 edition.
- Git available on `PATH`.
- Network access to fetch Rust dependencies and clone LIKWID when using a remote
  repository.

## Running

Run these commands from the Alumet repository root:

```bash
cargo run -p flops-memory --bin likwid_extractor
```

By default, the extractor clones LIKWID's `master` branch and writes
[`data/likwid_metrics.json`](data/likwid_metrics.json). An existing output file
is replaced atomically; missing parent directories are created.

Pass extractor arguments after Cargo's `--` separator:

```bash
cargo run -p flops-memory --bin likwid_extractor -- \
  --branch master \
  --output ./likwid_metrics.json \
  --verbose
```

To build and run the binary directly:

```bash
cargo build -p flops-memory --bin likwid_extractor
./target/debug/likwid_extractor --help
```

The binary path above assumes Cargo's default target directory.

## Options

| Option | Default | Description |
| --- | --- | --- |
| `--repo <REPO>` | `https://github.com/RRZE-HPC/likwid.git` | Git repository to clone. A local Git repository path is also accepted. |
| `--branch <BRANCH>` | `master` | Branch or tag to retrieve using a shallow clone. |
| `--output <OUTPUT>` | This package's `data/likwid_metrics.json` | Destination JSON file. The default is based on the package directory at compile time, not the current working directory. Explicit relative paths are resolved from the current working directory. |
| `--keep-workdir` | Disabled | Preserve the temporary source checkout instead of deleting it. Its location is logged after a successful extraction. |
| `--verbose` | Disabled | Enable debug-level logging in addition to the usual informational messages and warnings. |
| `-h`, `--help` | N/A | Display usage information and exit without cloning or extracting. |

For example, to extract from an existing local Git repository:

```bash
cargo run -p flops-memory --bin likwid_extractor -- \
  --repo /path/to/likwid \
  --branch master \
  --output ./local-metrics.json \
  --keep-workdir
```

The local repository is still cloned; uncommitted changes are not included.
Choose a branch or tag that exists in that repository.

## Extraction and output

The extractor reads:

- `groups/` for FLOPS and memory-volume groups.
- `src/includes/topology.h` for CPU constants.
- `src/topology.c` for architecture names, CPU models, and stepping conditions.
- `src/perfmon.c` for event-table selection, including shared tables.
- `src/includes/perfmon_*_events.txt` for event and unit-mask definitions.

Architecture and event-table names are derived from LIKWID source rather than a
static alias table. For each architecture, event-table lookup uses the first
supported CPU model in the parsed mapping.

The JSON contains:

- `source`: repository, branch or tag, commit, groups path, and generation time.
- `architectures`: records grouped by numeric CPU-family code, containing
  architecture definitions and the `flops` and `memory_volume` metric lists.

Each metric includes its name, normalized units (`FLOP` or `Bytes`), formula, and
referenced counters encoded as raw events such as `r01c7`.

Warnings do not necessarily stop extraction. Unresolved event dependencies are
represented by `"not found"` in `events` and listed in
`unresolved_dependencies`; that field is omitted when empty. Architecture groups
without a usable topology mapping are skipped. Review warnings before using the
generated metrics. Fatal errors are logged and produce a nonzero exit status.

## Tests

The offline unit tests live in
[`tests/bin/likwid_extractor.rs`](tests/bin/likwid_extractor.rs) and use temporary
fixtures, without cloning LIKWID or accessing hardware.

```bash
cargo test -p flops-memory --bin likwid_extractor
```

To run one test:

```bash
cargo test -p flops-memory --bin likwid_extractor -- \
  tests::derives_tables_from_topology_models_and_perfmon_selectors --exact
```

To lint the binary and tests:

```bash
cargo clippy -p flops-memory --bin likwid_extractor --tests -- -D warnings
```
