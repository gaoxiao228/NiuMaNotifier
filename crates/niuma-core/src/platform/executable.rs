use std::path::Path;

pub fn executable_name(base_name: &str) -> String {
    executable_name_for_os(base_name, cfg!(windows))
}

pub fn executable_name_for_os(base_name: &str, windows: bool) -> String {
    if windows {
        format!("{base_name}.exe")
    } else {
        base_name.to_string()
    }
}

pub fn command_on_path(base_name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            path_contains_executable(std::env::split_paths(&paths), base_name, cfg!(windows))
        })
        .unwrap_or(false)
}

pub fn path_contains_executable<I, P>(paths: I, base_name: &str, windows: bool) -> bool
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let executable = executable_name_for_os(base_name, windows);
    paths
        .into_iter()
        .any(|dir| dir.as_ref().join(&executable).is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executable_name_adds_exe_suffix_only_on_windows() {
        assert_eq!(executable_name_for_os("niuma", true), "niuma.exe");
        assert_eq!(executable_name_for_os("niuma", false), "niuma");
    }

    #[test]
    fn path_contains_executable_finds_platform_specific_binary() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("niuma.exe"), "").unwrap();

        assert!(path_contains_executable(
            [temp.path().to_path_buf()],
            "niuma",
            true
        ));
        assert!(!path_contains_executable(
            [temp.path().to_path_buf()],
            "niuma",
            false
        ));
    }

    #[test]
    fn path_contains_executable_returns_false_when_missing() {
        let temp = tempfile::tempdir().unwrap();

        assert!(!path_contains_executable(
            [temp.path().to_path_buf()],
            "niuma",
            cfg!(windows)
        ));
    }
}
