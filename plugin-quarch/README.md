# Quarch Plugin

## Requirements

- Have a account on Grid'5000.
- Have the debian file of Alumet, `scripts-configuration.txt`, `set.sh`, `run.sh`, and `exec.sh` file in the same folder on your computer.

## Metrics

Here are examples of the metrics collected by the plugin source.

| metric | timestamp | value | resource_kind | resource_id | consumer_kind | consumer_id | __late_attributes |
| --- | --- | --- | --- | --- | --- | --- | --- |
| disk_power_W | 2025-08-07T13:28:59.704495376Z | 0.000011400000000000001 | local_machine | | local_machine | | |
| disk_power_W | 2025-08-07T13:28:59.706319388Z | 0.000011400000000000001 | local_machine | | local_machine | | |
| disk_power_W | 2025-08-07T13:29:00.704652356Z | 0.000011400000000000001 | local_machine | | local_machine | | |
| disk_power_W | 2025-08-07T13:29:01.704617976Z | 0.000011400000000000001 | local_machine | | local_machine | | |

## Configuration

Here is a configuration example of the plugin. It's part of the Alumet configuration file (eg: `alumet-config.toml`).

```toml
[plugins.quarch]
quarch_ip = "172.17.30.102" # always this on yeti
quarch_port = 8080 # always this on yeti
metrics = ["disk_power"] # always this on yeti
poll_interval = "1s"
flush_interval = "5s"
```

## Usage

### Inital commands

``` bash
# Example on how to put the files on g5k
$ scp -r -i `ssh_key_g5k` `repo with the files` grenoble.g5k:/home/login/

# To set up node yeti-x :
login@fgrenoble$ ./set.sh yeti-x

# Every time you want to exec alumet:
login@fgrenoble$ ./exec.sh yeti-x command_to_exec

# Every time you want to run alumet:
login@fgrenoble$ ./run.sh yeti-x

# If you need, you can access node by
login@fgrenoble$ ssh root@yeti-x
``` 
### Outputs examples

``` bash
login@fgrenoble$ ./set.sh yeti-4
# Include exotic resources in the set of reservable resources (this does NOT exclude non-exotic resources).
OAR_JOB_ID=2522461
Node reserved with job ID: 2522461
Waiting for the job to start...
Current job status: Waiting
Current job status: Running
Job is running on node: yeti-4
Current job status: Launching
Deploying environment on yeti-4 with kadeploy...
...
... # Deploying
...
Setting up node yeti-4...
...
... # Dowloading tools the plugin needs on yeti-4
...
``` 
``` bash
login@fgrenoble$ ./exec.sh yeti-3 csv sleep 10
Do you want to keep the current config for the result directory?
-----
/home/mdacosta/public/results/quarch_implementation/2025-08-07-15-28-51
-----
Use this config? [Y/n] y
Directory created successfully
...
[2025-08-07T13:28:54Z INFO  alumet_agent] Default configuration file written to: /etc/alumet/alumet-config.toml
Do you want to keep the current config for alumet?
-----
# Alumet config file
-----
Use this config? [Y/n] y
Do you want to keep the current output file name?
-----
/home/mdacosta/public/results/quarch_implementation/2025-08-07-15-28-51/alumet-output.csv
-----
Use this output file name? [Y/n] y
PLUGIN_LIST: quarch,csv
COMMAND_TO_EXEC: sleep 10
[2025-08-07T13:28:59Z INFO  alumet_agent] Starting Alumet agent 'alumet-agent' v0.8.4-a4c62a2-dirty (2025-08-07T09:45:00.904535984Z, rustc 1.81.0, debug=false)
...
... #Alumet execution
    📥 1 source, 🔀 0 transform and 📝 1 output registered.
...
 Gathering experiment results...
alumet-output.csv                                          100% 1562   466.4KB/s   00:00
 Done.
```
``` bash
login@fgrenoble$ ./run.sh yeti-3 csv
Do you want to keep the current config for the result directory?
-----
/home/mdacosta/public/results/quarch_implementation/2025-08-07-15-30-25
-----
Use this config? [Y/n] y
[2025-08-07T13:30:27Z INFO  alumet_agent] Starting Alumet agent 'alumet-agent' v0.8.4-a4c62a2-dirty (2025-08-07T09:45:00.904535984Z, rustc 1.81.0, debug=false)
[2025-08-07T13:30:27Z WARN  plugin_cgroupv2::k8s::plugin] Error : Path '/sys/fs/cgroup/kubepods.slice/' not exist.
[2025-08-07T13:30:27Z INFO  alumet_agent] Default configuration file written to: /etc/alumet/alumet-config.toml
Do you want to keep the current config for alumet?
-----
... # Alumet config file
-----
Use this config? [Y/n] y
Do you want to keep the current output file name?
-----
/home/mdacosta/public/results/quarch_implementation/2025-08-07-15-30-25/alumet-output.csv
-----
Use this output file name? [Y/n] y
PLUGIN_LIST: quarch,csv
[2025-08-07T13:30:29Z INFO  alumet_agent] Starting Alumet agent 'alumet-agent' v0.8.4-a4c62a2-dirty (2025-08-07T09:45:00.904535984Z, rustc 1.81.0, debug=false)
...
... # Alumet execution
    📥 1 source, 🔀 0 transform and 📝 1 output registered.
^C

 Gathering experiment results...
alumet-output.csv                                          100%  573   226.9KB/s   00:00
 Done.
```

### Getting datas

wget pour recup ??

## More information

?????
