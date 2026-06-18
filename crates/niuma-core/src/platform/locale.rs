use std::sync::atomic::{AtomicI8, Ordering};

const LANGUAGE_SYSTEM_INDEX: i8 = 0;

static LANGUAGE_PREFERENCE_INDEX: AtomicI8 = AtomicI8::new(LANGUAGE_SYSTEM_INDEX);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SystemLanguage {
    ZhCn,
    ZhTw,
    En,
    Ja,
    Ko,
    De,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LanguagePreference {
    System,
    Fixed(SystemLanguage),
}

impl LanguagePreference {
    pub fn resolve(self) -> SystemLanguage {
        match self {
            Self::System => system_language(),
            Self::Fixed(language) => language,
        }
    }

    pub fn storage_id(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Fixed(language) => language.storage_id(),
        }
    }

    pub fn from_storage_id(value: &str) -> Option<Self> {
        if value == "system" {
            return Some(Self::System);
        }
        SystemLanguage::from_storage_id(value).map(Self::Fixed)
    }
}

impl SystemLanguage {
    pub fn storage_id(self) -> &'static str {
        match self {
            Self::ZhCn => "zh-CN",
            Self::ZhTw => "zh-TW",
            Self::En => "en",
            Self::Ja => "ja",
            Self::Ko => "ko",
            Self::De => "de",
        }
    }

    pub fn from_storage_id(value: &str) -> Option<Self> {
        match value {
            "zh-CN" => Some(Self::ZhCn),
            "zh-TW" => Some(Self::ZhTw),
            "en" => Some(Self::En),
            "ja" => Some(Self::Ja),
            "ko" => Some(Self::Ko),
            "de" => Some(Self::De),
            _ => None,
        }
    }
}

pub fn system_language() -> SystemLanguage {
    let env_languages = environment_language_tags();
    let platform_languages = platform_language_tags();
    let env_refs = env_languages.iter().map(String::as_str).collect::<Vec<_>>();
    let platform_refs = platform_languages
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    preferred_language_from_sources(&env_refs, &platform_refs)
}

pub fn active_language() -> SystemLanguage {
    active_language_preference().resolve()
}

pub fn active_language_preference() -> LanguagePreference {
    language_preference_from_index(LANGUAGE_PREFERENCE_INDEX.load(Ordering::SeqCst))
}

pub fn set_active_language_preference(preference: LanguagePreference) {
    LANGUAGE_PREFERENCE_INDEX.store(language_preference_index(preference), Ordering::SeqCst);
}

pub fn preferred_language_from_sources(
    env_languages: &[&str],
    macos_apple_languages: &[&str],
) -> SystemLanguage {
    macos_apple_languages
        .iter()
        .chain(env_languages.iter())
        .find_map(|value| language_from_tag(value))
        .unwrap_or(SystemLanguage::En)
}

fn language_preference_index(preference: LanguagePreference) -> i8 {
    match preference {
        LanguagePreference::System => LANGUAGE_SYSTEM_INDEX,
        LanguagePreference::Fixed(SystemLanguage::ZhCn) => 1,
        LanguagePreference::Fixed(SystemLanguage::ZhTw) => 2,
        LanguagePreference::Fixed(SystemLanguage::En) => 3,
        LanguagePreference::Fixed(SystemLanguage::Ja) => 4,
        LanguagePreference::Fixed(SystemLanguage::Ko) => 5,
        LanguagePreference::Fixed(SystemLanguage::De) => 6,
    }
}

fn language_preference_from_index(value: i8) -> LanguagePreference {
    match value {
        1 => LanguagePreference::Fixed(SystemLanguage::ZhCn),
        2 => LanguagePreference::Fixed(SystemLanguage::ZhTw),
        3 => LanguagePreference::Fixed(SystemLanguage::En),
        4 => LanguagePreference::Fixed(SystemLanguage::Ja),
        5 => LanguagePreference::Fixed(SystemLanguage::Ko),
        6 => LanguagePreference::Fixed(SystemLanguage::De),
        _ => LanguagePreference::System,
    }
}

pub fn language_from_tag(value: &str) -> Option<SystemLanguage> {
    let normalized = value.replace('_', "-").to_lowercase();
    if normalized.starts_with("zh-cn") || normalized.contains("hans") {
        return Some(SystemLanguage::ZhCn);
    }
    if normalized.starts_with("zh-tw")
        || normalized.starts_with("zh-hk")
        || normalized.starts_with("zh-mo")
        || normalized.contains("hant")
    {
        return Some(SystemLanguage::ZhTw);
    }
    if normalized.starts_with("ja") {
        return Some(SystemLanguage::Ja);
    }
    if normalized.starts_with("ko") {
        return Some(SystemLanguage::Ko);
    }
    if normalized.starts_with("de") {
        return Some(SystemLanguage::De);
    }
    if normalized.starts_with("en") {
        return Some(SystemLanguage::En);
    }
    if normalized.starts_with("zh") {
        return Some(SystemLanguage::ZhCn);
    }
    None
}

pub fn macos_apple_languages_from_defaults_output(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            let value = line.trim().trim_end_matches(',').trim_matches('"').trim();
            if value.is_empty() || value == "(" || value == ")" {
                None
            } else {
                Some(value.to_string())
            }
        })
        .collect()
}

fn environment_language_tags() -> Vec<String> {
    ["LC_ALL", "LANG", "LANGUAGE"]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok())
        .filter(|value| !value.trim().is_empty())
        .collect()
}

#[cfg(target_os = "macos")]
fn platform_language_tags() -> Vec<String> {
    // GUI 应用从 Finder/Dock 启动时经常没有 LANG；AppleLanguages 才是 macOS 用户语言顺序。
    std::process::Command::new("defaults")
        .args(["read", "-g", "AppleLanguages"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|output| macos_apple_languages_from_defaults_output(&output))
        .unwrap_or_default()
}

#[cfg(not(target_os = "macos"))]
fn platform_language_tags() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_from_tag_supports_supported_locales() {
        assert_eq!(language_from_tag("zh_CN.UTF-8"), Some(SystemLanguage::ZhCn));
        assert_eq!(language_from_tag("zh-Hans-CN"), Some(SystemLanguage::ZhCn));
        assert_eq!(language_from_tag("zh_TW.UTF-8"), Some(SystemLanguage::ZhTw));
        assert_eq!(language_from_tag("zh-Hant-HK"), Some(SystemLanguage::ZhTw));
        assert_eq!(language_from_tag("ja_JP.UTF-8"), Some(SystemLanguage::Ja));
        assert_eq!(language_from_tag("ko_KR.UTF-8"), Some(SystemLanguage::Ko));
        assert_eq!(language_from_tag("de_DE.UTF-8"), Some(SystemLanguage::De));
        assert_eq!(language_from_tag("en_US.UTF-8"), Some(SystemLanguage::En));
    }

    #[test]
    fn language_from_tag_ignores_unsupported_locales() {
        assert_eq!(language_from_tag("fr_FR.UTF-8"), None);
        assert_eq!(language_from_tag(""), None);
    }

    #[test]
    fn preferred_language_uses_macos_apple_languages_when_env_is_missing() {
        assert_eq!(
            preferred_language_from_sources(&[], &["zh-Hans-CN", "en-US"]),
            SystemLanguage::ZhCn
        );
    }

    #[test]
    fn preferred_language_prefers_macos_apple_languages_before_env() {
        assert_eq!(
            preferred_language_from_sources(&["de_DE.UTF-8"], &["zh-Hans-CN"]),
            SystemLanguage::ZhCn
        );
    }

    #[test]
    fn preferred_language_falls_back_to_english() {
        assert_eq!(
            preferred_language_from_sources(&["fr_FR.UTF-8"], &[]),
            SystemLanguage::En
        );
    }

    #[test]
    fn parses_macos_apple_languages_defaults_output() {
        assert_eq!(
            macos_apple_languages_from_defaults_output(
                r#"(
    "zh-Hans-CN",
    "en-CN"
)"#
            ),
            vec!["zh-Hans-CN".to_string(), "en-CN".to_string()]
        );
    }
}
