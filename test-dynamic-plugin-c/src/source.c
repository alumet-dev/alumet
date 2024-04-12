#include <sys/stat.h>
#include <sys/types.h>
#include <stdio.h>
#include <errno.h>
#include <string.h>

#include "source.h"

// Get the size of a file.
static off_t file_size(const char *filename);

/// @brief Creates a new PowercapSource.
/// @param metric_id id of the metric to push the measurements to - should be obtained in plugin_start()
/// @return the new source
PowercapSource *source_init(RawMetricId metric_id, AString custom_attribute) {
    PowercapSource *source = malloc(sizeof(PowercapSource));

    // store the custom attribute (it's only there for testing purposes)
    source->custom_attribute = custom_attribute;

    // remmeber the metric id, so that we can push metrics to alumet in source_poll
    source->metric_id = metric_id;

    // open powercap sysfs file for package 0
    source->powercap_sysfs_file = "/sys/devices/virtual/powercap/intel-rapl/intel-rapl:0/energy_uj";
    source->powercap_sysfs_fd = fopen(source->powercap_sysfs_file, "r");
    if (!source->powercap_sysfs_fd) {
        fprintf(stderr, "Failed to open file '%s': %s\n", source->powercap_sysfs_file, strerror(errno));
    }

    // determine buffer size
    off_t max_size = file_size("/sys/devices/virtual/powercap/intel-rapl/intel-rapl:0/max_energy_range_uj");
    if (max_size < 0) {
        return NULL;
    }
    source->buf_size = max_size+1; // +1 for the trailing zero that we'll add to the string

    // set previous counter to "None"
    source->previous_counter = -1;

    return source;
}

/// @brief Destructor of the source: frees the memory that source points to.
/// @param source the source to destruct
void source_drop(PowercapSource *source) {
    int ok = fclose(source->powercap_sysfs_fd);
    if (ok != 0) {
        fprintf(stderr, "Error in fclose(%s): %s\n", source->powercap_sysfs_file, strerror(errno));
    }
    astring_free(source->custom_attribute);
    free(source);
}

/// @brief Source.poll(acc, timestamp)
/// @param source the source to poll
/// @param acc where to write the measurements to
/// @param timestamp the current timestamp
void source_poll(PowercapSource *source, MeasurementAccumulator *acc, Timestamp timestamp) {
    // The first argument of SourcePollFn is void*, but it's actually a pointer to the source struct,
    // so it's fine to use PowercapSource* directly.

    // read the file into a buffer
    char *buffer = calloc(source->buf_size, 1);
    FILE *f = source->powercap_sysfs_fd;
    size_t n_bytes_read = fread(buffer, 1, source->buf_size, f);
    if (ferror(f)) {
        fprintf(stderr, "Failed to read file '%s': %s\n", source->powercap_sysfs_file, strerror(errno));
        return; // TODO propagate error
    } else if ((n_bytes_read < source->buf_size) && feof(f)) {
    }
    rewind(f); // go back to the beginning

    // parse the powercap counter
    errno = 0;
    char *end;
    long long counter = strtoll(buffer, &end, 10);
    if (errno != 0) {
        fprintf(stderr, "Failed to parse file '%s' with content '%s': %s\n", source->powercap_sysfs_file, buffer, strerror(errno));
        return; // TODO propagate error
    }

    // compute the different between the previous value of the counter
    long long previous = source->previous_counter;
    uint64_t consumed_energy_uj;
    if (previous == -1) {
        consumed_energy_uj = (uint64_t)counter;
    } else {
        if (counter < previous) {
            consumed_energy_uj = (uint64_t)(counter) - (uint64_t)(previous) + (uint64_t)(0xFFFFFFFFFFFFFFFF);
        } else {
            consumed_energy_uj = (uint64_t)(counter - previous);
        }
    }

    // convert the counter to joules
    double joules = consumed_energy_uj * 0.0000001;

    // create the measurement point
    FfiResourceId resource = resource_new_cpu_package(0);
    MeasurementPoint *p = mpoint_new_f64(timestamp, source->metric_id, resource, joules);
    mpoint_attr_u64(p, astring_ref(source->custom_attribute), 1234);

    // push the measurement to alumet
    maccumulator_push(acc, p);
}

off_t file_size(const char *filename) {
    struct stat st;
    if (stat(filename, &st) != 0) {
        fprintf(stderr, "Cannot determine the size of file '%s': %s\n", filename, strerror(errno));
        return -1;
    }
    return st.st_size;
}
