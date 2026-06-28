use crate::remote::config::RemoteConfig;

pub const DEFAULT_REMOTE_SERVER_URL: &str = "https://remote.niuma.example";

pub fn default_remote_config() -> RemoteConfig {
    RemoteConfig::default_for_server(DEFAULT_REMOTE_SERVER_URL)
}

pub fn normalize_server_url(value: &str) -> Result<String, String> {
    let trimmed = value.trim().trim_end_matches('/').to_string();
    if trimmed.is_empty() {
        return Err("远程服务地址不能为空".to_string());
    }
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        return Err("远程服务地址必须以 http:// 或 https:// 开头".to_string());
    }
    Ok(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_server_url() {
        assert_eq!(
            normalize_server_url(" https://remote.example.com/ ").unwrap(),
            "https://remote.example.com"
        );
    }

    #[test]
    fn rejects_missing_scheme() {
        assert_eq!(
            normalize_server_url("remote.example.com").unwrap_err(),
            "远程服务地址必须以 http:// 或 https:// 开头"
        );
    }
}
