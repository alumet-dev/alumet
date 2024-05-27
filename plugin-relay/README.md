# Relay plugin

The relay plugin allows to send and receive measurements over the network with the efficient gRPC protocol.

This plugin is made of two parts, enabled by cargo features:
- `client`: sends all measurements to the relay server
- `server`: receives measurements from one or multiple clients
