use super::{parse_plugin_manifest, PluginManifest, PluginSource};

pub const CODEX_PLUGIN_COMMAND_ENV: &str = "NIUMA_CODEX_PLUGIN_COMMAND";
pub const BARK_PLUGIN_COMMAND_ENV: &str = "NIUMA_BARK_PLUGIN_COMMAND";
pub const NTFY_PLUGIN_COMMAND_ENV: &str = "NIUMA_NTFY_PLUGIN_COMMAND";
pub const BUILTIN_CODEX_PLUGIN_ID: &str = "builtin-codex";
pub const BUILTIN_BARK_PLUGIN_ID: &str = "builtin-bark";
pub const BUILTIN_NTFY_PLUGIN_ID: &str = "builtin-ntfy";

const CODEX_PLUGIN_COMMAND: &str = "niuma-codex-plugin";
const BARK_PLUGIN_COMMAND: &str = "niuma-plugin-bark";
const NTFY_PLUGIN_COMMAND: &str = "niuma-plugin-ntfy";
const BUILTIN_CODEX_PLUGIN_MANIFEST_JSON: &str =
    include_str!("../../../../builtin-plugins/codex/plugin.json");
const BUILTIN_BARK_PLUGIN_MANIFEST_JSON: &str =
    include_str!("../../../../builtin-plugins/bark/plugin.json");
const BUILTIN_NTFY_PLUGIN_MANIFEST_JSON: &str =
    include_str!("../../../../builtin-plugins/ntfy/plugin.json");

pub fn builtin_codex_manifest() -> PluginManifest {
    let mut manifest = parse_plugin_manifest(BUILTIN_CODEX_PLUGIN_MANIFEST_JSON)
        .expect("内置 Codex 插件 manifest 必须是有效 plugin.json");
    manifest.source = PluginSource::Builtin;
    manifest.command = Some(builtin_codex_plugin_command(manifest.command));
    // 内置插件的 binary 路径由桌面端/环境变量解析，不使用源码目录作为运行目录。
    manifest.base_dir = None;
    manifest
}

pub fn builtin_bark_manifest() -> PluginManifest {
    let mut manifest = parse_plugin_manifest(BUILTIN_BARK_PLUGIN_MANIFEST_JSON)
        .expect("内置 Bark 插件 manifest 必须是有效 plugin.json");
    manifest.source = PluginSource::Builtin;
    manifest.command = Some(builtin_plugin_command(
        BARK_PLUGIN_COMMAND_ENV,
        manifest.command,
        BARK_PLUGIN_COMMAND,
    ));
    manifest.base_dir = None;
    manifest
}

pub fn builtin_ntfy_manifest() -> PluginManifest {
    let mut manifest = parse_plugin_manifest(BUILTIN_NTFY_PLUGIN_MANIFEST_JSON)
        .expect("内置 ntfy 插件 manifest 必须是有效 plugin.json");
    manifest.source = PluginSource::Builtin;
    manifest.command = Some(builtin_plugin_command(
        NTFY_PLUGIN_COMMAND_ENV,
        manifest.command,
        NTFY_PLUGIN_COMMAND,
    ));
    manifest.base_dir = None;
    manifest
}

fn builtin_codex_plugin_command(default_command: Option<String>) -> String {
    // 桌面端或打包脚本可覆盖命令路径；默认值来自内置 plugin.json，最后回退到普通裸命令。
    builtin_plugin_command(
        CODEX_PLUGIN_COMMAND_ENV,
        default_command,
        CODEX_PLUGIN_COMMAND,
    )
}

fn builtin_plugin_command(
    env_key: &str,
    default_command: Option<String>,
    fallback_command: &str,
) -> String {
    std::env::var(env_key)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| default_command.filter(|value| !value.trim().is_empty()))
        .unwrap_or_else(|| fallback_command.to_string())
}
