use super::{config, PluginCapability, PluginKind, PluginManifest};

pub(crate) fn parse_plugin_manifest(content: &str) -> Result<PluginManifest, String> {
    let manifest: PluginManifest = serde_json::from_str(content)
        .map_err(|error| format!("解析插件 manifest 失败：{error}"))?;
    validate_plugin_manifest(&manifest)?;
    Ok(manifest)
}

fn validate_plugin_manifest(manifest: &PluginManifest) -> Result<(), String> {
    if manifest.kind == PluginKind::Tool && manifest.tool_id.is_none() {
        return Err(format!("工具插件缺少 tool_id：{}", manifest.id));
    }
    if manifest.kind != PluginKind::Tool {
        for capability in non_tool_forbidden_provider_capabilities(&manifest.capabilities) {
            return Err(format!(
                "非工具插件不能声明 {}：{}",
                plugin_capability_id(&capability),
                manifest.id
            ));
        }
    }
    if manifest
        .capabilities
        .contains(&PluginCapability::ToolSessionDetailProvider)
        && !manifest
            .capabilities
            .contains(&PluginCapability::ToolSessionListProvider)
    {
        return Err(format!(
            "tool_session_detail_provider 必须同时声明 tool_session_list_provider：{}",
            manifest.id
        ));
    }
    config::validate_config_schema(&manifest.id, &manifest.config_schema)
}

fn non_tool_forbidden_provider_capabilities(
    capabilities: &[PluginCapability],
) -> Vec<PluginCapability> {
    // 这些能力会代表具体 tool 上报或提供数据，必须绑定 tool_id 后才能安全路由。
    provider_capabilities(capabilities)
}

pub(crate) fn provider_capabilities(capabilities: &[PluginCapability]) -> Vec<PluginCapability> {
    [
        PluginCapability::EventWatcher,
        PluginCapability::ToolSessionListProvider,
        PluginCapability::ToolSessionDetailProvider,
    ]
    .into_iter()
    .filter(|capability| capabilities.contains(capability))
    .collect()
}

pub(crate) fn plugin_capability_id(capability: &PluginCapability) -> &'static str {
    match capability {
        PluginCapability::EventWatcher => "event_watcher",
        PluginCapability::EventConsumer => "event_consumer",
        PluginCapability::ApprovalHandler => "approval_handler",
        PluginCapability::NotificationTest => "notification_test",
        PluginCapability::StateConsumer => "state_consumer",
        PluginCapability::ToolSessionListProvider => "tool_session_list_provider",
        PluginCapability::ToolSessionDetailProvider => "tool_session_detail_provider",
        PluginCapability::ToolSessionListReader => "tool_session_list_reader",
        PluginCapability::ToolSessionDetailReader => "tool_session_detail_reader",
    }
}
