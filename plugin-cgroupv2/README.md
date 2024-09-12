# Cgroup V2 plugins

Allows to measure CPU-time used using the cgroup v2 sysfs.

## Table of Contents
1. [Kubernetes Plugin](#Kubernetes-Plugin)
2. [OAR3 Plugin](#OAR3-Plugin)

# Kubernetes Plugin

## How to use

Just compile the app-agent of the alumet's github repository.

```bash
cargo run
```

Make sure that in the app-agent's cargo.toml file the Kubernetes plugin is imported.
Make sure that in main.rs of app-agent the Kubernetes plugin is imported and used.

The binary created by the compilation will be found under the target repository.

## Prepare your environment

To work this plugin needs several things.

1. cgroupv2
2. kubectl
3. alumet-reader user

### cgroupv2

As the plugin use cgroupv2 to gather data, make sure to use this version of cgroup. In fact, a check is made by the plugin.
If it doesn't detect the use of cgroupv2, the plugin will not start.

### kubectl

In this version of the plugin, to gather some data about pods and nodes, the plugin use kubectl. So make sure that kubectl is installed and usable.
[How to install kubectl](https://kubernetes.io/docs/tasks/tools/install-kubectl-linux/)

Thanks to kubectl, you can get the kubernetes API URL.
Use:

```bash
kubectl config view -o jsonpath='{"Cluster name\tServer\n"}{range .clusters[*]}{.name}{"\t"}{.cluster.server}{"\n"}{end}'
```

And note the URL corresponding to the kubernetes you want interact with.

Example:

```bash
Cluster name    Server
kubernetes      https://1.2.3.4:6443
```

I get the kubernetes part and I wrote the result in the **alumet-config.toml** file.
Under the section: **[plugins.k8s]** I add the following:
> **kubernetes_api_url = "https://1.2.3.4:6443"**

By default the value **kubernetes_api_url** will be set at: **https://127.0.0.1:8080**

### alumet-reader

To use the Kubernetes API an user is needed. It's the **alumet-reader** user. Make sur the user exist and have the good rights.
You can use the yaml file: [alumet-user.yaml](./alumet-user.yaml) to create this user.
Run:

```bash
kubectl apply -f alumet-user.yaml
```

## How it works

The plugin read data inside the sysfs. At its start, the plugin retrieves a token thanks to the alumet-reader user. It uses the token to retrieve all pods running on the current node.
After that, it retrieves all cgroup related to Kubernetes pods, and make a correspondence between data retrieve from API and gathered from cgroup to fulfil the input plugin.
When a new pod is started. The plugin interrogate the API to retrieve data about the pod, such as its name, its namespace and the node its running on.

# OAR3 Plugin

## How to use

Just compile the app-agent of the alumet's github repository.

```bash
cargo run
```

Make sure that in the app-agent's cargo.toml file the OAR3 plugin is imported.
Make sure that in main.rs of app-agent the OAR3 plugin is imported and used.

The binary created by the compilation will be found under the target repository.

## Prepare your environment

To work this plugin needs several things.

1. cgroupv2

### cgroupv2

As the plugin uses cgroupv2 to gather data, make sure to use this version of cgroup. In fact, a check is made by the plugin.
If it doesn't detect the use of cgroupv2, the plugin will not start.


## How it works

The plugin read data inside the sysfs. It read file under the path given in the config. If it find a cpu consumption time it gather it. If some folder are created, it try to read the new folder and find information about time consumption.
