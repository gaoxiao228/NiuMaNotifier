use std::path::PathBuf;
use tauri::Manager;

const CODEX_PLUGIN_BINARY_NAME: &str = "niuma-codex-plugin";
const BARK_PLUGIN_BINARY_NAME: &str = "niuma-plugin-bark";
const NTFY_PLUGIN_BINARY_NAME: &str = "niuma-plugin-ntfy";

pub fn configure_builtin_codex_plugin_command(app: &tauri::App) {
    configure_builtin_plugin_command(
        app,
        niuma_core::plugin::CODEX_PLUGIN_COMMAND_ENV,
        CODEX_PLUGIN_BINARY_NAME,
    );
}

pub fn configure_builtin_bark_plugin_command(app: &tauri::App) {
    configure_builtin_plugin_command(
        app,
        niuma_core::plugin::BARK_PLUGIN_COMMAND_ENV,
        BARK_PLUGIN_BINARY_NAME,
    );
}

pub fn configure_builtin_ntfy_plugin_command(app: &tauri::App) {
    configure_builtin_plugin_command(
        app,
        niuma_core::plugin::NTFY_PLUGIN_COMMAND_ENV,
        NTFY_PLUGIN_BINARY_NAME,
    );
}

fn configure_builtin_plugin_command(app: &tauri::App, env_key: &str, binary_name: &str) {
    if std::env::var_os(env_key).is_some() {
        return;
    }
    let resource_dir = app.path().resource_dir().ok();
    let current_exe = std::env::current_exe().ok();
    if let Some(command) =
        resolve_builtin_plugin_command(binary_name, resource_dir.as_ref(), current_exe.as_ref())
    {
        // 只设置命令路径，不直接启动插件；启动仍由通用插件管理器按 manifest 完成。
        std::env::set_var(env_key, command.to_string_lossy().to_string());
    }
}

pub(crate) fn resolve_builtin_plugin_command(
    binary_name: &str,
    resource_dir: Option<&PathBuf>,
    current_exe: Option<&PathBuf>,
) -> Option<PathBuf> {
    let executable_name = niuma_core::platform::executable::executable_name(binary_name);
    let mut candidates = Vec::new();
    if let Some(resource_dir) = resource_dir {
        // 打包资源放在 resource_dir/bin，兼容旧版本曾经放在 resource_dir 根目录的情况。
        candidates.push(resource_dir.join("bin").join(&executable_name));
        candidates.push(resource_dir.join(&executable_name));
    }
    if let Some(current_exe) = current_exe {
        if let Some(exe_dir) = current_exe.parent() {
            candidates.push(exe_dir.join(&executable_name));
        }
    }
    candidates.into_iter().find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_plugin_command_prefers_resource_binary() {
        let temp = tempfile::tempdir().unwrap();
        let resource_dir = temp.path().join("resources");
        let resource_bin_dir = resource_dir.join("bin");
        let exe_dir = temp.path().join("bin");
        std::fs::create_dir_all(&resource_bin_dir).unwrap();
        std::fs::create_dir_all(&exe_dir).unwrap();
        let executable_name =
            niuma_core::platform::executable::executable_name(BARK_PLUGIN_BINARY_NAME);
        let resource_binary = resource_bin_dir.join(&executable_name);
        let exe_binary = exe_dir.join(&executable_name);
        std::fs::write(&resource_binary, "").unwrap();
        std::fs::write(&exe_binary, "").unwrap();

        let command = resolve_builtin_plugin_command(
            BARK_PLUGIN_BINARY_NAME,
            Some(&resource_dir),
            Some(&exe_dir.join("NiumaNotifier")),
        );

        assert_eq!(command.as_deref(), Some(resource_binary.as_path()));
    }

    #[test]
    fn builtin_ntfy_plugin_command_prefers_resource_binary() {
        let temp = tempfile::tempdir().unwrap();
        let resource_dir = temp.path().join("resources");
        let resource_bin_dir = resource_dir.join("bin");
        std::fs::create_dir_all(&resource_bin_dir).unwrap();
        let executable_name =
            niuma_core::platform::executable::executable_name(NTFY_PLUGIN_BINARY_NAME);
        let resource_binary = resource_bin_dir.join(&executable_name);
        std::fs::write(&resource_binary, "").unwrap();

        let command =
            resolve_builtin_plugin_command(NTFY_PLUGIN_BINARY_NAME, Some(&resource_dir), None);

        assert_eq!(command.as_deref(), Some(resource_binary.as_path()));
    }

    #[test]
    fn builtin_plugin_command_falls_back_to_current_exe_dir() {
        let temp = tempfile::tempdir().unwrap();
        let exe_dir = temp.path().join("bin");
        std::fs::create_dir_all(&exe_dir).unwrap();
        let executable_name =
            niuma_core::platform::executable::executable_name(BARK_PLUGIN_BINARY_NAME);
        let exe_binary = exe_dir.join(&executable_name);
        std::fs::write(&exe_binary, "").unwrap();

        let command = resolve_builtin_plugin_command(
            BARK_PLUGIN_BINARY_NAME,
            Some(&temp.path().join("missing-resources")),
            Some(&exe_dir.join("NiumaNotifier")),
        );

        assert_eq!(command.as_deref(), Some(exe_binary.as_path()));
    }
}
