# Alumet integration tests

This folder contains all script files and scenarios for testing alumet on bare metal.

## Prerequisites

Before running the tests, ensure you have:

- Python **>= 3.12**
- Poetry **>= 2.1**

## Run the tests

Before executing the tests, you need to install robot framework.
To do so, you can run the following command, which will install robotframework and the required dependencies.

```bash
make init
```

You should now be able to run the robotframework test scenarios.

```bash
make test
```

## Validate the robot framework files

The following tools: [robocop](https://robocop.dev/stable/)
and [robotunused](https://github.com/Lakitna/robotframework-find-unused) will be used
in order to check the lint and format of your robot files.

You can run the following commands:

```bash
# Lint, using: robocop check
make lint

# Check the format, using: robocop format --check
make format-check

# Search for any unused object in your robot tests
make unused

# Run every checks
make check
```

These commands must pass before pushing your code.

You can format your code with:

```bash
# Format, using: robocop format
make format
```
