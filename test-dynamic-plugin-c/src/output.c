#include <inttypes.h>
#include "output.h"

StdOutput *output_init() {
    return malloc(sizeof(StdOutput));
}

void output_drop(StdOutput *output) {
    free(output);
}

void output_write(StdOutput *output, const MeasurementBuffer *buffer) {
    mbuffer_foreach(buffer, NULL, write_point);
}

void write_point(void *data, const CMeasurementPoint point) {
    // TODO: how to get the metric name, resource kind and resource id,
    // as a const *c_char owned by C code? this looks tedious and easy to get wrong,
    // because memory allocated by Rust must be deallocated by Rust
    switch (point.value.tag) {
        case CMeasurementValue_U64: {
            printf("%s %s = " PRIu64 "\n", point.timestamp.secs, point.resource.resource, point.value.u64);
        }
        break;
        case CMeasurementValue_F64: {
            printf("%s %s = %f\n", point.timestamp.secs, point.resource.resource, point.value.f64);
        }
        break;
    };
}
