# RAPL plugin

Collects the energy consumption of the CPU and other RAPL domains.

Currently, this plugin only works on Linux, because it relies on some abstractions provided by the Linux kernel over RAPL.
Using MSR registers directly is tricky, hard to maintain, and does not offer any performance benefit.
