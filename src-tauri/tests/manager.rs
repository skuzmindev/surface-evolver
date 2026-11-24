//! Tests of `worker::Manager` against a real se-worker, libse and fe/ files.
//! It takes plain paths, not an AppHandle, so no mocking is needed.

mod common;

use common::{fe, lib_path, scratch_dir, skip, worker_bin};
use serde_json::json;
use surface_evolver::worker::{lock, Manager};

#[test]
fn load_session_returns_real_stats() {
    if skip() { return }
    let mgr = Manager::default();
    let stats = mgr
        .load_session("s1", &fe("cube.fe").to_string_lossy(), &worker_bin(), &lib_path())
        .expect("load cube.fe");
    assert_eq!(stats["vertex_count"], 14);
    assert_eq!(stats["sdim"], 3);
}

#[test]
fn reload_kills_previous_worker_and_starts_fresh() {
    if skip() { return }
    let mgr = Manager::default();
    mgr.load_session("s1", &fe("cube.fe").to_string_lossy(), &worker_bin(), &lib_path())
        .expect("load s1");
    let stats2 = mgr
        .load_session("s2", &fe("sphere.fe").to_string_lossy(), &worker_bin(), &lib_path())
        .expect("load s2");
    assert!(stats2["vertex_count"].as_u64().unwrap() > 0);

    // A request for s1 must fail, not silently reach s2's worker.
    let err = mgr.request("s1", json!({ "cmd": "mesh" })).unwrap_err();
    assert!(err.contains("not currently loaded"), "got: {err}");

    let mesh = mgr.request("s2", json!({ "cmd": "mesh" })).expect("mesh s2");
    assert_eq!(mesh["ok"], true);
}

/// Engine state after a failed load is undefined, so the attempt must not be
/// registered as a session, and the next good load must work normally.
#[test]
fn failed_load_leaves_manager_clean_for_the_next_attempt() {
    if skip() { return }
    let dir = scratch_dir("string-fixture");
    let bad = dir.join("string_tri.fe");
    std::fs::write(&bad, concat!(
        "STRING\n",
        "space_dimension 2\n\n",
        "vertices\n1  0.0 0.0\n2  1.0 0.0\n3  0.5 1.0\n\n",
        "edges\n1   1 2\n2   2 3\n3   3 1\n",
    )).unwrap();

    let mgr = Manager::default();
    let err = mgr
        .load_session("bad", &bad.to_string_lossy(), &worker_bin(), &lib_path())
        .unwrap_err();
    assert!(err.to_lowercase().contains("soapfilm"), "got: {err}");

    let after = mgr.request("bad", json!({ "cmd": "mesh" }));
    assert!(after.is_err(), "no session should be registered after a failed load");

    let stats = mgr
        .load_session("good", &fe("cube.fe").to_string_lossy(), &worker_bin(), &lib_path())
        .expect("load after a failed load must still work");
    assert_eq!(stats["vertex_count"], 14);

    common::cleanup(&dir);
}

#[test]
fn kill_with_no_active_worker_is_a_no_op() {
    let mgr = Manager::default();
    mgr.kill(); // must not panic
    mgr.kill(); // idempotent
}

#[test]
fn request_with_no_worker_loaded_errors_cleanly() {
    let mgr = Manager::default();
    let err = mgr.request("nope", json!({ "cmd": "mesh" })).unwrap_err();
    assert!(err.contains("No active SE worker"), "got: {err}");
}

#[test]
fn worker_dying_mid_session_is_reported_and_recovered_from() {
    if skip() { return }
    let mgr = Manager::default();
    mgr.load_session("s1", &fe("cube.fe").to_string_lossy(), &worker_bin(), &lib_path())
        .expect("load s1");

    // Crash it behind the manager's back; `kill()` would clear `io` itself.
    {
        let mut proc = lock(&mgr.proc);
        let child = proc.as_mut().expect("worker running");
        child.kill().expect("kill worker process");
        let _ = child.wait();
    }

    let err = mgr.request("s1", json!({ "cmd": "mesh" })).unwrap_err();
    assert!(!err.is_empty());

    // The failed request must have cleared `io`, so the next load works.
    let stats = mgr
        .load_session("s2", &fe("cube.fe").to_string_lossy(), &worker_bin(), &lib_path())
        .expect("load after a crashed worker must still work");
    assert_eq!(stats["vertex_count"], 14);
}

/// Why `io` and `proc` are separate locks: `kill` must not wait for the running
/// command. It takes only `proc`, so the blocked request gets EOF and errors.
/// Detached threads and a timeout make a regression fail instead of hang.
#[test]
fn kill_unblocks_an_in_flight_request() {
    if skip() { return }
    let mgr = std::sync::Arc::new(Manager::default());
    mgr.load_session("s1", &fe("cube.fe").to_string_lossy(), &worker_bin(), &lib_path())
        .expect("load cube.fe");
    let (tx, rx) = std::sync::mpsc::channel();
    let m = mgr.clone();
    std::thread::spawn(move || {
        let _ = tx.send(m.request("s1", json!({ "cmd": "run", "command": "r; r; r; r; r; r; g 100000" })));
    });
    std::thread::sleep(std::time::Duration::from_millis(500));
    let m = mgr.clone();
    std::thread::spawn(move || m.kill());
    let res = rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("kill did not unblock the in-flight request");
    let err = res.expect_err("a killed command must not report success");
    assert!(err.contains("crashed"), "got: {err}");
    assert!(mgr.request("s1", json!({ "cmd": "mesh" })).is_err(), "no session survives a kill");
}

/// SIGINT aborts the command (ERROR 2073) and keeps the worker and the partly
/// evolved surface.
#[test]
fn interrupt_stops_a_running_command_and_keeps_the_session() {
    if skip() { return }
    let mgr = std::sync::Arc::new(Manager::default());
    mgr.load_session("s1", &fe("cube.fe").to_string_lossy(), &worker_bin(), &lib_path())
        .expect("load cube.fe");
    mgr.request("s1", json!({ "cmd": "run", "command": "r; r; r; r" })).expect("refine");

    let (tx, rx) = std::sync::mpsc::channel();
    let m = mgr.clone();
    std::thread::spawn(move || {
        let _ = tx.send(m.request("s1", json!({ "cmd": "run", "command": "g 200000" })));
    });
    std::thread::sleep(std::time::Duration::from_millis(800));

    assert!(mgr.interrupt(std::time::Duration::from_secs(5)), "the command did not stop on SIGINT");
    let res = rx.recv_timeout(std::time::Duration::from_secs(5)).expect("request still blocked");
    let err = res.expect_err("an interrupted command reports the abort");
    assert!(err.contains("2073"), "got: {err}");

    // Same session, and the surface kept what it had evolved.
    let after = mgr.request("s1", json!({ "cmd": "run", "command": "" })).expect("session survives");
    assert!(after["energy"].as_f64().unwrap() < 6.0, "surface was reset: {after}");
}

#[test]
fn interrupt_with_nothing_running_is_a_no_op() {
    if skip() { return }
    let mgr = Manager::default();
    mgr.load_session("s1", &fe("cube.fe").to_string_lossy(), &worker_bin(), &lib_path())
        .expect("load cube.fe");
    assert!(mgr.interrupt(std::time::Duration::from_secs(1)));
    // No stray SIGINT reached the idle engine.
    let r = mgr.request("s1", json!({ "cmd": "run", "command": "g 2" })).expect("next command runs");
    assert_eq!(r["ok"], true);
}

/// SE honours SIGINT only while it iterates, so one sent during a mesh fetch is
/// lost and the `g` queued behind it runs on. `interrupt` must keep signalling
/// until the worker is idle.
#[test]
fn interrupt_reaches_a_command_queued_behind_a_slow_mesh_fetch() {
    if skip() { return }
    let mgr = std::sync::Arc::new(Manager::default());
    mgr.load_session("s1", &fe("phelanc.fe").to_string_lossy(), &worker_bin(), &lib_path())
        .expect("load phelanc.fe");
    mgr.request("s1", json!({ "cmd": "run", "command": "r; r; r; r" })).expect("refine");

    // As in the app: the viewer's mesh fetch, with the CLI's `g` queued behind it.
    let m = mgr.clone();
    std::thread::spawn(move || { let _ = m.request("s1", json!({ "cmd": "mesh", "colors": true })); });
    std::thread::sleep(std::time::Duration::from_millis(50));
    let (tx, rx) = std::sync::mpsc::channel();
    let m = mgr.clone();
    std::thread::spawn(move || {
        let _ = tx.send(m.request("s1", json!({ "cmd": "run", "command": "g 200000" })));
    });
    std::thread::sleep(std::time::Duration::from_millis(250));   // inside the mesh fetch

    assert!(mgr.interrupt(std::time::Duration::from_secs(10)), "the queued command was never stopped");
    let res = rx.recv_timeout(std::time::Duration::from_secs(5)).expect("g still running");
    assert!(res.expect_err("g reports the abort").contains("2073"));
    mgr.request("s1", json!({ "cmd": "run", "command": "" })).expect("session survives");
}
