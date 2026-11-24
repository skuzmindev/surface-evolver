//! Helpers for the app's integration tests. They need a built libse and
//! se-worker, whose paths nothing provides automatically: the worker is a
//! separate crate, so there is no `CARGO_BIN_EXE_*` for it.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

pub fn fe(name: &str) -> PathBuf {
    repo_root().join("fe").join(name)
}

pub fn lib_path() -> PathBuf {
    PathBuf::from(std::env::var("SE_LIB_PATH").expect("checked by skip()"))
}

fn worker_bin_str() -> Option<String> {
    if let Ok(p) = std::env::var("SE_WORKER_PATH") {
        return Some(p);
    }
    // Newest build wins: a stale release binary from `bun run dev` would otherwise be tested.
    ["release", "debug"]
        .iter()
        .map(|profile| repo_root().join("src-tauri/worker/target").join(profile).join("se-worker"))
        .filter_map(|p| Some((p.metadata().ok()?.modified().ok()?, p)))
        .max_by_key(|(mtime, _)| *mtime)
        .map(|(_, p)| p.to_string_lossy().into_owned())
}

pub fn worker_bin() -> PathBuf {
    PathBuf::from(worker_bin_str().expect("checked by skip()"))
}

/// Whether to skip because libse or the worker isn't built, which keeps
/// `cargo test` usable before the native build. Under CI a missing prerequisite
/// fails instead, where a skip would be a silent pass.
pub fn skip() -> bool {
    let ci = std::env::var_os("CI").is_some();
    if std::env::var("SE_LIB_PATH").is_err() {
        assert!(!ci, "SE_LIB_PATH unset under CI — refusing to skip");
        eprintln!("SE_LIB_PATH unset — skipping app-layer integration test");
        return true;
    }
    if worker_bin_str().is_none() {
        assert!(!ci, "se-worker binary not found under CI — refusing to skip");
        eprintln!("se-worker binary not found (set SE_WORKER_PATH or build src-tauri/worker) — skipping");
        return true;
    }
    false
}

/// A fresh scratch directory, named by pid and `tag` so parallel test binaries
/// never collide.
pub fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("se-apptest-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn cleanup(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}
