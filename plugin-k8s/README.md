# Kubernetes plugin

Allows to measure K8S pods using the cgroup v2 sysfs.

# How to use

Just compile the app-agent of the alumet's github repository.

```bash
cargo run
```

Make sure that in the app-agent's cargo.toml file the Kubernetes plugin is imported.
Make sure that in main.rs of app-agent the Kubernetes plugin is imported and used.

The binary created by the compilation will be found under the target repository.

# Prepare your environment

To work this plugin needs several things.

1. cgroupv2
2. kubectl
3. alumet-reader user

## cgroupv2

As the plugin use cgroupv2 to gather data, make sure to use this version of cgroup. In fact, a check is made by the plugin.
If it doesn't detect the use of cgroupv2, the plugin will not start.

## kubectl

In this version of the plugin, to gather some data about pods and nodes, the plugin use kubectl. So make sure that kubectl is installed and usable.
[How to install kubectl](https://kubernetes.io/docs/tasks/tools/install-kubectl-linux/)

## alumet-reader

To use the Kubernetes API an user is needed. It's the alumet-reader user. Make sur the user exist and have the good rights.
You can use the yaml file: [alumet-user.yaml](./alumet-user.yaml) to create this user.
Run:

```bash
kubectl apply -f alumet-user.yaml
```

# How it work

The plugin read data inside the sysfs. At its start, the plugin retrieves a token thanks to the alumet-reader user. It uses the token to retrieve all pods running on the current node.
After that, it retrieves all cgroup related to Kubernetes pods, and make a correspondence between data retrieve from API and gathered from cgroup to fulfil the input plugin.
When a new pod is started. The plugin interrogate the API to retrieve data about the pod, such as its name, its namespace and the node its running on.
