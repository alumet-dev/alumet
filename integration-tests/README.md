# Alumet integration tests <!-- omit in toc -->

This folder contains all script files and scenarios for testing alumet on bare metal.
The structure of files is:

```text

├── README.md
├── run.sh
└── scenarios
    ├── common
    │   └── alumet_keywords.robot
    ├── __init__.robot
    ├── installation.robot
    ├── plugin-perf.robot
    ├── plugin-rapl.robot
    ├── resources
    │   ├── help-config-option.txt
    │   ├── help-exec-option.txt
    │   ├── help-option.txt
    │   ├── help-plugins-option.txt
    │   └── help-watch-option.txt
    └── tools
        └── cpu_load.sh
```

The scenarios (test suites) are located in scenarios folder. Robot framework is used to perform automatically the tests. Each suite test is a set of tests.
The test strategy is to write one robot framework file (a test suite) per alumet plugin and/or per features.

The current version contains the following robot framework files:
- installation.robot: for testing installation of alumet (uninstallation is tested using the Suite Teardown)
- plugin-perf.robot: for testing perf plugin
- plugin-rapl.robot: for testing rapl plugin.

The \_\_init\_\_.robot file contains the Suite Setup (Install Alumet) and Teardown (UnInstall Alumet). When this file is present,  robot framework executes at the beginning of the tests the keyword `Install Alumet` and at the end of test execution the keyword `UnInstall alumet`

Each test can have one or several tags allowing the exclusion of tests from being run by robot framework.

Before executing the tests, you need to install [robot framework](https://docs.robotframework.org/docs/getting_started/testing#install-robot-framework-in-a-virtual-environment).
Then you need to initialize the robot framework virtual environment:

```bash
source ~/venv-robot/bin/activate
(venv-robot) [zychlae@carbon0 integration_tests]$
```

The script run.sh allows to execute the robot framework tests on a target node and with a target alumet's release and distribution. For that, you can modify the variables in run.sh script:

```bash
# credentials used to logon on the target node
NODE=otpaas2
USERNAME=emmanuel
KEY=${HOME}/.ssh/id_rsa
HOME_TEST=$(pwd)
# version of Alumet to installed
ALUMET_VERSION=0.9.4
ALUMET_DISTRIBUTION=1_amd64_ubuntu_22.04
```

Then you can execute your test:

```bash
./run.sh
Start running tests at: Thu Jun  4 17:06:14 CEST 2026
==============================================================================
Scenarios
==============================================================================
Scenarios.Installation :: Alumet installation / uninstallation
==============================================================================
Test connection on target node                                        | PASS |
------------------------------------------------------------------------------
install alumet                                                        | PASS |
------------------------------------------------------------------------------
which alumet-agent                                                    | PASS |
------------------------------------------------------------------------------
[...]
------------------------------------------------------------------------------
Scenarios.Installation :: Alumet installation / uninstallation        | FAIL |
9 tests, 8 passed, 1 failed
==============================================================================
Scenarios.Plugin-Perf :: Alumet test plugin perf
==============================================================================
Test connection on target node                                        | PASS |
------------------------------------------------------------------------------
run cpu_load                                                          | PASS |
------------------------------------------------------------------------------
run plugin csv perf                                                   | PASS |
------------------------------------------------------------------------------
check alumet running                                                  | PASS |
------------------------------------------------------------------------------
Check Perf Metric perf_hardware_REF_CPU_CYCLES                        metric value read: 17688
Check Perf Metric perf_hardware_REF_CPU_CYCLES                        | PASS |
------------------------------------------------------------------------------
[...]
------------------------------------------------------------------------------
Scenarios.Plugin-Perf :: Alumet test plugin perf                      | FAIL |
9 tests, 8 passed, 1 failed
==============================================================================
Scenarios.Plugin-Rapl :: Alumet test plugin rapl
==============================================================================
Test connection on target node                                        | PASS |
------------------------------------------------------------------------------
run plugins socket-control csv rapl                                   | PASS |
------------------------------------------------------------------------------
check alumet running                                                  | PASS |
------------------------------------------------------------------------------
Check Rapl Metric package                                             metric value read: 22.81256103515625
Check Rapl Metric package                                             | PASS |
------------------------------------------------------------------------------
[...]
------------------------------------------------------------------------------
Scenarios.Plugin-Rapl :: Alumet test plugin rapl                      | PASS |
9 tests, 9 passed, 0 failed
==============================================================================
Scenarios                                                             | FAIL |
27 tests, 25 passed, 2 failed
```

Robot framework generates a report and a log file in html format.

To write a new test suite for an input plugin, you must use the `Check Metric` keyword to validate the csv output file.
You use plugin-rapl.robot as an example.
