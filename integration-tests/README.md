# Alumet integration tests <!-- omit in toc -->

This folder contains all script files and scenarios for testing alumet on bare metal.

The scenarios (test suites) are located in scenarios folder. Robot framework is used to perform automatically the tests. Each suite test is a set of tests.
The test strategy is to write one robot framework file (a test suite) per alumet plugin.
Each test can have one or several tags allowing to exclude tests to during the execution with robot framework.

Before executing the tests, you need to install robot framework.
After you need to initialize the robot framework environment:

```bash
 source ~/venv-robot/bin/activate
(venv-robot) [zychlae@carbon0 integration_tests]$
```

A script run.sh allows to execute the robot framework tests on a target node.

Then you can execute your test:

```bash

==============================================================================
Plugin-Rapl :: Alumet test plugin rapl
==============================================================================
Test connection on target node                                        | PASS |
------------------------------------------------------------------------------
run plugins socket-control csv rapl                                   | PASS |
------------------------------------------------------------------------------
check alumet running                                                  | PASS |
------------------------------------------------------------------------------
Check Rapl Metrics package                                            metric value read: 26.535888671875
Check Rapl Metrics package                                            | PASS |
------------------------------------------------------------------------------
Check Rapl Metrics package_total                                      metric value read: 48.0457763671875
Check Rapl Metrics package_total                                      | PASS |
------------------------------------------------------------------------------
Check Rapl Metrics dram                                               metric value read: 3.331390380859375
Check Rapl Metrics dram                                               | PASS |
------------------------------------------------------------------------------
Check Rapl Metrics dram_total                                         metric value read: 6.8105316162109375
Check Rapl Metrics dram_total                                         | PASS |
------------------------------------------------------------------------------
stop alumet                                                           | PASS |
------------------------------------------------------------------------------
Check alumet not running                                              | PASS |
------------------------------------------------------------------------------
Plugin-Rapl :: Alumet test plugin rapl                                | PASS |
9 tests, 9 passed, 0 failed
==============================================================================
```

The robot framework generates a report and log file in html format.

To write a new test suite for an input plugin, you must be used the Check Metric keyword as a template. 
You can take example with the plugin-rapl.robot.
