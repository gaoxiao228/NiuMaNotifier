mod builtin;
mod config;
mod enablement;
mod filesystem;
mod manifest;
mod registry;
mod runtime_state;
mod validation;

pub use builtin::{
    builtin_bark_manifest, builtin_claude_code_manifest, builtin_codex_manifest,
    builtin_ntfy_manifest, BARK_PLUGIN_COMMAND_ENV, BUILTIN_BARK_PLUGIN_ID,
    BUILTIN_CLAUDE_CODE_PLUGIN_ID, BUILTIN_CODEX_PLUGIN_ID, BUILTIN_NTFY_PLUGIN_ID,
    CLAUDE_CODE_PLUGIN_COMMAND_ENV, CODEX_PLUGIN_COMMAND_ENV, NTFY_PLUGIN_COMMAND_ENV,
};
pub use config::{
    merge_plugin_config_with_defaults, plugin_config_defaults, resolve_plugin_config,
    validate_plugin_config, PluginConfigField, PluginConfigFieldType,
};
pub use enablement::{
    default_non_tool_plugin_enabled, default_plugin_enabled, listener_config_after_plugin_removed,
    plugin_uses_listener_config, save_plugin_enabled_state,
};
pub use filesystem::{
    current_plugin_registry, default_user_plugin_dir, import_external_plugin_dir,
    remove_external_plugin,
};
pub use manifest::{PluginCapability, PluginKind, PluginManifest, PluginSource};
pub use registry::{
    PluginImportResult, PluginManagementAction, PluginManagementActionKind,
    PluginManagementActionStatusLevel, PluginManagementItem, PluginRegistry, PluginRemoveResult,
    ToolPluginInfo,
};
pub use runtime_state::{PluginRuntimeState, PluginRuntimeStatus};
pub(super) use validation::parse_plugin_manifest;

#[cfg(test)]
mod tests;
