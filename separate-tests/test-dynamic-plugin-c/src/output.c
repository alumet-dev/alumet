#include <inttypes.h>
#include "output.h"

void write_point(void *data, const MeasurementPoint *point);

StdOutput *output_init() {
    return malloc(sizeof(StdOutput));
}

void output_drop(StdOutput *output) {
    free(output);
}

void output_write(StdOutput *output, const MeasurementBuffer *buffer, const FfiOutputContext *ctx) {
    mbuffer_foreach(buffer, (void*)ctx, write_point);
}

void write_point(void *data, const MeasurementPoint *point) {
    const FfiOutputContext *ctx = data;
    FfiMeasurementValue value = mpoint_value(point);
    Timestamp t = mpoint_timestamp(point);
    AStr metric = metric_name(mpoint_metric(point), ctx);

    AString resource_kind = mpoint_resource_kind(point);
    AString resource_id = mpoint_resource_id(point);
    AString consumer_kind = mpoint_consumer_kind(point);
    AString consumer_id =   mpoint_consumer_id(point);
    RawMetricId metric_id = mpoint_metric(point);

    switch (value.tag) {
        case FfiMeasurementValue_U64: {
            printf("[%lu] on %.*s %.*s by %.*s %.*s, %.*s(id %lu) = %" PRIu64 "\n",
                t.secs,
                (int)resource_kind.len, resource_kind.ptr,
                (int)resource_id.len, resource_id.ptr,
                (int)consumer_kind.len, consumer_kind.ptr,
                (int)consumer_id.len, consumer_id.ptr,
                (int)metric.len, metric.ptr,
                metric_id._0,
                value.u64
            );
        }
        break;
        case FfiMeasurementValue_F64: {
            printf("[%lu] on %.*s %.*s by %.*s %.*s, %.*s(id %lu) = %f\n",
                t.secs,
                (int)resource_kind.len, resource_kind.ptr,
                (int)resource_id.len, resource_id.ptr,
                (int)consumer_kind.len, consumer_kind.ptr,
                (int)consumer_id.len, consumer_id.ptr,
                (int)metric.len, metric.ptr,
                metric_id._0,
                value.f64
            );
        }
        break;
    };
}
