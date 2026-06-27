use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use chrono::{DateTime, Utc};
use niuma_core::main_state::{MainStateService, MainStateStatus, MainStateWatcher};
use niuma_core::models::RuntimeStateStatus;
use niuma_core::platform::locale::{
    active_language, active_language_preference, set_active_language_preference,
    LanguagePreference, SystemLanguage,
};
use niuma_core::runtime_event::RuntimeEventBus;
use niuma_core::store::NiumaStore;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

const MAIN_WINDOW_LABEL: &str = "main";
const EVENT_CENTER_WINDOW_LABEL: &str = "event-center";
const LANGUAGE_CHANGED_EVENT: &str = "niuma-language-changed";
const TRAY_ID: &str = "main-tray";
const SHOW_MENU_ID: &str = "tray-show-window";
const EVENT_CENTER_MENU_ID: &str = "tray-show-event-center";
const STATUS_MENU_ID: &str = "tray-current-status";
const LANGUAGE_MENU_ID: &str = "tray-language";
const LANGUAGE_SYSTEM_MENU_ID: &str = "tray-language-system";
const LANGUAGE_ZH_CN_MENU_ID: &str = "tray-language-zh-cn";
const LANGUAGE_ZH_TW_MENU_ID: &str = "tray-language-zh-tw";
const LANGUAGE_EN_MENU_ID: &str = "tray-language-en";
const LANGUAGE_JA_MENU_ID: &str = "tray-language-ja";
const LANGUAGE_KO_MENU_ID: &str = "tray-language-ko";
const LANGUAGE_DE_MENU_ID: &str = "tray-language-de";
const QUIT_MENU_ID: &str = "tray-quit";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackgroundPolicy {
    HideToStatusItem,
    ShowMainWindow,
    QuitApplication,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrayLocale {
    ZhCn,
    ZhTw,
    En,
    Ja,
    Ko,
    De,
}

struct EventCenterTrayMenuItem<R: Runtime> {
    item: CheckMenuItem<R>,
}

impl TrayLocale {
    fn from_active() -> Self {
        Self::from(active_language())
    }
}

impl From<SystemLanguage> for TrayLocale {
    fn from(language: SystemLanguage) -> Self {
        match language {
            SystemLanguage::ZhCn => Self::ZhCn,
            SystemLanguage::ZhTw => Self::ZhTw,
            SystemLanguage::En => Self::En,
            SystemLanguage::Ja => Self::Ja,
            SystemLanguage::Ko => Self::Ko,
            SystemLanguage::De => Self::De,
        }
    }
}

pub fn register_tray<R: Runtime>(
    app: &AppHandle<R>,
    is_quitting: Arc<AtomicBool>,
    store: NiumaStore,
    runtime_events: RuntimeEventBus,
) -> tauri::Result<TrayIcon<R>> {
    let locale = TrayLocale::from_active();
    let status = current_status_from_store(&store, Utc::now());
    let labels = tray_labels(&status, locale);
    let show_item = MenuItem::with_id(app, SHOW_MENU_ID, labels.show_window, true, None::<&str>)?;
    let event_center_item = CheckMenuItem::with_id(
        app,
        EVENT_CENTER_MENU_ID,
        labels.event_center,
        true,
        false,
        None::<&str>,
    )?;
    let status_item =
        MenuItem::with_id(app, STATUS_MENU_ID, labels.menu_status, false, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let language_items = TrayLanguageItems::new(app, active_language_preference())?;
    let language_menu = Submenu::with_id_and_items(
        app,
        LANGUAGE_MENU_ID,
        labels.language,
        true,
        language_items.as_menu_items().as_slice(),
    )?;
    let language_separator = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, QUIT_MENU_ID, labels.quit, true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &show_item,
            &event_center_item,
            &status_item,
            &separator,
            &language_menu,
            &language_separator,
            &quit_item,
        ],
    )?;
    let icon = tray_icon_for_status(status)?;
    let menu_items = TrayMenuItems {
        show_item: show_item.clone(),
        event_center_item: event_center_item.clone(),
        status_item: status_item.clone(),
        language_menu: language_menu.clone(),
        quit_item: quit_item.clone(),
        language_items: language_items.clone(),
    };
    let _ = app.manage(EventCenterTrayMenuItem {
        item: event_center_item.clone(),
    });

    let menu_event_store = store.clone();
    let tray = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .icon(icon)
        .title(labels.title)
        .tooltip(labels.tooltip)
        .show_menu_on_left_click(true)
        .on_menu_event({
            let menu_items = menu_items.clone();
            move |app, event| match event.id().as_ref() {
                SHOW_MENU_ID => show_main_window(app),
                EVENT_CENTER_MENU_ID => {
                    let visible = toggle_event_center_window(app);
                    let _ = menu_items.event_center_item.set_checked(visible);
                }
                QUIT_MENU_ID => {
                    is_quitting.store(true, Ordering::SeqCst);
                    app.exit(0);
                }
                id => {
                    if let Some(preference) = language_preference_from_menu_id(id) {
                        if let Err(error) = NiumaStore::new(NiumaStore::default_path())
                            .save_language_preference(preference)
                        {
                            eprintln!("NiumaNotifier language preference not saved: {error}");
                        }
                        set_active_language_preference(preference);
                        notify_language_changed(app);
                        if let Some(tray) = app.tray_by_id(TRAY_ID) {
                            let mut last_labels = None;
                            refresh_tray_labels(
                                &tray,
                                &menu_items,
                                &menu_event_store,
                                &mut last_labels,
                            );
                        }
                    }
                }
            }
        })
        .build(app)?;

    start_tray_status_refresh(tray.clone(), menu_items, store, runtime_events);
    Ok(tray)
}

fn notify_language_changed<R: Runtime>(app: &AppHandle<R>) {
    let language = active_language().storage_id();
    // 状态栏菜单运行在 Rust 侧，需主动通知 WebView 重新套用界面语言。
    if let Err(error) = app.emit(LANGUAGE_CHANGED_EVENT, language) {
        eprintln!("NiumaNotifier emit language change failed: {error}");
    }
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let script = format!(
            "window.dispatchEvent(new CustomEvent('niuma-language-changed', {{ detail: {:?} }}));",
            language
        );
        if let Err(error) = window.eval(&script) {
            eprintln!("NiumaNotifier dispatch language change failed: {error}");
        }
    }
}

pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    set_dock_visible(app, true);
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

pub fn toggle_event_center_window<R: Runtime>(app: &AppHandle<R>) -> bool {
    if let Some(window) = app.get_webview_window(EVENT_CENTER_WINDOW_LABEL) {
        if window.is_visible().unwrap_or(false) {
            let _ = window.close();
            return false;
        }
        let _ = window.show();
        let _ = window.set_focus();
        return true;
    }

    match WebviewWindowBuilder::new(
        app,
        EVENT_CENTER_WINDOW_LABEL,
        WebviewUrl::App("event-center.html".into()),
    )
    .title("NiumaNotifier Event Center")
    .inner_size(820.0, 560.0)
    .min_inner_size(640.0, 420.0)
    .build()
    {
        Ok(window) => {
            let _ = window.set_focus();
            true
        }
        Err(error) => {
            eprintln!("NiumaNotifier event center window failed: {error}");
            false
        }
    }
}

pub fn sync_event_center_menu_visibility<R: Runtime>(app: &AppHandle<R>, visible: bool) {
    if let Some(item) = app.try_state::<EventCenterTrayMenuItem<R>>() {
        let _ = item.item.set_checked(visible);
    }
}

pub fn hide_main_window<R: Runtime>(window: &tauri::Window<R>) {
    if window.label() == MAIN_WINDOW_LABEL {
        let _ = window.hide();
        set_dock_visible(&window.app_handle(), false);
    }
}

pub fn hide_main_window_from_app<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.hide();
    }
    set_dock_visible(app, false);
}

fn set_dock_visible<R: Runtime>(app: &AppHandle<R>, visible: bool) {
    // macOS 允许菜单栏常驻应用按需隐藏 Dock 图标；其他平台没有对应能力。
    #[cfg(target_os = "macos")]
    {
        let _ = app.set_dock_visibility(visible);
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, visible);
    }
}

fn start_tray_status_refresh<R: Runtime>(
    tray: TrayIcon<R>,
    menu_items: TrayMenuItems<R>,
    store: NiumaStore,
    runtime_events: RuntimeEventBus,
) {
    thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                eprintln!("NiumaNotifier tray refresh runtime failed: {error}");
                return;
            }
        };
        runtime.block_on(async move {
            let mut watcher = MainStateWatcher::new(&runtime_events);
            let mut last_labels = None;
            refresh_tray_labels(&tray, &menu_items, &store, &mut last_labels);
            while watcher.wait_for_refresh().await {
                refresh_tray_labels(&tray, &menu_items, &store, &mut last_labels);
            }
        });
    });
}

fn refresh_tray_labels<R: Runtime>(
    tray: &TrayIcon<R>,
    menu_items: &TrayMenuItems<R>,
    store: &NiumaStore,
    last_labels: &mut Option<TrayLabels>,
) {
    let locale = TrayLocale::from_active();
    let labels = tray_labels(&current_status_from_store(store, Utc::now()), locale);
    update_language_menu_checks(&menu_items.language_items, active_language_preference());
    if last_labels.as_ref() == Some(&labels) {
        return;
    }
    let _ = tray.set_title(Some(labels.title));
    let _ = tray.set_tooltip(Some(labels.tooltip.clone()));
    let _ = menu_items.show_item.set_text(labels.show_window);
    let _ = menu_items.event_center_item.set_text(labels.event_center);
    let _ = menu_items.status_item.set_text(labels.menu_status.clone());
    let _ = menu_items.language_menu.set_text(labels.language);
    let _ = menu_items.quit_item.set_text(labels.quit);
    *last_labels = Some(labels);
}

fn current_status_from_store(store: &NiumaStore, now: DateTime<Utc>) -> RuntimeStateStatus {
    MainStateService::new(store.clone())
        .current_state(now)
        .map(|state| runtime_state_status_from_main_state(&state.status))
        .unwrap_or(RuntimeStateStatus::Idle)
}

fn runtime_state_status_from_main_state(status: &MainStateStatus) -> RuntimeStateStatus {
    match status {
        MainStateStatus::WaitingApproval => RuntimeStateStatus::WaitingApproval,
        MainStateStatus::WaitingInput => RuntimeStateStatus::WaitingInput,
        MainStateStatus::Running => RuntimeStateStatus::Running,
        MainStateStatus::Completed => RuntimeStateStatus::Completed,
        MainStateStatus::Error => RuntimeStateStatus::Error,
        MainStateStatus::Idle => RuntimeStateStatus::Idle,
    }
}

fn tray_icon_for_status(_status: RuntimeStateStatus) -> tauri::Result<Image<'static>> {
    Image::from_bytes(include_bytes!("../icons/icon.png"))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TrayLabels {
    title: &'static str,
    menu_status: String,
    tooltip: String,
    show_window: &'static str,
    event_center: &'static str,
    language: &'static str,
    quit: &'static str,
}

fn tray_labels(status: &RuntimeStateStatus, locale: TrayLocale) -> TrayLabels {
    let title = tray_status_label(status, locale);
    TrayLabels {
        title,
        menu_status: format!("{}{}", current_status_prefix(locale), title),
        tooltip: format!("NiumaNotifier - {}{}", current_status_prefix(locale), title),
        show_window: show_window_label(locale),
        event_center: event_center_label(locale),
        language: language_menu_label(locale),
        quit: quit_label(locale),
    }
}

pub fn tray_status_label(status: &RuntimeStateStatus, locale: TrayLocale) -> &'static str {
    match locale {
        TrayLocale::ZhCn => match status {
            RuntimeStateStatus::Idle => "空闲",
            RuntimeStateStatus::Running => "运行中",
            RuntimeStateStatus::WaitingApproval => "待审批",
            RuntimeStateStatus::WaitingInput => "待输入",
            RuntimeStateStatus::Completed => "完成",
            RuntimeStateStatus::Error => "出错",
            RuntimeStateStatus::Stale => "空闲",
        },
        TrayLocale::ZhTw => match status {
            RuntimeStateStatus::Idle => "閒置",
            RuntimeStateStatus::Running => "執行中",
            RuntimeStateStatus::WaitingApproval => "待審批",
            RuntimeStateStatus::WaitingInput => "待輸入",
            RuntimeStateStatus::Completed => "完成",
            RuntimeStateStatus::Error => "出錯",
            RuntimeStateStatus::Stale => "閒置",
        },
        TrayLocale::En => match status {
            RuntimeStateStatus::Idle => "Idle",
            RuntimeStateStatus::Running => "Running",
            RuntimeStateStatus::WaitingApproval => "Approval",
            RuntimeStateStatus::WaitingInput => "Input",
            RuntimeStateStatus::Completed => "Done",
            RuntimeStateStatus::Error => "Error",
            RuntimeStateStatus::Stale => "Idle",
        },
        TrayLocale::Ja => match status {
            RuntimeStateStatus::Idle => "待機",
            RuntimeStateStatus::Running => "実行中",
            RuntimeStateStatus::WaitingApproval => "承認待ち",
            RuntimeStateStatus::WaitingInput => "入力待ち",
            RuntimeStateStatus::Completed => "完了",
            RuntimeStateStatus::Error => "エラー",
            RuntimeStateStatus::Stale => "待機",
        },
        TrayLocale::Ko => match status {
            RuntimeStateStatus::Idle => "대기",
            RuntimeStateStatus::Running => "실행 중",
            RuntimeStateStatus::WaitingApproval => "승인 대기",
            RuntimeStateStatus::WaitingInput => "입력 대기",
            RuntimeStateStatus::Completed => "완료",
            RuntimeStateStatus::Error => "오류",
            RuntimeStateStatus::Stale => "대기",
        },
        TrayLocale::De => match status {
            RuntimeStateStatus::Idle => "Leerlauf",
            RuntimeStateStatus::Running => "Läuft",
            RuntimeStateStatus::WaitingApproval => "Freigabe",
            RuntimeStateStatus::WaitingInput => "Eingabe",
            RuntimeStateStatus::Completed => "Fertig",
            RuntimeStateStatus::Error => "Fehler",
            RuntimeStateStatus::Stale => "Leerlauf",
        },
    }
}

fn current_status_prefix(locale: TrayLocale) -> &'static str {
    match locale {
        TrayLocale::ZhCn => "当前状态：",
        TrayLocale::ZhTw => "目前狀態：",
        TrayLocale::En => "Current status: ",
        TrayLocale::Ja => "現在の状態：",
        TrayLocale::Ko => "현재 상태: ",
        TrayLocale::De => "Aktueller Status: ",
    }
}

fn show_window_label(locale: TrayLocale) -> &'static str {
    match locale {
        TrayLocale::ZhCn => "显示 NiumaNotifier",
        TrayLocale::ZhTw => "顯示 NiumaNotifier",
        TrayLocale::En => "Show NiumaNotifier",
        TrayLocale::Ja => "NiumaNotifier を表示",
        TrayLocale::Ko => "NiumaNotifier 표시",
        TrayLocale::De => "NiumaNotifier anzeigen",
    }
}

pub fn event_center_label(locale: TrayLocale) -> &'static str {
    match locale {
        TrayLocale::ZhCn => "显示事件中心",
        TrayLocale::ZhTw => "顯示事件中心",
        TrayLocale::En => "Show Event Center",
        TrayLocale::Ja => "イベントセンターを表示",
        TrayLocale::Ko => "이벤트 센터 표시",
        TrayLocale::De => "Ereigniszentrum anzeigen",
    }
}

fn quit_label(locale: TrayLocale) -> &'static str {
    match locale {
        TrayLocale::ZhCn => "退出 NiumaNotifier",
        TrayLocale::ZhTw => "結束 NiumaNotifier",
        TrayLocale::En => "Quit NiumaNotifier",
        TrayLocale::Ja => "NiumaNotifier を終了",
        TrayLocale::Ko => "NiumaNotifier 종료",
        TrayLocale::De => "NiumaNotifier beenden",
    }
}

struct TrayMenuItems<R: Runtime> {
    show_item: MenuItem<R>,
    event_center_item: CheckMenuItem<R>,
    status_item: MenuItem<R>,
    language_menu: Submenu<R>,
    quit_item: MenuItem<R>,
    language_items: TrayLanguageItems<R>,
}

struct TrayLanguageItems<R: Runtime> {
    system: CheckMenuItem<R>,
    zh_cn: CheckMenuItem<R>,
    zh_tw: CheckMenuItem<R>,
    en: CheckMenuItem<R>,
    ja: CheckMenuItem<R>,
    ko: CheckMenuItem<R>,
    de: CheckMenuItem<R>,
}

impl<R: Runtime> Clone for TrayMenuItems<R> {
    fn clone(&self) -> Self {
        Self {
            show_item: self.show_item.clone(),
            event_center_item: self.event_center_item.clone(),
            status_item: self.status_item.clone(),
            language_menu: self.language_menu.clone(),
            quit_item: self.quit_item.clone(),
            language_items: self.language_items.clone(),
        }
    }
}

impl<R: Runtime> Clone for TrayLanguageItems<R> {
    fn clone(&self) -> Self {
        Self {
            system: self.system.clone(),
            zh_cn: self.zh_cn.clone(),
            zh_tw: self.zh_tw.clone(),
            en: self.en.clone(),
            ja: self.ja.clone(),
            ko: self.ko.clone(),
            de: self.de.clone(),
        }
    }
}

impl<R: Runtime> TrayLanguageItems<R> {
    fn new(app: &AppHandle<R>, preference: LanguagePreference) -> tauri::Result<Self> {
        Ok(Self {
            system: CheckMenuItem::with_id(
                app,
                LANGUAGE_SYSTEM_MENU_ID,
                system_language_option_label(TrayLocale::from_active()),
                true,
                preference == LanguagePreference::System,
                None::<&str>,
            )?,
            zh_cn: language_item(
                app,
                LANGUAGE_ZH_CN_MENU_ID,
                "简体中文",
                preference,
                SystemLanguage::ZhCn,
            )?,
            zh_tw: language_item(
                app,
                LANGUAGE_ZH_TW_MENU_ID,
                "繁體中文",
                preference,
                SystemLanguage::ZhTw,
            )?,
            en: language_item(
                app,
                LANGUAGE_EN_MENU_ID,
                "English",
                preference,
                SystemLanguage::En,
            )?,
            ja: language_item(
                app,
                LANGUAGE_JA_MENU_ID,
                "日本語",
                preference,
                SystemLanguage::Ja,
            )?,
            ko: language_item(
                app,
                LANGUAGE_KO_MENU_ID,
                "한국어",
                preference,
                SystemLanguage::Ko,
            )?,
            de: language_item(
                app,
                LANGUAGE_DE_MENU_ID,
                "Deutsch",
                preference,
                SystemLanguage::De,
            )?,
        })
    }

    fn as_menu_items(&self) -> Vec<&dyn tauri::menu::IsMenuItem<R>> {
        vec![
            &self.system,
            &self.zh_cn,
            &self.zh_tw,
            &self.en,
            &self.ja,
            &self.ko,
            &self.de,
        ]
    }
}

fn language_item<R: Runtime>(
    app: &AppHandle<R>,
    id: &str,
    label: &str,
    preference: LanguagePreference,
    language: SystemLanguage,
) -> tauri::Result<CheckMenuItem<R>> {
    CheckMenuItem::with_id(
        app,
        id,
        label,
        true,
        preference == LanguagePreference::Fixed(language),
        None::<&str>,
    )
}

fn update_language_menu_checks<R: Runtime>(
    language_items: &TrayLanguageItems<R>,
    preference: LanguagePreference,
) {
    let _ = language_items
        .system
        .set_text(system_language_option_label(TrayLocale::from_active()));
    let _ = language_items
        .system
        .set_checked(preference == LanguagePreference::System);
    let _ = language_items
        .zh_cn
        .set_checked(preference == LanguagePreference::Fixed(SystemLanguage::ZhCn));
    let _ = language_items
        .zh_tw
        .set_checked(preference == LanguagePreference::Fixed(SystemLanguage::ZhTw));
    let _ = language_items
        .en
        .set_checked(preference == LanguagePreference::Fixed(SystemLanguage::En));
    let _ = language_items
        .ja
        .set_checked(preference == LanguagePreference::Fixed(SystemLanguage::Ja));
    let _ = language_items
        .ko
        .set_checked(preference == LanguagePreference::Fixed(SystemLanguage::Ko));
    let _ = language_items
        .de
        .set_checked(preference == LanguagePreference::Fixed(SystemLanguage::De));
}

fn language_preference_from_menu_id(id: &str) -> Option<LanguagePreference> {
    match id {
        LANGUAGE_SYSTEM_MENU_ID => Some(LanguagePreference::System),
        LANGUAGE_ZH_CN_MENU_ID => Some(LanguagePreference::Fixed(SystemLanguage::ZhCn)),
        LANGUAGE_ZH_TW_MENU_ID => Some(LanguagePreference::Fixed(SystemLanguage::ZhTw)),
        LANGUAGE_EN_MENU_ID => Some(LanguagePreference::Fixed(SystemLanguage::En)),
        LANGUAGE_JA_MENU_ID => Some(LanguagePreference::Fixed(SystemLanguage::Ja)),
        LANGUAGE_KO_MENU_ID => Some(LanguagePreference::Fixed(SystemLanguage::Ko)),
        LANGUAGE_DE_MENU_ID => Some(LanguagePreference::Fixed(SystemLanguage::De)),
        _ => None,
    }
}

fn language_menu_label(locale: TrayLocale) -> &'static str {
    match locale {
        TrayLocale::ZhCn => "语言",
        TrayLocale::ZhTw => "語言",
        TrayLocale::En => "Language",
        TrayLocale::Ja => "言語",
        TrayLocale::Ko => "언어",
        TrayLocale::De => "Sprache",
    }
}

fn system_language_option_label(locale: TrayLocale) -> &'static str {
    match locale {
        TrayLocale::ZhCn => "跟随系统",
        TrayLocale::ZhTw => "跟隨系統",
        TrayLocale::En => "Follow System",
        TrayLocale::Ja => "システムに従う",
        TrayLocale::Ko => "시스템 설정 따르기",
        TrayLocale::De => "Systemsprache verwenden",
    }
}

pub fn policy_for_main_visibility(is_visible: bool) -> BackgroundPolicy {
    if is_visible {
        BackgroundPolicy::ShowMainWindow
    } else {
        BackgroundPolicy::HideToStatusItem
    }
}

pub fn policy_for_exit_request(is_quitting: bool) -> BackgroundPolicy {
    if is_quitting {
        BackgroundPolicy::QuitApplication
    } else {
        BackgroundPolicy::HideToStatusItem
    }
}

pub fn should_hide_window_on_close(label: &str, is_quitting: bool) -> bool {
    label == MAIN_WINDOW_LABEL && !is_quitting
}

pub fn enable_macos_default_menu() -> bool {
    true
}

pub fn install_macos_terminate_guard() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use niuma_core::listener_config::ListenerConfig;
    use niuma_core::models::{
        CompletionReason, EventType, NiumaEvent, RuntimeStateStatus, ToolKind,
    };
    use niuma_core::platform::locale::SystemLanguage;
    use niuma_core::store::NiumaStore;

    use crate::tray::{
        current_status_from_store, event_center_label, language_preference_from_menu_id,
        policy_for_exit_request, policy_for_main_visibility, should_hide_window_on_close,
        tray_labels, tray_status_label, BackgroundPolicy, TrayLocale, LANGUAGE_EN_MENU_ID,
        LANGUAGE_SYSTEM_MENU_ID,
    };

    #[test]
    fn zh_cn_tray_status_uses_short_labels() {
        assert_eq!(
            tray_status_label(&RuntimeStateStatus::WaitingApproval, TrayLocale::ZhCn),
            "待审批"
        );
        let labels = tray_labels(&RuntimeStateStatus::Running, TrayLocale::ZhCn);
        assert_eq!(labels.title, "运行中");
        assert_eq!(labels.menu_status, "当前状态：运行中");
        assert_eq!(labels.tooltip, "NiumaNotifier - 当前状态：运行中");
        assert_eq!(labels.show_window, "显示 NiumaNotifier");
        assert_eq!(labels.event_center, "显示事件中心");
        assert_eq!(labels.language, "语言");
        assert_eq!(labels.quit, "退出 NiumaNotifier");
    }

    #[test]
    fn english_tray_status_uses_compact_status_and_full_menu_text() {
        assert_eq!(
            tray_status_label(&RuntimeStateStatus::WaitingInput, TrayLocale::En),
            "Input"
        );
        let labels = tray_labels(&RuntimeStateStatus::Error, TrayLocale::En);
        assert_eq!(labels.title, "Error");
        assert_eq!(labels.menu_status, "Current status: Error");
        assert_eq!(labels.tooltip, "NiumaNotifier - Current status: Error");
        assert_eq!(labels.show_window, "Show NiumaNotifier");
        assert_eq!(labels.event_center, "Show Event Center");
        assert_eq!(labels.language, "Language");
        assert_eq!(labels.quit, "Quit NiumaNotifier");
    }

    #[test]
    fn event_center_tray_label_is_localized() {
        assert_eq!(event_center_label(TrayLocale::ZhTw), "顯示事件中心");
        assert_eq!(event_center_label(TrayLocale::Ja), "イベントセンターを表示");
        assert_eq!(event_center_label(TrayLocale::Ko), "이벤트 센터 표시");
        assert_eq!(
            event_center_label(TrayLocale::De),
            "Ereigniszentrum anzeigen"
        );
    }

    #[test]
    fn stale_tray_status_is_presented_as_idle() {
        let labels = tray_labels(&RuntimeStateStatus::Stale, TrayLocale::En);

        assert_eq!(labels.title, "Idle");
        assert_eq!(labels.menu_status, "Current status: Idle");
    }

    #[test]
    fn current_status_from_store_uses_main_state_completed_expiry() {
        let store = NiumaStore::new(test_sqlite_path("tray_completed_expiry"));
        enable_codex_listener(&store);
        store.append_event(completed_event(1_000)).unwrap();

        assert_eq!(
            current_status_from_store(&store, at(1_000 + 59)),
            RuntimeStateStatus::Completed
        );
        assert_eq!(
            current_status_from_store(&store, at(1_000 + 61)),
            RuntimeStateStatus::Idle
        );
    }

    #[test]
    fn system_language_detection_supports_chinese_variants() {
        assert_eq!(TrayLocale::from(SystemLanguage::ZhCn), TrayLocale::ZhCn);
        assert_eq!(TrayLocale::from(SystemLanguage::ZhTw), TrayLocale::ZhTw);
        assert_eq!(TrayLocale::from(SystemLanguage::De), TrayLocale::De);
    }

    #[test]
    fn language_menu_ids_map_to_preferences() {
        assert_eq!(
            language_preference_from_menu_id(LANGUAGE_SYSTEM_MENU_ID),
            Some(niuma_core::platform::locale::LanguagePreference::System)
        );
        assert_eq!(
            language_preference_from_menu_id(LANGUAGE_EN_MENU_ID),
            Some(niuma_core::platform::locale::LanguagePreference::Fixed(
                SystemLanguage::En
            ))
        );
        assert_eq!(language_preference_from_menu_id("unknown"), None);
    }

    #[test]
    fn background_policy_tracks_main_window_visibility() {
        assert_eq!(
            policy_for_main_visibility(true),
            BackgroundPolicy::ShowMainWindow
        );
        assert_eq!(
            policy_for_main_visibility(false),
            BackgroundPolicy::HideToStatusItem
        );
    }

    #[test]
    fn normal_exit_request_hides_to_status_item_and_explicit_quit_exits() {
        assert_eq!(
            policy_for_exit_request(false),
            BackgroundPolicy::HideToStatusItem
        );
        assert_eq!(
            policy_for_exit_request(true),
            BackgroundPolicy::QuitApplication
        );
    }

    #[test]
    fn only_main_window_hides_on_close() {
        assert!(should_hide_window_on_close("main", false));
        assert!(!should_hide_window_on_close("event-center", false));
        assert!(!should_hide_window_on_close("main", true));
    }

    #[test]
    fn macos_default_menu_is_enabled_so_edit_shortcuts_work() {
        assert!(crate::tray::enable_macos_default_menu());
    }

    #[test]
    fn macos_terminate_guard_is_required_for_dock_quit() {
        assert!(crate::tray::install_macos_terminate_guard());
    }

    fn completed_event(timestamp: i64) -> NiumaEvent {
        NiumaEvent {
            id: "tray-completed".to_string(),
            dedupe_key: "tray-completed".to_string(),
            source: "test".to_string(),
            tool: ToolKind::Codex,
            session_id: "tray-session".to_string(),
            parent_session_id: None,
            normalized_session_id: None,
            session_scope: None,
            agent_nickname: None,
            agent_role: None,
            project_path: "/tmp/tray".to_string(),
            project_name: "tray".to_string(),
            event_type: EventType::AssistantMessageCompleted,
            severity: "info".to_string(),
            summary: "任务完成".to_string(),
            content: Some("任务完成正文".to_string()),
            error_message: None,
            attention_resolve_key: None,
            completion_reason: Some(CompletionReason::Normal),
            failure_reason: None,
            payload_ref: None,
            interaction: None,
            created_at: at(timestamp),
        }
    }

    fn at(timestamp: i64) -> chrono::DateTime<Utc> {
        Utc.timestamp_opt(timestamp, 0).single().unwrap()
    }

    fn enable_codex_listener(store: &NiumaStore) {
        store
            .save_listener_config(&ListenerConfig {
                codex_listening_enabled: true,
                ..ListenerConfig::default()
            })
            .unwrap();
    }

    fn test_sqlite_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "niuma-tray-{name}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("niuma.sqlite")
    }
}
