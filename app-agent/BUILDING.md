# How to build

Run:

```sh
cargo build
```

## Development builds vs Official releases

Development builds embed additional build-time information in the agent, such as the (short) commit hash, the version of the Rust compiler and the date of the build. It allows to make the difference between multiple non-release versions of the agent without changing its version number in `Cargo.toml`. Development builds are the default option.

When building an official release of the standard Agent, the CI sets the environment variable `ALUMET_AGENT_RELEASE=true`, which makes the agent return simpler version information.

This is implemented in `build.rs` and `src/bin/main.rs`.
