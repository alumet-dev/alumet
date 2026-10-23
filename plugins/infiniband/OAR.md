# Attaching job information to InfiniBand measurements (OAR)

This plugin does **not** produce the `job_id` or the `involved_jobs` attribute.
Job attribution is left to the job plugins, which know about the jobs running on the node.

To attach `involved_jobs`, enable the [`oar`](../cgroups/oar/README.md) plugin: its `JobInfoAttacher` transform adds `involved_jobs` (a `ListU64` of the sorted ids of the jobs currently running on the node) to every measurement that has no `job_id` attribute.

The InfiniBand plugin deliberately adds no `job_id`, so `involved_jobs` is node-global information: it does not give any per-job granularity, it only tells which jobs were running when the InfiniBand measurement was taken. Per-job attribution would require per-process or per-cgroup accounting, which the InfiniBand counters do not provide.

```toml
[plugins.infiniband]
poll_interval = "1s"

[plugins.oar]
oar_version = "oar3"
jobs_only = true
poll_interval = "5s"
# Keep the OAR sources enabled: they populate the job tracker used by
# JobInfoAttacher to build the involved_jobs list.
disable_sources = false
# Not needed for involved_jobs (it only adds job_id to foreign measurements).
annotate_foreign_measurements = false
```

The `oar` sources must stay enabled (`disable_sources = false`): they are what feeds the job tracker used by `JobInfoAttacher` to build the `involved_jobs` list. Disabling the sources would leave the transform registered but with an empty job list.
