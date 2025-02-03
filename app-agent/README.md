# Alumet agents

This crate contains the alumet agents: binary crates (i.e. runnable applications) that are able to monitor and/or profile things on a system. It depends on the core of Alumet, plus a set of static plugins.

NOTE: this crate contains _multiple_ agents. Choose which one to run with the `--bin` cargo flag. Each agent can also require certain features to be enabled (see below).

## Local agent

Here is how to quickly run the local agent.

```sh
cargo run --bin alumet-local-agent --features local_x86
```

Use `cargo build --bin alumet-local-agent --features local_x86 --target-dir .` to build an executable without running it (it will be created under a "debug" folder).

For production, use add the option `--release` (it will be created under a "release" folder).

## Relay client agent

```sh
cargo run --bin alumet-relay-client --features relay_client
```

## Relay server agent

```sh
cargo run --bin alumet-relay-server --features relay_server
```
