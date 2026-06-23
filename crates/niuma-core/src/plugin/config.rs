use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::PluginManifest;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginConfigFieldType {
    String,
    Secret,
    Url,
    Number,
    Boolean,
    Select,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PluginConfigField {
    pub key: String,
    #[serde(rename = "type")]
    pub field_type: PluginConfigFieldType,
    pub label: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Value,
    #[serde(default)]
    pub options: Vec<String>,
}

pub fn plugin_config_defaults(
    manifest: &PluginManifest,
) -> serde_json::Map<String, serde_json::Value> {
    manifest
        .config_schema
        .iter()
        .filter(|field| !field.default.is_null())
        .map(|field| (field.key.clone(), field.default.clone()))
        .collect()
}

pub fn merge_plugin_config_with_defaults(
    manifest: &PluginManifest,
    config: serde_json::Map<String, serde_json::Value>,
) -> serde_json::Map<String, serde_json::Value> {
    let mut merged = plugin_config_defaults(manifest);
    for (key, value) in config {
        merged.insert(key, value);
    }
    merged
}

pub fn resolve_plugin_config(
    manifest: &PluginManifest,
    stored_config: Option<serde_json::Map<String, serde_json::Value>>,
) -> serde_json::Map<String, serde_json::Value> {
    let config = stored_config.unwrap_or_default();
    merge_plugin_config_with_defaults(manifest, config)
}

pub fn validate_plugin_config(
    manifest: &PluginManifest,
    config: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    for field in &manifest.config_schema {
        let Some(value) = config.get(&field.key) else {
            if field.required {
                return Err(format!("{} 不能为空", field.label));
            }
            continue;
        };
        if !plugin_config_value_matches(field, value) {
            return Err(format!("{} 类型无效", field.label));
        }
        if field.required && plugin_config_value_is_empty(value) {
            return Err(format!("{} 不能为空", field.label));
        }
    }
    Ok(())
}

pub(super) fn validate_config_schema(
    manifest_id: &str,
    fields: &[PluginConfigField],
) -> Result<(), String> {
    let mut keys = std::collections::BTreeSet::new();
    for field in fields {
        if field.key.trim().is_empty() {
            return Err(format!("插件配置项 key 不能为空：{manifest_id}"));
        }
        if field.label.trim().is_empty() {
            return Err(format!("插件配置项 label 不能为空：{manifest_id}"));
        }
        if !keys.insert(field.key.clone()) {
            return Err(format!("插件配置项 key 重复：{manifest_id}:{}", field.key));
        }
    }
    Ok(())
}

fn plugin_config_value_matches(field: &PluginConfigField, value: &Value) -> bool {
    match field.field_type {
        PluginConfigFieldType::String
        | PluginConfigFieldType::Secret
        | PluginConfigFieldType::Url => value.is_string(),
        PluginConfigFieldType::Number => value.is_number(),
        PluginConfigFieldType::Boolean => value.is_boolean(),
        PluginConfigFieldType::Select => {
            let Some(value) = value.as_str() else {
                return false;
            };
            field.options.is_empty() || field.options.iter().any(|option| option == value)
        }
    }
}

fn plugin_config_value_is_empty(value: &Value) -> bool {
    match value {
        Value::String(text) => text.trim().is_empty(),
        Value::Null => true,
        _ => false,
    }
}
