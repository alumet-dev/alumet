#ifndef __SOURCE_H
#define __SOURCE_H

#include <stdio.h>
#include "../../alumet/generated/alumet-api.h"

typedef struct {
    AString custom_attribute;
    UntypedMetricId metric_id; // id of the alumet metric
    const char *powercap_sysfs_file;
    FILE *powercap_sysfs_fd;
    size_t buf_size;
    long long previous_counter; // -1 for None
} PowercapSource;

PowercapSource *source_init(UntypedMetricId metric_id, AString custom_attribute);
void source_drop(PowercapSource *source);
void source_poll(PowercapSource *source, MeasurementAccumulator *acc, Timestamp timestamp);

#endif
