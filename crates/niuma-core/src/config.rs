use std::path::PathBuf;

// 集中管理跨 crate 复用的本机环境变量，避免 API、CLI、桌面端各自复制默认值。
pub const DEFAULT_LOCAL_API_ADDR: &str = "127.0.0.1:27874";

pub fn state_path() -> PathBuf {
    state_path_from_env(std::env::var("NIUMA_STATE_PATH").ok().as_deref())
}

pub fn state_path_from_env(value: Option<&str>) -> PathBuf {
    value.map(PathBuf::from).unwrap_or_else(default_state_path)
}

fn default_state_path() -> PathBuf {
    crate::platform::paths::app_data_dir().join("state.sqlite")
}

pub fn local_api_addr() -> String {
    local_api_addr_from_env(std::env::var("NIUMA_LOCAL_API_ADDR").ok().as_deref())
}

pub fn local_api_addr_from_env(value: Option<&str>) -> String {
    value.unwrap_or(DEFAULT_LOCAL_API_ADDR).to_string()
}

pub fn manual_test_enabled(debug_assertions: bool) -> bool {
    manual_test_enabled_from_env(
        debug_assertions,
        std::env::var("NIUMA_ENABLE_MANUAL_TEST").ok().as_deref(),
    )
}

pub fn manual_test_enabled_from_env(debug_assertions: bool, value: Option<&str>) -> bool {
    debug_assertions || env_flag_enabled(value)
}

pub fn codex_home() -> PathBuf {
    crate::platform::paths::codex_home()
}

pub fn codex_home_from_env(codex_home: Option<&str>, home: Option<&str>) -> PathBuf {
    crate::platform::paths::codex_home_from_env(codex_home, home)
}

pub fn watcher_debug_enabled() -> bool {
    env_flag_enabled(std::env::var("NIUMA_CODEX_WATCHER_DEBUG").ok().as_deref())
}

pub fn watcher_trace_enabled() -> bool {
    env_flag_enabled(std::env::var("NIUMA_CODEX_WATCHER_TRACE").ok().as_deref())
}

pub fn env_flag_enabled(value: Option<&str>) -> bool {
    value == Some("1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_path_uses_override_or_default_sqlite_path() {
        assert_eq!(
            state_path_from_env(Some("/tmp/custom-state.sqlite")),
            PathBuf::from("/tmp/custom-state.sqlite")
        );
        assert_eq!(
            state_path_from_env(None),
            crate::platform::paths::app_data_dir().join("state.sqlite")
        );
    }

    #[test]
    fn local_api_addr_uses_override_or_default_loopback_port() {
        assert_eq!(
            local_api_addr_from_env(Some("127.0.0.1:30000")),
            "127.0.0.1:30000"
        );
        assert_eq!(local_api_addr_from_env(None), "127.0.0.1:27874");
    }

    #[test]
    fn manual_test_is_enabled_in_debug_or_explicit_release_override() {
        assert!(manual_test_enabled_from_env(true, None));
        assert!(manual_test_enabled_from_env(false, Some("1")));
        assert!(!manual_test_enabled_from_env(false, Some("true")));
        assert!(!manual_test_enabled_from_env(false, None));
    }

    #[test]
    fn codex_home_uses_override_home_or_relative_fallback() {
        assert_eq!(
            codex_home_from_env(Some("/tmp/codex"), Some("/Users/demo")),
            PathBuf::from("/tmp/codex")
        );
        assert_eq!(
            codex_home_from_env(None, Some("/Users/demo")),
            PathBuf::from("/Users/demo").join(".codex")
        );
        assert_eq!(codex_home_from_env(None, None), PathBuf::from(".codex"));
    }

    #[test]
    fn env_flag_accepts_only_one() {
        assert!(env_flag_enabled(Some("1")));
        assert!(!env_flag_enabled(Some("true")));
        assert!(!env_flag_enabled(Some("")));
        assert!(!env_flag_enabled(None));
    }
}
