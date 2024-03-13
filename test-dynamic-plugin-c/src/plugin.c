#include <stdio.h>
#include <string.h>
#include "../../alumet/generated/alumet-api.h"
#include "source.h"
#include "output.h"

PLUGIN_API const char *PLUGIN_NAME = "test-dynamic-plugin-c";
PLUGIN_API const char *PLUGIN_VERSION = "0.1.0";
PLUGIN_API const char *ALUMET_VERSION = "0.1.0";

typedef struct {
    const char *custom_attribute;
} PluginStruct;

PLUGIN_API PluginStruct *plugin_init(const ConfigTable *config) {
    PluginStruct *plugin = malloc(sizeof(PluginStruct));
    const char *custom_attribute = config_string_in(config, "custom_attribute");
    if (custom_attribute == NULL) {
        plugin->custom_attribute = "null";
    } else {
        char *buf = calloc(strlen(custom_attribute)+1, 1);
        plugin->custom_attribute = strcpy(buf, custom_attribute);
    }
    printf("plugin = %p, custom_attribute = %s\n", plugin, plugin->custom_attribute);
    return plugin;
}

PLUGIN_API void plugin_start(PluginStruct *plugin, AlumetStart *alumet) {
    printf("plugin_start begins with plugin = %p, custom_attribute = %s\n", plugin, plugin->custom_attribute);

    // create the source
    Unit u = {.tag = Unit_Joule};
    UntypedMetricId rapl_pkg_metric = alumet_create_metric(alumet, "rapl_pkg_consumption", WrappedMeasurementType_F64, u, "Energy consumption of the RAPL domain `package`, since the previous measurement.");
    PowercapSource *source = source_init(rapl_pkg_metric, plugin->custom_attribute);

    // register the source
    alumet_add_source(alumet, source, (SourcePollFn)source_poll, (NullableDropFn)source_drop);
    
    // create and register the output
    StdOutput *output = output_init();
    alumet_add_output(alumet, output, (OutputWriteFn)output_write, (NullableDropFn)output_drop);

    // ok!
    printf("plugin_start finished successfully\n");
}

PLUGIN_API void plugin_stop(PluginStruct *plugin) {
    printf("plugin stopped\n");
}

PLUGIN_API void plugin_drop(PluginStruct *plugin) {
    printf("plugin Dropped\n");
    free(plugin);
}
