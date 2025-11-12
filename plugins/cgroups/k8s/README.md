# K8S plugin

The `k8s` plugin gathers measurements about Kubernetes pods.

## Requirements

You need:
1. A Kubernetes cluster
2. A ServiceAccount token (see the configuration section)

We do not require a minimum version, because our use of the API is very minimal.

To test the plugin locally, you can use [minikube](https://minikube.sigs.k8s.io).

## Metrics

Here are the metrics collected by the plugin's sources.

|Name|Type|Unit|Description|Resource|ResourceConsumer|Attributes|
|----|----|----|-----------|--------|----------------|----------|
|`cpu_time_delta`|Delta|nanoseconds|time spent by the pod executing on the CPU|`LocalMachine`|`Cgroup`|see below|
|`cpu_percent`|Gauge|Percent (0 to 100)|`cpu_time_delta / delta_t` (1 core used fully = 100%)|`LocalMachine`|`Cgroup`|see below|
|`memory_usage`|Gauge|Bytes|total pod's memory usage|`LocalMachine`|`Cgroup`|see below|
|`cgroup_memory_anonymous`|Gauge|Bytes|anonymous memory usage|`LocalMachine`|`Cgroup`|see below|
|`cgroup_memory_file`|Gauge|Bytes|memory used to cache filesystem data|`LocalMachine`|`Cgroup`|see below|
|`cgroup_memory_kernel_stack`|Gauge|Bytes|memory allocated to kernel stacks|`LocalMachine`|`Cgroup`|see below|
|`cgroup_memory_pagetables`|Gauge|Bytes|memory reserved for the page tables|`LocalMachine`|`Cgroup`|see below|

### Attributes

The measurements produced by the `k8s` plugin have the following attributes:
- `uid`: the pod's UUID
- `name`: the pod's name
- `namespace`: the pod's namespace
- `node`: the name of the node (see the configuration)

The **cpu** measurements have an additional attribute `kind`, which can be one of:
- `total`: time spent in kernel and user mode
- `system`: time spent in kernel mode only
- `user`: time spent in user mode only

## Configuration

Here are some examples of how to configure this plugin.

### Example Configuration for Minikube
<!-- markdownlint-disable MD029 -->

Context: you have started Minikube on your local machine and want to run Alumet alongside of it (not in a pod).

Prerequisites:
1. create a namespace and service account:

```sh
kubectl create ns alumet
kubectl create serviceaccount alumet-reader -n alumet
```

The service account's token will be created and retrieved by the `k8s` Alumet plugin itself.

2. Make the K8S API available locally:

```sh
kubectl proxy --port=8080
```

Then, you can use the following configuration:

```toml
[plugins.k8s]
k8s_node = "minikube"
k8s_api_url = "http://127.0.0.1:8080"
token_retrieval = "auto"
poll_interval = "5s"
```

### Example Configuration for a full K8S Cluster

Context: you have a K8S cluster and are deploying Alumet in a pod.

Prerequisites:
1. Inject the name of the node in the `NODE_NAME` environment variable of the pod that runs the Alumet agent. See [K8S Docs − Expose Pod Information to Containers Through Environment Variables](https://kubernetes.io/docs/tasks/inject-data-application/environment-variable-expose-pod-information/).
2. Create a ServiceAccount and mount its token in the pod that runs the Alumet agent.

Then, configure the `k8s` plugin.
A typical configuration would look like the following:

```toml
[plugins.k8s]
k8s_node = "${NODE_NAME}"
k8s_api_url = "https://kubernetes.default.svc:443"
token_retrieval = "file"
poll_interval = "5s"
```

### Possible Token Retrieval Strategies

```toml
# try "file" and fall back to "kubectl"
token_retrieval = "auto"

# run 'kubectl create token'
token_retrieval = "kubectl"

# read /var/run/secrets/kubernetes.io/serviceaccount/token
token_retrieval = "file"

# custom file
token_retrieval.file = "/path/to/token"

# custom kubectl
token_retrieval.kubectl = {
    service_account = "alumet-reader"
    namespace = "alumet"
}
```
