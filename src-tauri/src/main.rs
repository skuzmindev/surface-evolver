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
