#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

typedef struct ConfigArray ConfigArray;

typedef struct ConfigTable ConfigTable;

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
