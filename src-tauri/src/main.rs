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
            // WebDriver runs drive the app while it sits behind other windows.
            // macOS then marks the page hidden and stops requestAnimationFrame
            // (backgroundThrottling doesn't cover occlusion), freezing the
            // viewer. Private WebKit setting, so dev-only, behind the feature.
            #[cfg(all(feature = "webdriver", target_os = "macos"))]
            if let Some(w) = app.get_webview_window("main") {
                w.with_webview(|wv| unsafe {
                    let view = &*(wv.inner() as *const objc2::runtime::AnyObject);
                    let _: () = objc2::msg_send![view, _setWindowOcclusionDetectionEnabled: false];
                })?;
                // SE_WEBDRIVER_GHOST=1: no window for the user at all, measured
                // to keep the viewer drawing at 60 rAF/s. Not a hidden window
                // (it stops rendering) and not an off-screen one (macOS pulls
                // it back into view): an accessory app (no Dock icon, never
                // takes focus) with an ordered-in window that is fully
                // transparent and click-through. Becoming an accessory orders
                // the window out, which also stops rendering, so it is ordered
                // back in without activating the app.
                if std::env::var_os("SE_WEBDRIVER_GHOST").is_some_and(|v| v == "1") {
                    app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                    let ns_window = w.ns_window()? as *const objc2::runtime::AnyObject;
                    unsafe {
                        let _: () = objc2::msg_send![&*ns_window, setAlphaValue: 0.0f64];
                        let _: () = objc2::msg_send![&*ns_window, setIgnoresMouseEvents: true];
                        let _: () = objc2::msg_send![&*ns_window, orderFrontRegardless];
                    }
                }
            }
            let m = menu::build(app)?;
            app.set_menu(m)?;
            app.on_menu_event(|handle, event| menu::on_event(handle, event.id().as_ref()));
            Ok(())
        })
        .on_window_event(|window, event| {
            // App quit = worker orphaned otherwise (it blocks on stdin read).
            if let tauri::WindowEvent::Destroyed = event {
                let state: tauri::State<rpc::AppState> = window.app_handle().state();
                state.manager.kill();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
