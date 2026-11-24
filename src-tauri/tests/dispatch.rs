//! End-to-end tests of the `rpc` command. MockRuntime replaces the webview;
//! everything below it is real: the se-worker, libse, fe/ files, the filesystem.
//!
//! `dispatch()` reads the SE_* paths from the environment on every call, so
//! tests that set them can't run concurrently: each takes `LOCK` first.

mod common;

use common::{lib_path, scratch_dir, skip, worker_bin};
use serde_json::{json, Value};
use std::sync::{Mutex, MutexGuard};
use surface_evolver::rpc::{self, AppState};
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
use tauri::{App, Manager};

static LOCK: Mutex<()> = Mutex::new(());

/// A mock app set up like main.rs, with SE_FE_DIR at the bundled fe/ and
/// SE_STATE_DIR at a fresh scratch dir. Holds `LOCK` while alive.
struct TestApp {
    app: App<MockRuntime>,
    state_dir: std::path::PathBuf,
    _lock: MutexGuard<'static, ()>,
}

impl TestApp {
    fn new(tag: &str) -> Self {
        let _lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let state_dir = scratch_dir(tag);
        std::env::set_var("SE_LIB_PATH", lib_path());
        std::env::set_var("SE_WORKER_PATH", worker_bin());
        std::env::set_var("SE_FE_DIR", common::repo_root().join("fe"));
        std::env::set_var("SE_STATE_DIR", &state_dir);

        let app = mock_builder()
            .plugin(tauri_plugin_opener::init())
            .manage(AppState::default())
            .build(mock_context(noop_assets()))
            .expect("build mock app");
        Self { app, state_dir, _lock }
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let handle = self.app.handle().clone();
        tauri::async_runtime::block_on(rpc::rpc(handle, method.to_string(), params))
    }
}

impl Drop for TestApp {
    fn drop(&mut self) {
        common::cleanup(&self.state_dir);
    }
}

#[test]
fn list_files_sees_bundled_fixtures() {
    if skip() { return }
    let t = TestApp::new("listfiles");
    let files = t.call("listFiles", Value::Null).expect("listFiles");
    let files = files.as_array().unwrap();
    let names: Vec<&str> = files.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(names.contains(&"cube.fe"), "got: {names:?}");
    assert!(names.len() >= 20, "expected at least 20 bundled files, got {}", names.len());
}

#[test]
fn create_session_on_missing_file_errors_without_leaking_a_worker() {
    if skip() { return }
    let t = TestApp::new("missing-file");
    let err = t.call("createSession", json!({ "fe_file": "does_not_exist.fe" })).unwrap_err();
    assert!(err.contains("File not found"), "got: {err}");
}

#[test]
fn full_session_lifecycle_round_trips_through_the_app_layer() {
    if skip() { return }
    let t = TestApp::new("lifecycle");

    let session = t.call("createSession", json!({ "fe_file": "cube.fe" })).expect("createSession");
    let sid = session["session_id"].as_str().unwrap().to_string();
    assert_eq!(session["vertex_count"], 14);

    let run = t.call("runCommand", json!({ "sessionId": sid, "command": "r; g 5" })).expect("runCommand");
    assert!(run["energy"].is_f64());
    // The UI refetches the mesh only on this flag.
    assert_eq!(run["geometry_changed"], true);

    let mesh = t.call("getMesh", json!({ "sessionId": sid, "colors": true })).expect("getMesh");
    assert!(!mesh["vertices"].as_array().unwrap().is_empty());
    assert!(mesh["facet_colors"].is_array());

    let vinfo = t.call("vertexInfo", json!({ "sessionId": sid, "vpos": 0 })).expect("vertexInfo");
    assert_eq!(vinfo["ok"], true);

    let topo = t.call("topo", json!({ "sessionId": sid, "op": "refine" })).expect("topo");
    assert!(topo["counts"].is_object());

    let dmp = t.call("exportDmp", json!({ "sessionId": sid })).expect("exportDmp");
    assert!(dmp["content"].as_str().unwrap().contains("vertices"));

    let fe_export = t.call("exportFe", json!({ "sessionId": sid })).expect("exportFe");
    assert!(fe_export["filename"].as_str().unwrap().ends_with(".fe"));

    // Nothing is running, so Stop is a no-op that keeps the session.
    let cancelled = t.call("cancel", Value::Null).expect("cancel");
    assert_eq!(cancelled["cancelled"], true);
    assert_eq!(cancelled["interrupted"], true);
    t.call("getMesh", json!({ "sessionId": sid })).expect("the session survives an idle Stop");
}

#[test]
fn upload_file_enforces_extension_and_rejects_duplicates() {
    if skip() { return }
    let t = TestApp::new("upload");

    let bad_ext = t.call("uploadFile", json!({ "filename": "notes.txt", "content": "AA==" })).unwrap_err();
    assert!(bad_ext.contains(".fe"), "got: {bad_ext}");

    let content = base64_encode(b"SOAPFILM\nvertices\n1 0 0 0\n");
    let ok = t.call("uploadFile", json!({ "filename": "my_upload.fe", "content": content })).expect("upload");
    assert_eq!(ok["filename"], "my_upload.fe");

    let dup = t.call("uploadFile", json!({ "filename": "my_upload.fe", "content": content })).unwrap_err();
    assert!(dup.contains("already exists"), "got: {dup}");

    let bad_b64 = t.call("uploadFile", json!({ "filename": "other.fe", "content": "not base64!!" })).unwrap_err();
    assert!(bad_b64.contains("base64"), "got: {bad_b64}");
}

#[test]
fn get_restore_resumes_from_a_persisted_snapshot() {
    if skip() { return }
    let t = TestApp::new("restore");

    // No snapshot yet: null, not an error.
    let none = t.call("getRestore", Value::Null).expect("getRestore (empty)");
    assert!(none.is_null());

    let session = t.call("createSession", json!({ "fe_file": "cube.fe" })).expect("createSession");
    let sid = session["session_id"].as_str().unwrap().to_string();
    // The snapshot is written in the background, so poll for it.
    t.call("runCommand", json!({ "sessionId": sid, "command": "g 1" })).expect("runCommand");
    let snapshot_file = t.state_dir.join("last-session.json");
    for _ in 0..50 {
        if snapshot_file.exists() { break }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(snapshot_file.exists(), "snapshot was never written");

    // A fresh AppState, as after a restart, on the same SE_STATE_DIR.
    let app2 = mock_builder()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .build(mock_context(noop_assets()))
        .expect("build second mock app");
    let handle2 = app2.handle().clone();
    // Two overlapping calls, as React StrictMode makes in dev: the one that finds
    // the restore running must wait for it, not answer null.
    let gate = std::sync::Arc::new(std::sync::Barrier::new(2));
    let calls: Vec<_> = (0..2)
        .map(|_| {
            let (h, gate) = (handle2.clone(), gate.clone());
            std::thread::spawn(move || {
                gate.wait();
                tauri::async_runtime::block_on(rpc::rpc(h, "getRestore".into(), Value::Null))
            })
        })
        .collect();
    let mut results = calls.into_iter().map(|c| c.join().unwrap().expect("getRestore (resume)"));
    let (restored, overlapping) = (results.next().unwrap(), results.next().unwrap());
    assert_eq!(overlapping, restored, "the overlapping getRestore didn't get the restored session");
    assert_eq!(restored["fe_file"], "cube.fe");
    assert!(restored["session_id"].is_string());

    // A later call returns the same session, not a second restore.
    let restored2 = tauri::async_runtime::block_on(rpc::rpc(handle2.clone(), "getRestore".into(), Value::Null))
        .expect("getRestore (memoized)");
    assert_eq!(restored2, restored);

    // Once another file is open the restored session is dead and must not come back.
    tauri::async_runtime::block_on(rpc::rpc(handle2.clone(), "createSession".into(), json!({ "fe_file": "sphere.fe" })))
        .expect("createSession after restore");
    let after = tauri::async_runtime::block_on(rpc::rpc(handle2, "getRestore".into(), Value::Null))
        .expect("getRestore (after a new load)");
    assert!(after.is_null(), "getRestore returned a replaced session: {after}");
}

/// A late getRestore must not kill the worker of a file the user already opened.
#[test]
fn get_restore_is_a_noop_once_the_user_has_already_loaded_a_file() {
    if skip() { return }
    let t = TestApp::new("restore-noop");
    t.call("createSession", json!({ "fe_file": "cube.fe" })).expect("createSession");
    let restore = t.call("getRestore", Value::Null).expect("getRestore");
    assert!(restore.is_null());
    let files = t.call("listFiles", Value::Null).expect("listFiles still works");
    assert!(files.as_array().unwrap().len() >= 20);
}

/// createSession kills the old worker before loading, so a failed load must
/// report the old session gone rather than leave it for later commands to fail on.
#[test]
fn a_failed_load_ends_the_old_session_instead_of_leaving_it_dead() {
    if skip() { return }
    let t = TestApp::new("failed-reload");
    let old = t.call("createSession", json!({ "fe_file": "cube.fe" })).expect("createSession");
    let old_sid = old["session_id"].as_str().unwrap().to_string();

    let string_fe = base64_encode(b"STRING\nspace_dimension 2\n\nvertices\n1 0 0\n2 1 0\n\nedges\n1 1 2\n");
    t.call("uploadFile", json!({ "filename": "string_model.fe", "content": string_fe })).expect("upload");
    let err = t.call("createSession", json!({ "fe_file": "string_model.fe" })).unwrap_err();
    assert!(err.starts_with("Session ended:"), "got: {err}");

    let err = t.call("runCommand", json!({ "sessionId": old_sid, "command": "g 1" })).unwrap_err();
    assert_eq!(err, "No file is loaded");
}

/// A worker that dies between requests: the next request reports the session
/// gone, and the one after finds no session rather than a dead worker.
#[test]
fn a_worker_crash_ends_the_session() {
    if skip() { return }
    let t = TestApp::new("crash");
    let s = t.call("createSession", json!({ "fe_file": "cube.fe" })).expect("createSession");
    let sid = s["session_id"].as_str().unwrap().to_string();
    t.app.state::<AppState>().manager.kill();

    let err = t.call("runCommand", json!({ "sessionId": sid, "command": "g 1" })).unwrap_err();
    assert!(err.starts_with("Session ended:"), "got: {err}");
    let err = t.call("getMesh", json!({ "sessionId": sid })).unwrap_err();
    assert_eq!(err, "No file is loaded");
}

/// A datafile that fails to load must still reach the editor, to be fixed there.
#[test]
fn read_file_returns_datafile_text_without_a_session() {
    if skip() { return }
    let t = TestApp::new("read-file");
    let bundled = t.call("readFile", json!({ "filename": "cube.fe" })).expect("readFile bundled");
    assert_eq!(bundled["filename"], "cube.fe");
    assert!(bundled["content"].as_str().unwrap().contains("vertices"));

    // A user copy, as Save & Reload writes, wins over the bundled file.
    t.call("updateFile", json!({ "filename": "cube.fe", "content": "broken text" })).expect("updateFile");
    let user = t.call("readFile", json!({ "filename": "cube.fe" })).expect("readFile user copy");
    assert_eq!(user["content"], "broken text");

    // Directory components are stripped.
    let err = t.call("readFile", json!({ "filename": "../../etc/does_not_exist.fe" })).unwrap_err();
    assert!(err.contains("File not found"), "got: {err}");
}

/// Stop sends SIGINT first, so the session and its evolved surface survive.
#[test]
fn cancel_interrupts_a_running_command_and_keeps_the_session() {
    if skip() { return }
    let t = TestApp::new("cancel-interrupt");
    let s = t.call("createSession", json!({ "fe_file": "cube.fe" })).expect("createSession");
    let sid = s["session_id"].as_str().unwrap().to_string();
    t.call("runCommand", json!({ "sessionId": sid, "command": "r; r; r; r" })).expect("refine");

    let handle = t.app.handle().clone();
    let (sid2, (tx, rx)) = (sid.clone(), std::sync::mpsc::channel());
    std::thread::spawn(move || {
        let r = tauri::async_runtime::block_on(rpc::rpc(handle, "runCommand".into(),
            json!({ "sessionId": sid2, "command": "g 200000" })));
        let _ = tx.send(r);
    });
    std::thread::sleep(std::time::Duration::from_millis(800));

    let cancelled = t.call("cancel", Value::Null).expect("cancel");
    assert_eq!(cancelled["interrupted"], true, "got: {cancelled}");
    let run = rx.recv_timeout(std::time::Duration::from_secs(5)).expect("run still blocked");
    let err = run.expect_err("the interrupted command reports the abort");
    assert!(!err.starts_with("Session ended:"), "an interrupt must not end the session: {err}");

    let mesh = t.call("getMesh", json!({ "sessionId": sid })).expect("same session still works");
    assert_eq!(mesh["ok"], true);
}

/// An empty command succeeds and reports the current energy and area (the UI's
/// refresh after Stop); only a missing `command` is an error.
#[test]
fn an_empty_command_reports_the_current_stats() {
    if skip() { return }
    let t = TestApp::new("empty-command");
    let s = t.call("createSession", json!({ "fe_file": "cube.fe" })).expect("createSession");
    let sid = s["session_id"].as_str().unwrap().to_string();
    let r = t.call("runCommand", json!({ "sessionId": sid, "command": "" })).expect("empty command");
    assert_eq!(r["energy"].as_f64(), Some(6.0));
    let err = t.call("runCommand", json!({ "sessionId": sid })).unwrap_err();
    assert!(err.contains("command is required"), "got: {err}");
}

/// What the datafile's `read` section printed comes back with createSession.
#[test]
fn create_session_reports_what_the_read_section_printed() {
    if skip() { return }
    let t = TestApp::new("load-output");
    let fe = base64_encode(b"vertices\n1 0 0 0\n2 1 0 0\n3 0 1 0\nedges\n1 1 2\n2 2 3\n3 3 1\nfaces\n1 1 2 3\nread\nprint 5150\n");
    t.call("uploadFile", json!({ "filename": "prints.fe", "content": fe })).expect("upload");
    let s = t.call("createSession", json!({ "fe_file": "prints.fe" })).expect("createSession");
    assert!(s["output"].as_str().unwrap_or_default().contains("5150"), "got: {s}");
}

fn base64_encode(bytes: &[u8]) -> String {
    const TBL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        out.push(TBL[(b[0] >> 2) as usize] as char);
        out.push(TBL[(((b[0] & 0x03) << 4) | (b[1] >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 { TBL[(((b[1] & 0x0f) << 2) | (b[2] >> 6)) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { TBL[(b[2] & 0x3f) as usize] as char } else { '=' });
    }
    out
}
