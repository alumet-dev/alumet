#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

/**
 * `AlumetStart` allows the plugins to perform some actions before starting the measurment pipeline,
 * such as registering new measurement sources.
 */
typedef struct AlumetStart AlumetStart;

typedef struct ConfigArray ConfigArray;

typedef struct ConfigTable ConfigTable;

/**
 * An accumulator stores measured data points.
 * Unlike a [`MeasurementBuffer`], the accumulator only allows to [`push()`] new points, not to modify them.
 */
typedef struct MeasurementAccumulator MeasurementAccumulator;

/**
 * A `MeasurementBuffer` stores measured data points.
 * Unlike a [`MeasurementAccumulator`], the buffer allows to modify the measurements.
 */
typedef struct MeasurementBuffer MeasurementBuffer;

/**
 * A data point about a metric that has been measured.
 */
typedef struct MeasurementPoint MeasurementPoint;

typedef struct MeasurementType MeasurementType;

typedef struct TimeSpec TimeSpec;

typedef struct Unit Unit;

typedef void (*SourcePollFn)(void *instance,
                             struct MeasurementAccumulator *buffer,
                             const struct TimeSpec *timestamp);

typedef void (*TransformApplyFn)(void *instance, struct MeasurementBuffer *buffer);

typedef void (*OutputWriteFn)(void *instance, const struct MeasurementBuffer *buffer);

const char *config_string_in(const struct ConfigTable *table, const char *key);

const int64_t *config_int_in(const struct ConfigTable *table, const char *key);

const bool *config_bool_in(const struct ConfigTable *table, const char *key);

const double *config_float_in(const struct ConfigTable *table, const char *key);

const struct ConfigArray *config_array_in(const struct ConfigTable *table, const char *key);

const struct ConfigTable *config_table_in(const struct ConfigTable *table, const char *key);

const char *config_string_at(const struct ConfigArray *array, uintptr_t index);

const int64_t *config_int_at(const struct ConfigArray *array, uintptr_t index);

const bool *config_bool_at(const struct ConfigArray *array, uintptr_t index);

const double *config_float_at(const struct ConfigArray *array, uintptr_t index);

const struct ConfigArray *config_array_at(const struct ConfigArray *array, uintptr_t index);

const struct ConfigTable *config_table_at(const struct ConfigArray *array, uintptr_t index);

uint64_t alumet_create_metric(struct AlumetStart *alumet,
                              const char *name,
                              struct MeasurementType value_type,
                              struct Unit unit,
                              const char *description);

void alumet_add_source(struct AlumetStart *alumet, void *source_data, SourcePollFn source_poll_fn);

void alumet_add_transform(struct AlumetStart *alumet,
                          void *transform_data,
                          TransformApplyFn transform_apply_fn);

void alumet_add_output(struct AlumetStart *alumet,
                       void *output_data,
                       OutputWriteFn output_write_fn);

uintptr_t mbuffer_len(const struct MeasurementBuffer *buf);

void mbuffer_reserve(struct MeasurementBuffer *buf, uintptr_t additional);

void mbuffer_push(struct MeasurementBuffer *buf, struct MeasurementPoint point);

void maccumulator_push(struct MeasurementBuffer *buf, struct MeasurementPoint point);
