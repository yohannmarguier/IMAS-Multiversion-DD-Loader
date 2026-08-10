// Minimal loadable plugin used to drive the plugin-management ABI through the
// shim and into a real IMAS-Core. IMAS-Core resolves the C factories below
// from <plugin-name>_plugin.so; the plugin itself has no libal dependency.

#include <access_layer_plugin.h>

#include <cstdio>
#include <cstdlib>
#include <string>

namespace {

class MvddTestPlugin : public access_layer_plugin {
public:
    void setParameter(const char* name, int datatype, int dim, int*,
                      void* data) override {
        const char* path = std::getenv("IMAS_MVDD_TEST_PLUGIN_LOG");
        if (path == nullptr) return;

        std::FILE* log = std::fopen(path, "a");
        if (log == nullptr) return;

        if (dim == 0 && data != nullptr && datatype == INTEGER_DATA) {
            std::fprintf(log, "%s|%d|%d|%d\n", name, datatype, dim,
                         *static_cast<int*>(data));
        } else if (dim == 0 && data != nullptr && datatype == DOUBLE_DATA) {
            std::fprintf(log, "%s|%d|%d|%g\n", name, datatype, dim,
                         *static_cast<double*>(data));
        } else {
            std::fprintf(log, "%s|%d|%d|-\n", name, datatype, dim);
        }
        std::fclose(log);
    }

    void begin_global_action(int, const char*, const char*, int, int) override {}
    void begin_slice_action(int, const char*, int, double, int, int) override {}
    void begin_arraystruct_action(int, int*, const char*, const char*,
                                  int*) override {}
    void end_action(int) override {}
    int read_data(int, const char*, const char*, void**, int, int, int*) override {
        return 0;
    }
    void write_data(int, const char*, const char*, void*, int, int,
                    int*) override {}
    plugin::OPERATION node_operation(const std::string&) override {
        return plugin::PUT_ONLY;
    }

    std::string getName() override { return "mvddtest"; }
    std::string getDescription() override { return "MVDD real-Core seam test"; }
    std::string getCommit() override { return "test"; }
    std::string getVersion() override { return "1.0.0"; }
    std::string getRepository() override { return "in-tree"; }
    std::string getParameters() override { return ""; }

    std::string getReadbackName(const std::string&, int*) override { return ""; }
    std::string getReadbackDescription(const std::string&) override { return ""; }
    std::string getReadbackCommit(const std::string&) override { return ""; }
    std::string getReadbackVersion(const std::string&) override { return ""; }
    std::string getReadbackRepository(const std::string&) override { return ""; }
    std::string getReadbackParameters(const std::string&) override { return ""; }
};

}  // namespace

extern "C" access_layer_base_plugin* create() { return new MvddTestPlugin(); }
extern "C" void destroy(access_layer_base_plugin* plugin) { delete plugin; }
