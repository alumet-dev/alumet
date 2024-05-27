# Alumet relay collection client/server

This crate contains a special version of the Alumet agent that works in "relay mode" with the `plugin-relay`.

Two binaries are produced:
- a relay agent, that runs on every system to monitor
- a relay server, that collects the metrics sent by the agents
