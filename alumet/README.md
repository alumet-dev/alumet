# ALUMET core library

This crate contains the core of ALUMET.
It is intended to be used as a dependency of a binary crate to create a runnable measurement tool, such as [`app-agent`](../app-agent/).

## Plugin API

ALUMET provides a plugin API for static and dynamic plugins, written in Rust or C.

Static Rust plugins are regular libary crates, added to the dependencies of the runnable binary, alongside the `alumet` library crate.
They use the public interface of the `alumet` crate.

Dynamic plugins, on the other hand, do not depend on the `alumet` crate, but on its exported C API (yes, this is also true for dynamic plugins written in Rust). The C ABI (Application Binary Interface) is used as a stable ABI, because the default Rust ABI is voluntarily unstable across compiler versions.

The exported C API is automatically generated with `cbindgen`, and can be found in the [`generated/` folder](./generated/).
