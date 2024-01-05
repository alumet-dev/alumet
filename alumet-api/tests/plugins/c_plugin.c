#include <stdio.h>
#include <alumet.h>

const char *PLUGIN_NAME = "test_plugin";
const char *PLUGIN_VERSION = "0.0.1";

struct my_plugin {
    _Atomic uint64_t counter;
};

struct my_plugin *plugin_init(struct ConfigTable *config) {
    printf("plugin initializing\n");

    // create the plugin struct (this allows to store some data during the life of the plugin)
    struct my_plugin *plugin = malloc(sizeof(struct my_plugin));
    if (plugin == NULL) {
        return NULL;
    }

    // read the config
    int64_t int_from_config = config_int_in(config, "int_value");
    printf("int from config: %lu\n", int_from_config);

    // set plugin data
    plugin->counter = 0;

    printf("plugin initialized\n");
    return plugin;
}

void plugin_start(struct my_plugin *self) {
    printf("plugin starting\n");
}

void plugin_stop(struct my_plugin *self) {
    printf("plugin stopping\n");
}

void plugin_drop(struct my_plugin *self) {
    printf("plugin dropping\n");
    free(self);
    printf("the plugin has been dropped, it cannot be used anymore\n");
}
