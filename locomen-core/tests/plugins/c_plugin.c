#include <stdio.h>
#include <c_locomen.h>

const char *PLUGIN_NAME = "test_plugin";
const char *PLUGIN_VERSION = "0.0.1";

struct my_plugin {
    _Atomic uint64_t counter;  
};

struct my_plugin *plugin_init(struct ConfigTable *config) {
    printf("plugin_init has been called\n");
    struct my_plugin *plugin = malloc(sizeof(struct my_plugin));
    plugin->counter = 0;
    return plugin;
}

void plugin_start(struct my_plugin *self) {
    printf("plugin starting\n");
}

void plugin_stop(struct my_plugin *self) {
    printf("plugin stopping\n");
}
