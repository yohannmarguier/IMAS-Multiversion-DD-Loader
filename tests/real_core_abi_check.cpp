// Compile-time comparison of the shim's generated declarations with the real
// IMAS-Core header. The runtime binding deliberately gives the linker no
// opportunity to type-check these signatures, so keep this translation unit
// in the default CMake build (ADR 0001).

#include <cstddef>
#include <tuple>
#include <type_traits>

// Give the real declarations private names so both public headers can be
// included in one translation unit. al_status_t also needs a private name:
// both projects intentionally use an anonymous C struct for it.
#define al_status_t core_al_status_t
#define al_context_info core_al_context_info
#define al_begin_dataentry_action core_al_begin_dataentry_action
#define al_close_pulse core_al_close_pulse
#define al_begin_global_action core_al_begin_global_action
#define al_begin_slice_action core_al_begin_slice_action
#define al_begin_timerange_action core_al_begin_timerange_action
#define al_begin_arraystruct_action core_al_begin_arraystruct_action
#define al_end_action core_al_end_action
#define al_read_data core_al_read_data
#define al_write_data core_al_write_data
#define al_delete_data core_al_delete_data
#define al_iterate_over_arraystruct core_al_iterate_over_arraystruct
#define al_get_occurrences core_al_get_occurrences
#define al_list_filled_paths core_al_list_filled_paths
#define al_register_plugin core_al_register_plugin
#define al_unregister_plugin core_al_unregister_plugin
#define al_bind_plugin core_al_bind_plugin
#define al_unbind_plugin core_al_unbind_plugin
#define al_bind_readback_plugins core_al_bind_readback_plugins
#define al_unbind_readback_plugins core_al_unbind_readback_plugins
#define al_is_plugin_registered core_al_is_plugin_registered
#define al_write_plugins_metadata core_al_write_plugins_metadata
#define al_setvalue_parameter_plugin core_al_setvalue_parameter_plugin
#define al_setvalue_int_scalar_parameter_plugin \
    core_al_setvalue_int_scalar_parameter_plugin
#define al_setvalue_double_scalar_parameter_plugin \
    core_al_setvalue_double_scalar_parameter_plugin
#define al_plugin_begin_global_action core_al_plugin_begin_global_action
#define al_plugin_begin_slice_action core_al_plugin_begin_slice_action
#define al_plugin_begin_timerange_action core_al_plugin_begin_timerange_action
#define al_plugin_begin_arraystruct_action core_al_plugin_begin_arraystruct_action
#define al_plugin_end_action core_al_plugin_end_action
#define al_plugin_read_data core_al_plugin_read_data
#define al_plugin_write_data core_al_plugin_write_data
#include <al_lowlevel.h>

constexpr int core_maxdim = MAXDIM;
constexpr int core_max_err_msg_len = MAX_ERR_MSG_LEN;

#undef al_status_t
#undef al_context_info
#undef al_begin_dataentry_action
#undef al_close_pulse
#undef al_begin_global_action
#undef al_begin_slice_action
#undef al_begin_timerange_action
#undef al_begin_arraystruct_action
#undef al_end_action
#undef al_read_data
#undef al_write_data
#undef al_delete_data
#undef al_iterate_over_arraystruct
#undef al_get_occurrences
#undef al_list_filled_paths
#undef al_register_plugin
#undef al_unregister_plugin
#undef al_bind_plugin
#undef al_unbind_plugin
#undef al_bind_readback_plugins
#undef al_unbind_readback_plugins
#undef al_is_plugin_registered
#undef al_write_plugins_metadata
#undef al_setvalue_parameter_plugin
#undef al_setvalue_int_scalar_parameter_plugin
#undef al_setvalue_double_scalar_parameter_plugin
#undef al_plugin_begin_global_action
#undef al_plugin_begin_slice_action
#undef al_plugin_begin_timerange_action
#undef al_plugin_begin_arraystruct_action
#undef al_plugin_end_action
#undef al_plugin_read_data
#undef al_plugin_write_data
#undef MAXDIM
#undef MAX_ERR_MSG_LEN

#include <imas_mvdd_loader.h>

template <typename CoreFunction, typename ShimFunction>
struct same_status_function : std::false_type {};

template <typename... CoreArgs, typename... ShimArgs>
struct same_status_function<core_al_status_t (*)(CoreArgs...),
                            al_status_t (*)(ShimArgs...)>
    : std::is_same<std::tuple<CoreArgs...>, std::tuple<ShimArgs...>> {};

#define CHECK_SIGNATURE(name)                                                  \
    static_assert(                                                            \
        same_status_function<decltype(&core_##name), decltype(&name)>::value, \
        #name " must exactly match IMAS-Core's parameter types")

static_assert(sizeof(al_status_t) == sizeof(core_al_status_t));
static_assert(alignof(al_status_t) == alignof(core_al_status_t));
static_assert(offsetof(al_status_t, code) == offsetof(core_al_status_t, code));
static_assert(offsetof(al_status_t, message) ==
              offsetof(core_al_status_t, message));
static_assert(MAXDIM == core_maxdim);
static_assert(MAX_ERR_MSG_LEN == core_max_err_msg_len);

CHECK_SIGNATURE(al_context_info);
using core_begin_dataentry_action_c =
    core_al_status_t (*)(const char*, int, int*);
static_assert(same_status_function<
              decltype(static_cast<core_begin_dataentry_action_c>(
                  &core_al_begin_dataentry_action)),
              decltype(&al_begin_dataentry_action)>::value,
              "al_begin_dataentry_action must match IMAS-Core's C overload");
CHECK_SIGNATURE(al_close_pulse);
CHECK_SIGNATURE(al_begin_global_action);
CHECK_SIGNATURE(al_begin_slice_action);
CHECK_SIGNATURE(al_begin_timerange_action);
CHECK_SIGNATURE(al_begin_arraystruct_action);
CHECK_SIGNATURE(al_end_action);
CHECK_SIGNATURE(al_read_data);
CHECK_SIGNATURE(al_write_data);
CHECK_SIGNATURE(al_delete_data);
CHECK_SIGNATURE(al_iterate_over_arraystruct);
CHECK_SIGNATURE(al_get_occurrences);
CHECK_SIGNATURE(al_list_filled_paths);
CHECK_SIGNATURE(al_register_plugin);
CHECK_SIGNATURE(al_unregister_plugin);
CHECK_SIGNATURE(al_bind_plugin);
CHECK_SIGNATURE(al_unbind_plugin);
CHECK_SIGNATURE(al_bind_readback_plugins);
CHECK_SIGNATURE(al_unbind_readback_plugins);
CHECK_SIGNATURE(al_is_plugin_registered);
CHECK_SIGNATURE(al_write_plugins_metadata);
CHECK_SIGNATURE(al_setvalue_parameter_plugin);
CHECK_SIGNATURE(al_setvalue_int_scalar_parameter_plugin);
CHECK_SIGNATURE(al_setvalue_double_scalar_parameter_plugin);
CHECK_SIGNATURE(al_plugin_begin_global_action);
CHECK_SIGNATURE(al_plugin_begin_slice_action);
CHECK_SIGNATURE(al_plugin_begin_arraystruct_action);
CHECK_SIGNATURE(al_plugin_end_action);
CHECK_SIGNATURE(al_plugin_read_data);
CHECK_SIGNATURE(al_plugin_write_data);

// al_plugin_begin_timerange_action is intentionally excluded: the real header
// declaration has no matching exported IMAS-Core symbol, so the shim must not
// declare or export it (issue #7).

int main() { return 0; }
