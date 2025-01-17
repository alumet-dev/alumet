# Contributing guide

Hello and thank you for your interest in contributing to the Alumet project!

Here is what you need to know.

## The repositories

### Main repository (the one you are in)

This repository is divided in several parts:
- The `alumet` crate contains the core of the measurement tool, as a Rust library.
- Binaries can be created from this library, in order to provide a runnable measurement software. The official binaries that we provide are defined in `app-agent`. Agents always depend on `alumet`.
- Plugins are defined in separate folders: `plugin-nvidia`, `plugin-rapl`, etc. Plugins always depend on `alumet`.
- Two more crates, `alumet-api-dynamic` and `alumet-api-macros`, ease the creation of dynamic plugins written in Rust. This is WIP (work in progress) = not finished yet.
- `test-dynamic-plugins` only exists for testing purposes.

### Other repositories

The [`alumet-dev` organization](https://github.com/alumet-dev) contains additional repositories for the website and the packaging of the tool. You'll find more information in each repository.

## What you can do

There are several categories of tasks that can help the Alumet project. You don't necessarily need to code in Rust!

### Report issues

If you find a bug in the Alumet library or in one of the official agents, you should [open an issue on the `alumet` repository](https://github.com/alumet-dev/alumet/issues). Please use the search function to make sure that the bug you have found has not already been reported. For questions that are not bugs, or if you are not sure whether something is a bug or not, you can [open a discussion](https://github.com/alumet-dev/alumet/discussions).

If you find a mistake or confusing point in the user book or in the developer book, you should open an issue on the `user-book` or `developer-book` repository.

### Write documentation

Writing documentation or tutorials that show how to use Alumet, in the [user book](https://github.com/alumet-dev/user-book), is very helpful to the project

If you have a good understanding of Alumet internals, you can also explain how to write plugins and how to  contribute to Alumet, in the [developer book](https://github.com/alumet-dev/developer-book).

### Code 🧑‍💻

Using the open issues on GitHub, you can find something to work on. You should choose an issue that is not already assigned to someone. If unsure, feel free to ask in a comment or in a new discussion.

If you are an external contributor, it works as follows:
1. Find something to work on using the [issues](https://github.com/alumet-dev/alumet/issues) or the [discussions](https://github.com/alumet-dev/alumet/discussions).
2. Fork the alumet repository.
3. Create a new git branch to work on.
4. On this branch, implement the fix or feature that you'd like Alumet to have.
5. Document new functions and types. Write [unit tests](https://doc.rust-lang.org/rust-by-example/testing/unit_testing.html) and/or integration tests. Run the tests with `cargo test`.
6. Format your code by running `cargo fmt` in the project directory. We provide a `.rustfmt.toml` that will be automatically used by the formatting tool.
7. When you are ready, submit your work by opening a [Pull Request (PR)](https://github.com/alumet-dev/alumet/pulls).

If your goal is to optimize somethings, please run benchmarks and provide [flame graphs](https://github.com/killercup/cargo-flamegraph) or other metrics to show your improvements. For micro-benchmarks, we recommend the tool [Criterion](https://bheisler.github.io/criterion.rs/book/index.html).

## Rust good practices

### Linting

You should use [Clippy](https://doc.rust-lang.org/stable/clippy/index.html) to lint your code. The workspace `Cargo.toml` defines some lint rules, that must apply to every crate in the Alumet repository.

Manual action required: if you add a crate (like a plugin) to the repository, add the following two lines to your `Cargo.toml`:

```toml
[lints]
workspace = true
```

### Dependencies

When you add a new plugin to the repository, make it depend on the `alumet` crate in a relative way, without specifying a version. That is, the `dependencies` section of its `Cargo.toml` should look like this:

```toml
[dependencies]
alumet = { path = "../alumet" }
```

### Basic tips

- For efficiency, avoid too much cloning. It's fine for a PoC but should be optimized before merging the PR.
- Use `anyhow` and `thiserror` to simplify error management. Alumet already uses those.
- Use [`log`](https://docs.rs/log/latest/log/) to log messages, not `println`. Example:

```rs
let value = ();
log::debug!("My value is: {value:?}");
```

### Advanced tips

- Alumet internally uses Tokio. To contribute to the core of Alumet, please first follow the [Tokio tutorial](https://tokio.rs/tokio/tutorial). This is not necessary for most plugins.
- If you get weird errors with `async` code, such as `higher-ranked lifetime error`, try to decompose your operation in several functions and variables, and write down the types of each step explicitly. This will help the compiler to figure out the types of the futures.
