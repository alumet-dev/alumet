# Alumet integration tests <!-- omit in toc -->

This folder contains all script files and scenarios for testing alumet on bare metal.
The structure of files is:

```bash

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

The __init__.robot file contains the Suite Setup (Install Alumet) and Teardown (UnInstall Alumet). When this file is present,  robot framework executes at the beginning of the tests the keyword _Install Alumet_ and at the end of test execution the keyword _UnInstall alumet_

Each test can have one or several tags allowing to exclude tests to during the execution with robot framework.

Before executing the tests, you need to install robot framework.
After you need to initialize the robot framework environment:

```bash
 source ~/venv-robot/bin/activate
(venv-robot) [zychlae@carbon0 integration_tests]$
```

A script run.sh allows to execute the robot framework tests on a target node and with a target alumet release and distribution. For that, you can modify the variables in run.sh script:

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
help option                                                           | PASS |
------------------------------------------------------------------------------
help exec option                                                      | PASS |
------------------------------------------------------------------------------
help plugins option                                                   | PASS |
------------------------------------------------------------------------------
help watch option                                                     | PASS |
------------------------------------------------------------------------------
help config option                                                    | PASS |
------------------------------------------------------------------------------
config regen                                                          | FAIL |
'' does not contain 'Default configuration file written to: /etc/alumet/alumet-config.toml'
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
Check Perf Metric perf_hardware_CACHE_MISSES                          metric value read: 0
Check Perf Metric perf_hardware_CACHE_MISSES                          | FAIL |
'0 !=0.0' should be true.
------------------------------------------------------------------------------
Check Perf Metric perf_hardware_BRANCH_MISSES                         metric value read: 26
Check Perf Metric perf_hardware_BRANCH_MISSES                         | PASS |
------------------------------------------------------------------------------
stop alumet                                                           | PASS |
------------------------------------------------------------------------------
Check alumet not running                                              | PASS |
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
Check Rapl Metric package_total                                       metric value read: 43.15618896484375
Check Rapl Metric package_total                                       | PASS |
------------------------------------------------------------------------------
Check Rapl Metric dram                                                metric value read: 2.7979583740234375
Check Rapl Metric dram                                                | PASS |
------------------------------------------------------------------------------
Check Rapl Metric dram_total                                          metric value read: 5.789459228515625
Check Rapl Metric dram_total                                          | PASS |
------------------------------------------------------------------------------
stop alumet                                                           | PASS |
------------------------------------------------------------------------------
Check alumet not running                                              | PASS |
------------------------------------------------------------------------------
Scenarios.Plugin-Rapl :: Alumet test plugin rapl                      | PASS |
9 tests, 9 passed, 0 failed
==============================================================================
Scenarios                                                             | FAIL |
27 tests, 25 passed, 2 failed
```

The robot framework generates a report and log file in html format.

To write a new test suite for an input plugin, you must be used the Check Metric keyword as a template. 
You can take example with the plugin-rapl.robot.
