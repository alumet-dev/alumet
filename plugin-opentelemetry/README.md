# OpenTelemetry plugin

This crate is a library that defines the OpenTelemetry plugin.

Implements a push-based exporter (via gRPC) which can be connected to an OpenTelemetry Collector (via a receiver), processed in any way, and then exported to a observability backend like Jaeger, Prometheus, Thanos, OpenSearch, ElasticSearch, etc.
