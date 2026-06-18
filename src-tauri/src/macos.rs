use std::sync::Once;

use objc2::ffi::{class_addMethod, object_getClass};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Imp, ProtocolObject, Sel};
use objc2::{sel, MainThreadMarker};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate,
    NSApplicationTerminateReply,
};

static INSTALL_TERMINATE_GUARD: Once = Once::new();

pub fn install_terminate_guard() {
    INSTALL_TERMINATE_GUARD.call_once(|| {
        if let Some(mtm) = MainThreadMarker::new() {
            let app = NSApplication::sharedApplication(mtm);
            if let Some(delegate) = app.delegate() {
                add_should_terminate_guard(&delegate);
            }
        }
    });
}

fn add_should_terminate_guard(delegate: &Retained<ProtocolObject<dyn NSApplicationDelegate>>) {
    // Tao 的 AppDelegate 没实现 applicationShouldTerminate:；Dock Quit 会直接走 AppKit terminate。
    // 给现有 delegate 动态补一个取消终止的方法，保留 Tao/Tauri 已有 delegate 行为。
    unsafe {
        let delegate_ptr = Retained::as_ptr(delegate).cast::<AnyObject>();
        let delegate_class = object_getClass(delegate_ptr).cast_mut();
        let implementation: unsafe extern "C-unwind" fn(
            &AnyObject,
            Sel,
            &NSApplication,
        ) -> NSApplicationTerminateReply = dock_quit_should_hide_to_status_item;
        let _ = class_addMethod(
            delegate_class,
            sel!(applicationShouldTerminate:),
            std::mem::transmute::<_, Imp>(implementation),
            c"Q@:@".as_ptr(),
        );
    }
}

unsafe extern "C-unwind" fn dock_quit_should_hide_to_status_item(
    _delegate: &AnyObject,
    _selector: Sel,
    app: &NSApplication,
) -> NSApplicationTerminateReply {
    app.hide(None);
    let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    NSApplicationTerminateReply::TerminateCancel
}
