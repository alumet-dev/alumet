#ifndef __ALUMET_API_H
#define __ALUMET_API_H

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#define PLUGIN_API __attribute__((visibility("default")))

typedef enum WrappedMeasurementType {
  WrappedMeasurementType_F64,
  WrappedMeasurementType_U64,
} WrappedMeasurementType;

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

/**
 * A metric id, used for internal purposes such as storing the list of metrics.
 */
typedef struct UntypedMetricId {
  uintptr_t _0;
} UntypedMetricId;

typedef struct CustomUnitId {
  uint32_t _0;
} CustomUnitId;

enum Unit_Tag {
  /**
   * Indicates a dimensionless value. This is suitable for counters.
   */
  Unit_Unity,
  /**
   * Standard unit of **time**.
   */
  Unit_Second,
  /**
   * Standard unit of **power**.
   */
  Unit_Watt,
  /**
   * Standard unit of **energy**.
   */
  Unit_Joule,
  /**
   * Electric tension (aka voltage)
   */
  Unit_Volt,
  /**
   * Intensity of an electric current
   */
  Unit_Ampere,
  /**
   * Frequency (1 Hz = 1/second)
   */
  Unit_Hertz,
  /**
   * Temperature in °C
   */
  Unit_DegreeCelsius,
  /**
   * Temperature in °F
   */
  Unit_DegreeFahrenheit,
  /**
   * Energy in Watt-hour (1 W⋅h = 3.6 kiloJoule = 3.6 × 10^3 Joules)
   */
  Unit_WattHour,
  /**
   * A custom unit
   */
  Unit_Custom,
};
typedef uint8_t Unit_Tag;

typedef union Unit {
  Unit_Tag tag;
  struct {
    Unit_Tag custom_tag;
    struct CustomUnitId custom;
  };
} Unit;

typedef struct Timestamp {
  uint64_t secs;
  uint32_t nanos;
} Timestamp;

typedef void (*SourcePollFn)(void *instance,
                             struct MeasurementAccumulator *buffer,
                             struct Timestamp timestamp);

typedef void (*NullableDropFn)(void *instance);

typedef void (*TransformApplyFn)(void *instance, struct MeasurementBuffer *buffer);

typedef void (*OutputWriteFn)(void *instance, const struct MeasurementBuffer *buffer);

typedef enum CResourceId_Tag {
  /**
   * The whole local machine, for instance the whole physical server.
   */
  CResourceId_LocalMachine,
  /**
   * A process at the OS level.
   */
  CResourceId_Process,
  /**
   * A control group, often abbreviated cgroup.
   */
  CResourceId_ControlGroup,
  /**
   * A physical CPU package (which is not the same as a NUMA node).
   */
  CResourceId_CpuPackage,
  /**
   * A CPU core.
   */
  CResourceId_CpuCore,
  /**
   * The RAM attached to a CPU package.
   */
  CResourceId_Dram,
  /**
   * A dedicated GPU.
   */
  CResourceId_Gpu,
  /**
   * A custom resource
   */
  CResourceId_Custom,
} CResourceId_Tag;

typedef struct CResourceId_Process_Body {
  uint32_t pid;
} CResourceId_Process_Body;

typedef struct CResourceId_ControlGroup_Body {
  const char *path;
} CResourceId_ControlGroup_Body;

typedef struct CResourceId_CpuPackage_Body {
  uint32_t id;
} CResourceId_CpuPackage_Body;

typedef struct CResourceId_CpuCore_Body {
  uint32_t id;
} CResourceId_CpuCore_Body;

typedef struct CResourceId_Dram_Body {
  uint32_t pkg_id;
} CResourceId_Dram_Body;

typedef struct CResourceId_Gpu_Body {
  const char *bus_id;
} CResourceId_Gpu_Body;

typedef struct CResourceId_Custom_Body {
  const char *kind;
  const char *id;
} CResourceId_Custom_Body;

typedef struct CResourceId {
  CResourceId_Tag tag;
  union {
    CResourceId_Process_Body process;
    CResourceId_ControlGroup_Body control_group;
    CResourceId_CpuPackage_Body cpu_package;
    CResourceId_CpuCore_Body cpu_core;
    CResourceId_Dram_Body dram;
    CResourceId_Gpu_Body gpu;
    CResourceId_Custom_Body custom;
  };
} CResourceId;

typedef enum CMeasurementValue_Tag {
  CMeasurementValue_U64,
  CMeasurementValue_F64,
} CMeasurementValue_Tag;

typedef struct CMeasurementValue {
  CMeasurementValue_Tag tag;
  union {
    struct {
      uint64_t u64;
    };
    struct {
      double f64;
    };
  };
} CMeasurementValue;

typedef struct CMeasurementPoint {
  struct UntypedMetricId metric;
  struct Timestamp timestamp;
  struct CMeasurementValue value;
  const struct MeasurementPoint *_original;
} CMeasurementPoint;

typedef void (*ForeachPointFn)(void*, struct CMeasurementPoint);

struct UntypedMetricId alumet_create_metric(struct AlumetStart *alumet,
                                            const char *name,
                                            enum WrappedMeasurementType value_type,
                                            union Unit unit,
                                            const char *description);

void alumet_add_source(struct AlumetStart *alumet,
                       void *source_data,
                       SourcePollFn source_poll_fn,
                       NullableDropFn source_drop_fn);

void alumet_add_transform(struct AlumetStart *alumet,
                          void *transform_data,
                          TransformApplyFn transform_apply_fn,
                          NullableDropFn transform_drop_fn);

void alumet_add_output(struct AlumetStart *alumet,
                       void *output_data,
                       OutputWriteFn output_write_fn,
                       NullableDropFn output_drop_fn);

struct Timestamp *system_time_now(void);

struct MeasurementPoint *mpoint_new_u64(struct Timestamp timestamp,
                                        struct UntypedMetricId metric,
                                        struct CResourceId resource,
                                        uint64_t value);

struct MeasurementPoint *mpoint_new_f64(struct Timestamp timestamp,
                                        struct UntypedMetricId metric,
                                        struct CResourceId resource,
                                        double value);

/**
 * Free a MeasurementPoint.
 * Do **not** call this function after pushing a point with [`mbuffer_push`] or [`maccumulator_push`].
 */
void mpoint_free(struct MeasurementPoint *point);

void mpoint_attr_u64(struct MeasurementPoint *point, const char *key, uint64_t value);

void mpoint_attr_f64(struct MeasurementPoint *point, const char *key, double value);

void mpoint_attr_bool(struct MeasurementPoint *point, const char *key, bool value);

void mpoint_attr_str(struct MeasurementPoint *point, const char *key, const char *value);

uintptr_t mbuffer_len(const struct MeasurementBuffer *buf);

void mbuffer_reserve(struct MeasurementBuffer *buf, uintptr_t additional);

void mbuffer_foreach(const struct MeasurementBuffer *buf, void *data, ForeachPointFn f);

/**
 * Adds a measurement to the buffer.
 * The point is consumed in the operation, you must **not** use it afterwards.
 */
void mbuffer_push(struct MeasurementBuffer *buf, struct MeasurementPoint *point);

/**
 * Adds a measurement to the accumulator.
 * The point is consumed in the operation, you must **not** use it afterwards.
 */
void maccumulator_push(struct MeasurementAccumulator *buf, struct MeasurementPoint *point);

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

#endif
