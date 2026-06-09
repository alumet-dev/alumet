# Process to Cgroup Bridge Plugin

The Process to Cgroup Bridge plugin creates an Alumet **transform** that will take as input measurements with a ResourceConsumer::Process and transform it to ResourceConsumer::ControlGroup using procfs to bridge the process id to the related cgroup.

It's designed to be coupled with another Alumet source that produce process measurements (eg: `plugin-nvidia-nvml`).
The [Configuration](#configuration) allows to make the transformation step only on some selected metrics.

## Requirements

- A **source** plugin that produces measurements with ResourceConsumer::Process

## Configuration

Here is a configuration example of the Process to Cgroup Bridge Plugin. It's part of the Alumet configuration file (eg: `alumet-config.toml`).

```toml
[plugins.process-to-cgroup-bridge]
# The metrics names we want to find the cgroup for
processes_metrics = [
    "some_metric_to_bridge",
    "another_metric_to_bridge",
]
# Will aggregate measurements in case multiple processes share the same cgroup and have the same timestamp. This leads to one measurement per metric per cgroup per timestamp.
merge_similar_cgroups = true
# Will keep all the measurements that have been processed by the transformer. In case it's false only the measurements with a cgroup resource consumer will be kept.
keep_processed_measurements = true
# Will keep use the cgroup of pod instead of container
pod_cgroup_only = true
```

## More informations

### Cgroup not found

In case the transform plugin doesn't find a cgroup for a process measurement, it will silently skip the transformation step for this measurement.

### Cgroup v1

The plugin currently does not handle processes managed by cgroup v1 correctly.
If a process is associated with multiple cgroups (that's the case in cgroup v1), the plugin will arbitrarily select one from the list found in the /proc/[pid]/cgroup file.

### `pod_cgroup_only` argument

Sometimes, the plugin retrieve as cgroup the container's one, so in case of annotation, the plugin is not able to find the related cgroup as we decided to only look at pod level.

#### Example

cgroup retrieved is: `/sys/fs/cgroup/kubepods.slice/kubepods-besteffort.slice/kubepods-besteffort-podYYYYYYYYY.slice/crio-XXXXXXX.scope`.
annotation plugin only know: `/sys/fs/cgroup/kubepods.slice/kubepods-besteffort.slice/kubepods-besteffort-podYYYYYYYYY.slice`.
So, set `pod_cgroup_only` argument at true will remove the last part and use as cgroup name the pod's one: `/sys/fs/cgroup/kubepods.slice/kubepods-besteffort.slice/kubepods-besteffort-podYYYYYYYYY.slice/crio-XXXXXXX.scope` will be replaced by `/sys/fs/cgroup/kubepods.slice/kubepods-besteffort.slice/kubepods-besteffort-podYYYYYYYYY.slice`
