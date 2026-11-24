//! The app as a library, so the integration tests in `tests/` can link `rpc`
//! and `worker`. `rpc` is generic over `R: Runtime` so those tests can drive it
//! with Tauri's `MockRuntime` instead of a real webview.

pub mod menu;
pub mod rpc;
pub mod worker;
