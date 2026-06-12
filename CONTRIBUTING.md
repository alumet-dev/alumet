# Contribution Guidelines

Thank you for your interest in contributing to Alumet!

Alumet is a collaborative project.
Questions, suggestions, bug reports and pull requests are welcome.

To improve the experience of everyone (users, casual contributors, maintainers), please follow these guidelines.

## How to ask a question

Feel free to [open a discussion on GitHub](https://github.com/alumet-dev/alumet/discussions).

## How to report a bug

1. Try to [find an issue](https://github.com/alumet-dev/alumet/issues?q=is%3Aissue) that already exists for your problem.
2. If no one has reported your problem, open a [new issue](https://github.com/alumet-dev/alumet/issues). If you're unsure, open a [discussion](https://github.com/alumet-dev/alumet/discussions).
We will convert it into an issue if needed.

## How to contribute to the project

### Shared Values

Alumet is a free and open source software built by a variety of contributors.
Together, we try to:

- **Build something useful**. The _raison d'être_ of Alumet is to make a reliable, versatile and efficient measurement/monitoring tool that can be controlled by its users.
- **Foster human collaboration**. Human relationships matter as much as technical considerations. Be respectful, learn new things and help others grow.
- **Keep the project maintainable**. We are building for the long term, therefore we choose quality over quantity.

Please keep these values in mind when contributing.

### Using Git

#### General Workflow

1. [Fork](https://github.com/alumet-dev/alumet/fork) this repository.
1. Clone your fork.
1. Create a new branch. For instance `feat/plugins/csv/colored-output`.
1. Work on something.
    - When writing tests, don't modify the code that you test unless it's necessary.
    - When developing new features, try to write unit and/or integration tests.
    - Respect the coding style (see [code](#code)).
1. **Test** and **format** the code that you have touched (see [useful commands](#useful-commands)).
1. Commit your work. Each commit should represent a unit of work with a clear goal.
1. Push to your fork.
1. Open a Pull Request (PR).

Please help the reviewers: **make small PRs** (10-100 modified lines, if possible) 🙂.

It is recommended to add `alumet-dev/alumet`, the main repository of Alumet, as a remote.
Usually, we call it "upstream".

```sh
git remote add upstream git@github.com:alumet-dev/alumet.git
```

#### Commit Messages

Commits messages should look like this:

```txt
type(scope): description

optional details
```

Examples:

```txt
feat(plugins/csv): support colored output
```

```txt
docs(plugins/csv): add missing description of config option abc
```

```txt
ci(lint): update cspell version
```

We mostly follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/), with two exceptions:
1. **The scope is not optional**, except for `ci` and `chore` types.
2. We do not tie commits to semantic versioning, because breaking changes can be hard to detect. We prefer to rely on automatic tools that check whether there are breaking changes.

List of common types: feat, fix, test, refactor, docs, ci, chore.

The scope does not correspond to the Rust package, but adhere to the following format:

```txt
area/sub-area/more-details
```

- Levels (separated by /) can be added or removed at your option.
- For plugins, use `plugins/name`, without prefixing the plugin's name with `plugin-` (see the example above).
- List of common areas: core, plugins, agent

**Each commit should have only one scope**.
If your commit needs more than one scope, you should break it down into multiple, smaller commits.

#### Rebasing

When you fork Alumet, you get a copy of the repository in its current state.
You create a new branch and push commits.
Your git history looks like this:

```txt
main          A---B
                   \
feat/xyz            E---F
```

Before the PR gets merged, new commits may appear upstream.

```txt
upstream/main A---B---C---D
main          A---B
                   \
feat/xyz            E---F
```

In this situation, you need to update your local copy of the `main` branch and rebase your `feat/xyz` branch on it.
One way to do it is:

```sh
git fetch upstream main:main
git rebase main
```

This will update the history like this:

```txt
upstream/main A---B---C---D
main          A---B---C---D   (local copy updated)
                           \
feat/xyz                    E---F   (branch rebased)
```

If the PR gets approved by the projet maintainers, it will be merged into upstream.
A merge commit `M` will be created, with some information about the PR: title, description, etc.

```txt
upstream/main A---B---C---D---M
                               \
feat/xyz                        E---F
```

While rebasing is a good practice that you should follow, maintainers might merge a PR from a branch that is "running late" if there are no conflicts.

### Code

Read the [Alumet Developer Book][dev-book] to learn how to develop plugins, build custom Alumet-based agents and contribute to the core.

#### Repo Overview

The `alumet` repository is a [Cargo workspace](https://doc.rust-lang.org/cargo/reference/workspaces.html).
It contains multiple Rust _packages_. To ease the navigation, they are grouped in multiple directories.

```txt
├── agent/  -- the standard Alumet agent (executable app)
├── core/   -- the "engine" of Alumet
├── plugins/  -- official Alumet plugins
└── separate-tests/ -- additional integration tests
```

#### Style

TL;DR:
- Use the type system to your advantage. Make illegal states unrepresentable.
- Write comments that explain the _why_, not the _how_. Add [documentation](https://doc.rust-lang.org/rustdoc/how-to-write-documentation.html) to modules, types and functions.
- Use [anyhow](https://crates.io/crates/anyhow) (and call `.context` / `.with_context` for error messages) and [thiserror](https://crates.io/crates/thiserror).
- When working on a plugin, let the Alumet framework guide you. If your code becomes super complicated, there might be a better way. Don't hesitate to (re)read the docs and to ask for help.
- When working on the core, follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/about.html). Be careful of breaking changes. Strive to achieve great performance.

Refer to the [developer book][dev-book] for more information.

#### Useful Commands

To compile a specific package, run (the package name comes from its `Cargo.toml` file):

```sh
cargo build -p the_package
```

For instance, to build the app:

```sh
cargo build -p alumet-agent
```

To run tests for a specific package:

```sh
cargo test -p the_package
```

Always format your code:

```sh
cargo fmt
```

Lint your code with Clippy:

```sh
cargo clippy --no-deps -p the_package
```

### Documentation

There are different kinds of documentation:

- The code itself is documented. See [How to write documentation - The rustdoc book](https://doc.rust-lang.org/rustdoc/how-to-write-documentation.html).
- Each plugin has a README.md that briefly describes its purpose and options.
  - Use the templates from `readme/*_README_TEMPLATE.md`
  - Keep the README lightweight: no tutorial, no screenshot.
- The [Alumet User Book][user-book] is a guide for users.
  - Contribute in the dedicated repository.
  - Sync plugins' pages with their README.
  - Add more detailed explanations, tutorials, diagrams, etc.
- The [Alumet Developer Book][dev-book] is a guide for developers.
  - Contribute in the dedicated repository.
  - Add technical explanations, tutorials, diagrams, etc.
  - Since the dev book is more technical, it's not easy for a newcomer to contribute to it. Ask the maintainers via a [discussion](https://github.com/alumet-dev/alumet/discussions) to plan improvements to the dev book.

## LLM policy

Note: in this section, we use "LLMs" to mean "Large Language Models _and_ AI agents based on Large Language Models".

On top of ethical and environmental considerations, LLMs introduce some risks for open-source projects:
- It becomes easy to generate "slop" spam, which takes precious time from the maintainers.
- You can be tempted to rely on them for everything, thus losing your skills over time.
- They can act as a band-aid for poor abstraction and poor documentation.

Because we value **quality** over quantity, and **human collaboration** over bots, we have agreed on the rules below.

- ✅ LLMs **may** be used to ask questions and research information. However, please **check the documentation first** (in the code, in the [User Book][user-book] and in the [Developer Book][dev-book]). If the documentation is unclear, please open an issue on the relevant repository. Also, be aware that LLMs can make mistakes.
- ✅ You **may** use LLMs to generate private exercises and small proofs of concept in order to help you learn by experimenting. Then, using what you have learned, you can implement your contribution on your own.
- ❌ LLMs **must not** be used to generate discussions, comments, issues and PR descriptions. Using an LLM-based tool as a redaction assistant is OK, but the content must come from a human.
- ❌ LLMs **are not a substitute for thought**. You are responsible for the contributions that you offer to the project. **You must understand every modification**, every line of code.
- ❌ LLMs **are not authors** according to European regulations. Therefore, never use `Co-authored-by: <LLM>` in your commits.
- ❌ LLMs **must not** be used to plagiarise existing projects, nor to reinvent the wheel. You should always look for an existing library before implementing a large "generic" feature, such as an HTTP client.
- ❌ LLMs **must not** be used to write major features on their own. You must always control the architecture and the implementation.
- ⚠️ You **may** use LLMs to generate "boilerplate" code. As always, you must review the code before submitting it. Note that a huge amount of boilerplate is often a sign that something could be improved. Consider refactoring your code to remove duplicates, using a library that does what you want, or leveraging a deterministic tool that generates code.
- ⚠️ Using LLMs for the following tasks must be **disclosed in the description** of the PR/issue/discussion: content generation of any kind, extensive search that impact your contribution.

By participating in the Alumet project, you agree to comply with the current version of this policy.

This policy has been inspired by the [Rust LLM Usage Policy](https://github.com/jyn514/rust-forge/blob/llm-policy/src/policies/llm-usage.md), [Typst Contributing Guide](https://github.com/typst/typst/blob/main/CONTRIBUTING.md#how-to-land-a-contribution) and [Matplotlib Contributing Guide](https://matplotlib.org/devdocs/devel/contribute.html#use-of-generative-ai).

## Moderation

Failure to follow the contribution guidelines may result in the rejection of your PR, or any other action the maintainers deem necessary.

We expect you to act in good faith and to apply the [values of the project](#shared-values).
Let's build great things together!

[dev-book]: https://alumet-dev.github.io/developer-book/
[user-book]: https://alumet-dev.github.io/user-book/
