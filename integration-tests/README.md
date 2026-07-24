# Alumet integration tests

This folder contains all script files and scenarios for testing alumet.
Below, the structure of this folder.

```text

├── Makefile
├── output
│   ├── log.html
│   ├── output.xml
│   └── report.html
├── pyproject.toml
├── README.md
└── scenarios
    ├── baremetal
    │   ├── __init__.robot
    │   ├── installation.robot
    │   ├── plugin-perf.robot
    │   └── plugin-rapl.robot
    ├── common
    ├── resources
    │   ├── alumet_keywords.resource
    │   ├── help-config-option.txt
    │   ├── help-exec-option.txt
    │   ├── help-option.txt
    │   ├── help-plugins-option.txt
    │   └── help-watch-option.txt
    └── tools
        └── cpu_load.sh
```

`scenarios` folder contains all robotframework files.
One folder per type of test, for example we have `baremetal` folder regarding the test of alumet installed in native mode. For future test, we should have a `container` folder for example.

`resources` folder is for keywords that are used by several test suite.

`tools` folder contains all the tools used by the test scenarios.

If you need to updated the robocop rules, you must update the `pyproject.toml` file (section `tool.robocop.lint`).

## Prerequisites

Before running the tests, ensure you have:

- Python **>= 3.12**
- Poetry **>= 2.1**

## installing robot framework

Before executing the tests, you need to install robot framework.
To do so, you can run the following command, which will install robot framework and the required dependencies.

```bash
make init
```

## Run the tests

The \_\_init\_\_.robot file contains the Suite Setup (Install Alumet) and Teardown (UnInstall Alumet). When this file is present,  robot framework executes at the beginning of the tests the keyword `Install Alumet` and at the end of test execution the keyword `UnInstall alumet`
Each test can have one or several tags allowing the exclusion of tests from being run by robot framework.

To run the robot framework test scenarios execute the command:

```bash
make test
```

The test report is written in `output/report.html` file.

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

These commands must pass before pushing your code (commands executed by the CI).

You can format your code with:

```bash
# Format, using: robocop format
make format
```
