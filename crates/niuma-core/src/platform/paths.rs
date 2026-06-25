use std::path::PathBuf;

const APP_DIR_NAME: &str = "NiumaNotifier";

pub fn codex_home() -> PathBuf {
    codex_home_from_env(
        std::env::var("CODEX_HOME").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
    )
}

pub fn codex_home_from_env(codex_home: Option<&str>, home: Option<&str>) -> PathBuf {
    codex_home
        .map(PathBuf::from)
        .or_else(|| home.map(|home| PathBuf::from(home).join(".codex")))
        .unwrap_or_else(|| PathBuf::from(".codex"))
}

pub fn app_data_dir() -> PathBuf {
    // 应用状态和用户配置需要长期保留，不能放进系统临时目录。
    dirs::data_dir()
        .map(|path| path.join(APP_DIR_NAME))
        .unwrap_or_else(|| {
            app_data_dir_from_env(
                std::env::var("APPDATA").ok().as_deref(),
                std::env::var("XDG_DATA_HOME").ok().as_deref(),
                std::env::var("HOME").ok().as_deref(),
            )
        })
}

pub fn codex_managed_registry_path() -> PathBuf {
    app_data_dir().join("managed-sessions").join("codex.json")
}

pub fn app_data_dir_from_env(
    appdata: Option<&str>,
    xdg_data_home: Option<&str>,
    home: Option<&str>,
) -> PathBuf {
    platform_app_data_base(appdata, xdg_data_home, home).join(APP_DIR_NAME)
}

#[cfg(target_os = "macos")]
fn platform_app_data_base(
    _appdata: Option<&str>,
    _xdg_data_home: Option<&str>,
    home: Option<&str>,
) -> PathBuf {
    home.map(PathBuf::from)
        .map(|path| path.join("Library").join("Application Support"))
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(target_os = "windows")]
fn platform_app_data_base(
    appdata: Option<&str>,
    _xdg_data_home: Option<&str>,
    home: Option<&str>,
) -> PathBuf {
    appdata
        .map(PathBuf::from)
        .or_else(|| home.map(|home| PathBuf::from(home).join("AppData").join("Roaming")))
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
fn platform_app_data_base(
    _appdata: Option<&str>,
    xdg_data_home: Option<&str>,
    home: Option<&str>,
) -> PathBuf {
    xdg_data_home
        .map(PathBuf::from)
        .or_else(|| home.map(|home| PathBuf::from(home).join(".local").join("share")))
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_home_prefers_explicit_override() {
        assert_eq!(
            codex_home_from_env(Some("/tmp/custom-codex"), Some("/Users/demo")),
            PathBuf::from("/tmp/custom-codex")
        );
    }

    #[test]
    fn codex_home_uses_home_dot_codex_when_override_is_missing() {
        assert_eq!(
            codex_home_from_env(None, Some("/Users/demo")),
            PathBuf::from("/Users/demo").join(".codex")
        );
    }

    #[test]
    fn codex_home_falls_back_to_relative_dot_codex_without_home() {
        assert_eq!(codex_home_from_env(None, None), PathBuf::from(".codex"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn app_data_dir_uses_macos_application_support() {
        assert_eq!(
            app_data_dir_from_env(None, None, Some("/Users/demo")),
            PathBuf::from("/Users/demo")
                .join("Library")
                .join("Application Support")
                .join("NiumaNotifier")
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn app_data_dir_uses_windows_roaming_appdata() {
        assert_eq!(
            app_data_dir_from_env(Some(r"C:\Users\demo\AppData\Roaming"), None, None),
            PathBuf::from(r"C:\Users\demo\AppData\Roaming").join("NiumaNotifier")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn app_data_dir_uses_linux_xdg_data_home() {
        assert_eq!(
            app_data_dir_from_env(None, Some("/home/demo/.local/share"), Some("/home/demo")),
            PathBuf::from("/home/demo/.local/share").join("NiumaNotifier")
        );
    }
}
