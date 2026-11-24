use surface_evolver::{menu, rpc};

use tauri::Manager;

fn main() {
    let builder = tauri::Builder::default();
    #[cfg(feature = "webdriver")]
    let builder = builder.plugin(tauri_plugin_webdriver::init());
    builder
        .plugin(tauri_plugin_opener::init())
        .manage(rpc::AppState::default())
        .invoke_handler(tauri::generate_handler![rpc::rpc])
        .setup(|app| {
            #[cfg(all(feature = "webdriver", target_os = "macos"))]
            prepare_window_for_webdriver(app)?;
            let m = menu::build(app)?;
            app.set_menu(m)?;
            app.on_menu_event(|handle, event| menu::on_event(handle, event.id().as_ref()));
            Ok(())
        })
        .on_window_event(|window, event| {
            // On quit, or the worker outlives the app, blocked reading its stdin.
            if let tauri::WindowEvent::Destroyed = event {
                let state: tauri::State<rpc::AppState> = window.app_handle().state();
                state.manager.kill();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Keeps the viewer rendering while WebDriver drives the app from behind other
/// windows, which macOS would otherwise mark hidden, stopping
/// requestAnimationFrame (backgroundThrottling doesn't cover that). It turns off
/// WKWebView's occlusion detection, a private WebKit setting, hence dev-only.
///
/// With `SE_WEBDRIVER_GHOST=1` the user sees no window at all: the app becomes an
/// accessory (no Dock icon, never takes focus) and the window turns transparent
/// and click-through. It must stay ordered in, because a hidden window stops
/// rendering and macOS pulls an off-screen one back into view. Becoming an
/// accessory orders it out, so it is ordered back in without activating the app.
#[cfg(all(feature = "webdriver", target_os = "macos"))]
fn prepare_window_for_webdriver(app: &mut tauri::App) -> tauri::Result<()> {
    let Some(w) = app.get_webview_window("main") else { return Ok(()) };
    w.with_webview(|wv| unsafe {
        let view = &*(wv.inner() as *const objc2::runtime::AnyObject);
        let _: () = objc2::msg_send![view, _setWindowOcclusionDetectionEnabled: false];
    })?;
    if std::env::var_os("SE_WEBDRIVER_GHOST").is_some_and(|v| v == "1") {
        app.set_activation_policy(tauri::ActivationPolicy::Accessory);
        let ns_window = w.ns_window()? as *const objc2::runtime::AnyObject;
        unsafe {
            let _: () = objc2::msg_send![&*ns_window, setAlphaValue: 0.0f64];
            let _: () = objc2::msg_send![&*ns_window, setIgnoresMouseEvents: true];
            let _: () = objc2::msg_send![&*ns_window, orderFrontRegardless];
        }
    }
    Ok(())
}
