# Alumet integration tests <!-- omit in toc -->

This folder contains all script files and scenarios for testing alumet on bare metal.

The scenarios ( a test suite) are located in scenarios folder. Robot framework is used to perform automatically the tests. Each suite test is a set of tests.
The test strategy is to write one robot framework file (a test suite) per alumet plugin.
Each test is tag allowing to exclude tests to during the execution with robot framework.

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
install alumet                                                        | PASS |
------------------------------------------------------------------------------
run plugins socket-control csv rapl                                   | PASS |
------------------------------------------------------------------------------
check alumet running                                                  | PASS |
------------------------------------------------------------------------------
check metric rapl_consumed_energy_J resource cpu_package domain pa... ..metric value read: 29.18206787109375
check metric rapl_consumed_energy_J resource cpu_package domain pa... | PASS |
------------------------------------------------------------------------------
check metric rapl_consumed_energy_J resource cpu_package domain pa... ..metric value read: 52.879150390625
check metric rapl_consumed_energy_J resource cpu_package domain pa... | PASS |
------------------------------------------------------------------------------
check metric rapl_consumed_energy_J resource dram domain dram         ..metric value read: 3.3978118896484375
check metric rapl_consumed_energy_J resource dram domain dram         | PASS |
------------------------------------------------------------------------------
check metric rapl_consumed_energy_J resource dram domain dram_total   ..metric value read: 6.769805908203125
check metric rapl_consumed_energy_J resource dram domain dram_total   | PASS |
------------------------------------------------------------------------------
stop alumet                                                           | PASS |
------------------------------------------------------------------------------
Check alumet not running                                              | PASS |
------------------------------------------------------------------------------
Plugin-Rapl :: Alumet test plugin rapl                                | PASS |
10 tests, 10 passed, 0 failed
==============================================================================
```

The robot framework generates a report and log file in html format.