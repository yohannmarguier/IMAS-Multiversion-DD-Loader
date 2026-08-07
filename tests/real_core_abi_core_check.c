/* Compile the expected ABI contract against IMAS-Core's public C header. */

#include <al_lowlevel.h>

#include "real_core_abi_contract.h"

CHECK_ABI_STATUS_LAYOUT();
CHECK_ABI_FUNCTION(al_context_info, AbiContextInfoFn);
CHECK_ABI_FUNCTION(al_get_backendID, AbiGetBackendIdFn);
CHECK_ABI_FUNCTION(al_build_uri_from_legacy_parameters,
                   AbiBuildUriFromLegacyParametersFn);
CHECK_ABI_FUNCTION(const2str, AbiStringLookupFn);
CHECK_ABI_FUNCTION(err2str, AbiStringLookupFn);
CHECK_ABI_FUNCTION(getALVersion, AbiVersionAccessorFn);
CHECK_ABI_FUNCTION(getDDVersion, AbiVersionAccessorFn);
CHECK_ABI_FUNCTION(al_begin_dataentry_action, AbiBeginDataentryActionFn);
CHECK_ABI_FUNCTION(al_close_pulse, AbiClosePulseFn);
CHECK_ABI_FUNCTION(al_begin_global_action, AbiBeginGlobalActionFn);
CHECK_ABI_FUNCTION(al_begin_slice_action, AbiBeginSliceActionFn);
CHECK_ABI_FUNCTION(al_begin_timerange_action, AbiBeginTimerangeActionFn);
CHECK_ABI_FUNCTION(al_begin_arraystruct_action, AbiBeginArraystructActionFn);
CHECK_ABI_FUNCTION(al_end_action, AbiEndActionFn);
CHECK_ABI_FUNCTION(al_read_data, AbiReadDataFn);
CHECK_ABI_FUNCTION(al_write_data, AbiWriteDataFn);
CHECK_ABI_FUNCTION(al_delete_data, AbiDeleteDataFn);
CHECK_ABI_FUNCTION(al_iterate_over_arraystruct, AbiIterateOverArraystructFn);
CHECK_ABI_FUNCTION(al_get_occurrences, AbiGetOccurrencesFn);
CHECK_ABI_FUNCTION(al_list_filled_paths, AbiListFilledPathsFn);
CHECK_ABI_FUNCTION(al_register_plugin, AbiPluginNameFn);
CHECK_ABI_FUNCTION(al_unregister_plugin, AbiPluginNameFn);
CHECK_ABI_FUNCTION(al_bind_plugin, AbiBindPluginFn);
CHECK_ABI_FUNCTION(al_unbind_plugin, AbiBindPluginFn);
CHECK_ABI_FUNCTION(al_bind_readback_plugins, AbiPluginContextFn);
CHECK_ABI_FUNCTION(al_unbind_readback_plugins, AbiPluginContextFn);
CHECK_ABI_FUNCTION(al_is_plugin_registered, AbiIsPluginRegisteredFn);
CHECK_ABI_FUNCTION(al_write_plugins_metadata, AbiPluginContextFn);
CHECK_ABI_FUNCTION(al_setvalue_parameter_plugin, AbiSetvalueParameterPluginFn);
CHECK_ABI_FUNCTION(al_setvalue_int_scalar_parameter_plugin,
                   AbiSetvalueIntScalarParameterPluginFn);
CHECK_ABI_FUNCTION(al_setvalue_double_scalar_parameter_plugin,
                   AbiSetvalueDoubleScalarParameterPluginFn);
CHECK_ABI_FUNCTION(al_plugin_begin_global_action, AbiBeginGlobalActionFn);
CHECK_ABI_FUNCTION(al_plugin_begin_slice_action, AbiBeginSliceActionFn);
CHECK_ABI_FUNCTION(al_plugin_begin_arraystruct_action,
                   AbiBeginArraystructActionFn);
CHECK_ABI_FUNCTION(al_plugin_end_action, AbiEndActionFn);
CHECK_ABI_FUNCTION(al_plugin_read_data, AbiReadDataFn);
CHECK_ABI_FUNCTION(al_plugin_write_data, AbiWriteDataFn);
