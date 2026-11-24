//! RPC dispatch. The webview calls `invoke("rpc", { method, params })`: one
//! Tauri command for every method keeps the frontend client a single function.

use crate::worker::{lock, Manager};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Condvar, Mutex};
use tauri::{AppHandle, Manager as TauriManager, Runtime, State};
use tauri_plugin_opener::OpenerExt;

const MAX_UPLOAD_BYTES: usize = 5 * 1024 * 1024;

/// State shared by every RPC call.
#[derive(Default)]
pub struct AppState {
    pub manager: Manager,
    /// The one live session. `Manager` runs a single worker and a load replaces
    /// it, so there is never more than one id worth tracking.
    pub session: Mutex<Option<Value>>,
    /// The startup restore, attempted at most once. Lock order: `restore`
    /// before `session`, never the other way round.
    pub restore: Mutex<Restore>,
    /// Signalled whenever `restore` leaves `Running`.
    restore_done: Condvar,
    /// Channel to the snapshot thread, started on first use (see `persist`).
    persist_tx: Mutex<Option<mpsc::Sender<()>>>,
}

/// Progress of the startup restore.
#[derive(Default)]
pub enum Restore {
    #[default]
    NotTried,
    /// A getRestore is loading the snapshot without holding the `restore` lock,
    /// so a createSession can supersede it. Other getRestore calls wait on
    /// `restore_done` for the result.
    Running,
    /// Finished or superseded: the restored session record, or null.
    Done(Value),
}

// ── paths ───────────────────────────────────────────────────────────────────

fn state_dir() -> PathBuf {
    std::env::var("SE_STATE_DIR").map(PathBuf::from).unwrap_or_else(|_| {
        dirs_home().join(".surface-evolver")
    })
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn user_fe_dir() -> PathBuf {
    state_dir().join("fe")
}

fn resource_dir<R: Runtime>(app: &AppHandle<R>) -> PathBuf {
    app.path().resource_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn bundled_fe_dir<R: Runtime>(app: &AppHandle<R>) -> PathBuf {
    std::env::var("SE_FE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| resource_dir(app).join("fe"))
}

fn lib_path<R: Runtime>(app: &AppHandle<R>) -> PathBuf {
    if let Ok(p) = std::env::var("SE_LIB_PATH") {
        return PathBuf::from(p);
    }
    let (os, ext) = if cfg!(target_os = "macos") { ("macos", "dylib") } else { ("linux", "so") };
    let arch = if cfg!(target_arch = "aarch64") { "arm64" } else { "x64" };
    resource_dir(app).join("native").join(format!("libse-{os}-{arch}.{ext}"))
}

/// The se-worker sidecar, which Tauri places next to the app executable.
fn worker_bin<R: Runtime>(_app: &AppHandle<R>) -> PathBuf {
    if let Ok(p) = std::env::var("SE_WORKER_PATH") {
        return PathBuf::from(p);
    }
    let exe = std::env::current_exe().unwrap_or_default();
    exe.parent().map(|d| d.join("se-worker")).unwrap_or_default()
}

/// Reduces a user-supplied name to a safe bare file name: no directory part,
/// and only `[A-Za-z0-9._-]`.
fn sanitize(name: &str) -> String {
    let base = Path::new(name)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    base.chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' })
        .collect()
}

/// User files shadow bundled ones of the same name.
fn resolve_fe_path<R: Runtime>(app: &AppHandle<R>, fe_file: &str) -> PathBuf {
    let name = sanitize(fe_file);
    let user = user_fe_dir().join(&name);
    if user.exists() { user } else { bundled_fe_dir(app).join(&name) }
}

/// The current UTC time as RFC 3339 to the second, without a date-time crate.
fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86400;
    let (h, m, s) = ((secs % 86400) / 3600, (secs % 3600) / 60, secs % 60);
    // civil-from-days (Howard Hinnant's algorithm)
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

// ── sessions ────────────────────────────────────────────────────────────────

/// The session record the UI and the snapshot use, built from a load's stats.
fn session_from_stats(session_id: &str, fe_file: &str, stats: &Value) -> Value {
    json!({
        "session_id": session_id,
        "fe_file": fe_file,
        "energy": stats["energy"],
        "area": stats["area"],
        "scale": stats["scale"],
        "sdim": stats["sdim"],
        "vertex_count": stats["vertex_count"],
        "edge_count": stats["edge_count"],
        "facet_count": stats["facet_count"],
        "lagrange_order": stats["lagrange_order"],
        "last_accessed": now_iso(),
    })
}

/// The session `params.sessionId` names, if it is the live one. Any other id is
/// a stale handle the UI kept after a load replaced the worker.
fn get_session(state: &AppState, params: &Value) -> Result<(String, Value), String> {
    let sid = params["sessionId"].as_str().ok_or("sessionId is required")?;
    let session = lock(&state.session).clone().ok_or("No file is loaded")?;
    if session["session_id"].as_str() != Some(sid) {
        return Err("This session is no longer loaded — reopen the file".into());
    }
    Ok((sid.to_string(), session))
}

/// Copies a command's new stats into the session record, unless the session was
/// replaced while it ran: those stats describe a surface that is gone.
fn update_session(state: &AppState, sid: &str, result: &Value) {
    let mut guard = lock(&state.session);
    let Some(s) = guard.as_mut().filter(|s| s["session_id"].as_str() == Some(sid)) else {
        return;
    };
    for k in ["energy", "area", "scale"] {
        if !result[k].is_null() {
            s[k] = result[k].clone();
        }
    }
    s["last_accessed"] = json!(now_iso());
}

// ── persistence (best-effort: never surfaces an error) ──────────────────────

fn persist_file() -> PathBuf {
    state_dir().join("last-session.json")
}

/// Asks the snapshot thread to save the live session. It runs off the command
/// path because a dump is a full worker round-trip. A single thread keeps only
/// the newest request: a thread per command would queue behind a long `g N`
/// and could land an older dump after a newer one.
fn persist<R: Runtime>(app: &AppHandle<R>, state: &AppState) {
    let mut guard = lock(&state.persist_tx);
    let tx = guard.get_or_insert_with(|| {
        let (tx, rx) = mpsc::channel::<()>();
        let app = app.clone();
        std::thread::spawn(move || {
            while rx.recv().is_ok() {
                while rx.try_recv().is_ok() {}
                snapshot(&app.state());
            }
        });
        tx
    });
    let _ = tx.send(());
}

/// Saves the live session's dump to `last-session.json`. Writes a temp file and
/// renames it, so a quit mid-write can't truncate the previous snapshot.
fn snapshot(state: &AppState) {
    let Some(session) = lock(&state.session).clone() else { return };
    let Some(sid) = session["session_id"].as_str() else { return };
    let Ok(dump) = state.manager.request(sid, json!({ "cmd": "dump" })) else { return };
    let saved = json!({
        "fe_file": session["fe_file"],
        "energy": session["energy"],
        "area": session["area"],
        "dmp": dump["content"],
        "saved_at": now_iso(),
    });
    let _ = fs::create_dir_all(state_dir());
    let tmp = persist_file().with_extension("json.tmp");
    if fs::write(&tmp, saved.to_string()).is_ok() {
        let _ = fs::rename(&tmp, persist_file());
    }
}

/// Loads the saved snapshot into a fresh worker and returns its session record,
/// without registering it: the caller decides whether it still should be.
/// `only_file` limits it to a snapshot of that datafile. A failed load keeps the
/// snapshot, since a missing library or sidecar is as likely as a bad dump.
fn load_snapshot<R: Runtime>(app: &AppHandle<R>, state: &AppState, only_file: Option<&str>) -> Option<Value> {
    let raw = fs::read_to_string(persist_file()).ok()?;
    let saved: Value = serde_json::from_str(&raw).ok()?;
    let (dmp, fe_file) = (saved["dmp"].as_str()?, saved["fe_file"].as_str()?);
    if only_file.is_some_and(|f| f != fe_file) {
        return None;
    }
    let tmp = std::env::temp_dir().join(format!("se-restore-{}.dmp", uuid::Uuid::new_v4()));
    fs::write(&tmp, dmp).ok()?;
    let sid = uuid::Uuid::new_v4().to_string();
    let loaded = state.manager.load_session(
        &sid,
        &tmp.to_string_lossy(),
        &worker_bin(app),
        &lib_path(app),
    );
    let _ = fs::remove_file(&tmp);
    loaded.ok().map(|stats| session_from_stats(&sid, fe_file, &stats))
}

/// Whether `s` is the session the worker currently serves.
fn is_live(state: &AppState, s: &Value) -> bool {
    !s.is_null()
        && lock(&state.session)
            .as_ref()
            .is_some_and(|live| live["session_id"] == s["session_id"])
}

// ── the command ─────────────────────────────────────────────────────────────

/// The single Tauri command. Runs on a blocking thread: a `g N` can keep the
/// worker busy for minutes.
#[tauri::command]
pub async fn rpc<R: Runtime>(app: AppHandle<R>, method: String, params: Value) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let result = dispatch(&app, &method, params);
        result.map_err(|e| end_session_if_worker_gone(&app.state(), e))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// How long Stop keeps interrupting before it kills the worker. A `g` stops
/// within one SIGINT; the wait covers a mesh fetch it may be queued behind,
/// which SIGINT can't stop (10 s covers about 400k vertices).
const CANCEL_GRACE: std::time::Duration = std::time::Duration::from_secs(10);

/// Prefix of every error that means the session is gone; the frontend clears its
/// session on it. Must match `SESSION_ENDED` in ui/src/api/client.ts.
pub const SESSION_ENDED: &str = "Session ended:";

/// Ends the session when a failure leaves no worker alive (a failed
/// createSession, a worker that died mid-request), so later commands don't fail
/// against a dead surface the UI still shows.
fn end_session_if_worker_gone(state: &AppState, err: String) -> String {
    if state.manager.has_worker() {
        return err;
    }
    match lock(&state.session).take() {
        Some(_) => format!("{SESSION_ENDED} {err}"),
        None => err,
    }
}

/// Routes one RPC call to its handler.
fn dispatch<R: Runtime>(app: &AppHandle<R>, method: &str, params: Value) -> Result<Value, String> {
    let state: State<AppState> = app.state();
    match method {
        "getRestore" => Ok(get_restore(app, &state)),

        "listFiles" => {
            let list = |dir: &Path| -> Vec<String> {
                fs::read_dir(dir)
                    .map(|rd| {
                        rd.filter_map(|e| e.ok())
                            .map(|e| e.file_name().to_string_lossy().into_owned())
                            .filter(|f| f.ends_with(".fe"))
                            .collect()
                    })
                    .unwrap_or_default()
            };
            let mut files: Vec<String> = list(&bundled_fe_dir(app));
            files.extend(list(&user_fe_dir()));
            files.sort();
            files.dedup();
            Ok(json!(files))
        }

        "createSession" => create_session(app, &state, &params),

        "cancel" => Ok(cancel(app, &state)),

        "uploadFile" => {
            let filename = params["filename"].as_str().ok_or("filename and content are required")?;
            let content = params["content"].as_str().ok_or("filename and content are required")?;
            if !filename.ends_with(".fe") {
                return Err("Only .fe files are accepted".into());
            }
            let safe = sanitize(filename);
            if resolve_fe_path(app, &safe).exists() {
                return Err(format!("File '{safe}' already exists — rename and retry"));
            }
            let buf = base64_decode(content).ok_or("content is not valid base64")?;
            if buf.len() > MAX_UPLOAD_BYTES {
                return Err("File exceeds 5 MB limit".into());
            }
            fs::create_dir_all(user_fe_dir()).map_err(|e| e.to_string())?;
            fs::write(user_fe_dir().join(&safe), &buf).map_err(|e| e.to_string())?;
            // No model check here: se_load rejects unsupported models with a clear message.
            Ok(json!({ "filename": safe, "size_bytes": buf.len() }))
        }

        "exportDmp" => {
            let (sid, session) = get_session(&state, &params)?;
            let result = state.manager.request(&sid, json!({ "cmd": "dump" }))?;
            let fe = session["fe_file"].as_str().unwrap_or("surface");
            let stem = Path::new(fe).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "surface".into());
            let ts = now_iso().replace([':', '.'], "-");
            let ts = &ts[..ts.len().min(19)];
            Ok(json!({ "filename": format!("{stem}_{ts}.dmp"), "content": result["content"] }))
        }

        "exportFe" => {
            let (_sid, session) = get_session(&state, &params)?;
            let fe = session["fe_file"].as_str().unwrap_or_default();
            let content = fs::read_to_string(resolve_fe_path(app, fe)).map_err(|e| e.to_string())?;
            Ok(json!({ "filename": sanitize(fe), "content": content }))
        }

        // Needs no session: a file that fails to load must still reach the editor.
        "readFile" => {
            let fe_file = params["filename"].as_str().ok_or("filename is required")?;
            let path = resolve_fe_path(app, fe_file);
            let content = fs::read_to_string(&path)
                .map_err(|_| format!("File not found: {}", sanitize(fe_file)))?;
            Ok(json!({ "filename": sanitize(fe_file), "content": content }))
        }

        "updateFile" => {
            let filename = params["filename"].as_str().ok_or("filename and content are required")?;
            let content = params["content"].as_str().ok_or("filename and content are required")?;
            if content.len() > MAX_UPLOAD_BYTES {
                return Err("File exceeds 5 MB limit".into());
            }
            let safe = sanitize(filename);
            fs::create_dir_all(user_fe_dir()).map_err(|e| e.to_string())?;
            fs::write(user_fe_dir().join(&safe), content).map_err(|e| e.to_string())?;
            Ok(json!({ "filename": safe, "size_bytes": content.len() }))
        }

        "saveExport" => {
            let filename = params["filename"].as_str().ok_or("filename and content are required")?;
            let content = params["content"].as_str().ok_or("filename and content are required")?;
            let safe = sanitize(filename);
            let dir = dirs_home().join("Downloads");
            fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            // Avoid clobbering: foo.dmp, foo (1).dmp, ...
            let (stem, ext) = match safe.rfind('.') {
                Some(i) if i > 0 => (&safe[..i], &safe[i..]),
                _ => (safe.as_str(), ""),
            };
            let mut dest = dir.join(&safe);
            let mut i = 1;
            while dest.exists() {
                dest = dir.join(format!("{stem} ({i}){ext}"));
                i += 1;
            }
            fs::write(&dest, content).map_err(|e| e.to_string())?;
            let _ = app.opener().reveal_item_in_dir(&dest);
            Ok(json!({ "path": dest.to_string_lossy() }))
        }

        "runCommand" => {
            let (sid, _) = get_session(&state, &params)?;
            // Empty is valid: nothing runs, and the reply reports the current energy and area.
            let command = params["command"].as_str().ok_or("command is required")?;
            let result = state.manager
                .request(&sid, json!({ "cmd": "run", "command": command }))
                ?;
            update_session(&state, &sid, &result);
            persist(app, &state);
            Ok(result)
        }

        "getMesh" => {
            let (sid, _) = get_session(&state, &params)?;
            let mut req = json!({ "cmd": "mesh" });
            if params["colors"].as_bool() == Some(true) {
                req["colors"] = json!(true);
            }
            state.manager.request(&sid, req)
        }

        "vertexInfo" => {
            let (sid, _) = get_session(&state, &params)?;
            let vpos = params["vpos"].as_i64().filter(|v| *v >= 0).ok_or("vpos is required")?;
            state.manager
                .request(&sid, json!({ "cmd": "vertex_info", "vpos": vpos }))
                
        }

        "topo" => {
            let (sid, _) = get_session(&state, &params)?;
            let op = params["op"].as_str().filter(|o| !o.is_empty()).ok_or("op is required")?;
            // One pass per call; `u 5` through runCommand covers repeated passes.
            let req = json!({ "cmd": "topo", "op": op });
            let result = state.manager.request(&sid, req)?;
            update_session(&state, &sid, &result);
            persist(app, &state);
            Ok(result)
        }

        other => Err(format!("Unknown RPC method: {other}")),
    }
}

/// The previous run's session, restored from its snapshot at most once.
///
/// Claims the attempt under the `restore` lock, then loads outside it, so a file
/// opened meanwhile isn't blocked, and that open wins. A caller that finds a
/// restore running waits for its result instead of getting null (React
/// StrictMode calls this twice in dev and ignores the first answer). A finished
/// restore is returned only while it is still the live session.
fn get_restore<R: Runtime>(app: &AppHandle<R>, state: &AppState) -> Value {
    {
        let mut r = state
            .restore_done
            .wait_while(lock(&state.restore), |r| matches!(r, Restore::Running))
            .unwrap_or_else(|e| e.into_inner());
        match &*r {
            Restore::Done(v) => {
                return if is_live(state, v) { v.clone() } else { Value::Null };
            }
            _ => *r = Restore::Running,
        }
    }
    let user_loaded = lock(&state.session).is_some();
    let restored = if user_loaded { None } else { load_snapshot(app, state, None) };
    let mut r = lock(&state.restore);
    let session = match (&*r, restored) {
        (Restore::Running, Some(s)) => {
            *lock(&state.session) = Some(s.clone());
            s
        }
        // Superseded (that load already killed this worker), or nothing to restore.
        _ => Value::Null,
    };
    if matches!(*r, Restore::Running) {
        *r = Restore::Done(session.clone());
    }
    state.restore_done.notify_all();
    session
}

/// Loads a datafile into a fresh worker, replacing the old one, and makes it the
/// live session.
///
/// It supersedes the startup restore: marking the restore done stops one from
/// starting, and one already running sees it when its load returns. The reply
/// adds `output`, what the datafile's `read` section printed, for the UI's log
/// only; it isn't stored in the session record.
fn create_session<R: Runtime>(app: &AppHandle<R>, state: &AppState, params: &Value) -> Result<Value, String> {
    let fe_file = params["fe_file"].as_str().ok_or("fe_file is required")?;
    let fe_path = resolve_fe_path(app, fe_file);
    if !fe_path.exists() {
        return Err(format!("File not found: {}", sanitize(fe_file)));
    }
    {
        let mut r = lock(&state.restore);
        if !matches!(*r, Restore::Done(_)) {
            *r = Restore::Done(Value::Null);
            state.restore_done.notify_all();
        }
    }
    let sid = uuid::Uuid::new_v4().to_string();
    let stats = state
        .manager
        .load_session(&sid, &fe_path.to_string_lossy(), &worker_bin(app), &lib_path(app))
        ?;
    let session = session_from_stats(&sid, fe_file, &stats);
    *lock(&state.session) = Some(session.clone());
    persist(app, state);
    let mut reply = session;
    reply["output"] = stats["output"].clone();
    Ok(reply)
}

/// Stop. Sends SIGINT first: the engine aborts the command but keeps the worker
/// and its evolved surface. Only if the command ignores it for `CANCEL_GRACE`
/// does it kill the worker, then reload that file's last snapshot (taken after
/// the previous command) instead of leaving the user to reopen the file.
/// `interrupt` and `kill` take only `proc`, so they work while a request holds `io`.
fn cancel<R: Runtime>(app: &AppHandle<R>, state: &AppState) -> Value {
    if state.manager.interrupt(CANCEL_GRACE) && state.manager.has_worker() {
        return json!({ "cancelled": true, "interrupted": true });
    }
    state.manager.kill();
    let ended = lock(&state.session).take();
    let restored = ended
        .as_ref()
        .and_then(|s| s["fe_file"].as_str())
        .and_then(|f| load_snapshot(app, state, Some(f)));
    if let Some(s) = &restored {
        *lock(&state.session) = Some(s.clone());
    }
    json!({ "cancelled": true, "interrupted": false, "restored": restored })
}

/// Decodes standard base64, ignoring whitespace: a few lines instead of a
/// dependency for one caller.
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    const TBL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut rev = [255u8; 256];
    for (i, &c) in TBL.iter().enumerate() {
        rev[c as usize] = i as u8;
    }
    let s: Vec<u8> = s.bytes().filter(|b| !b" \n\r\t".contains(b)).collect();
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    for chunk in s.chunks(4) {
        let mut vals = [0u8; 4];
        let mut len = 0;
        for (i, &c) in chunk.iter().enumerate() {
            if c == b'=' {
                break;
            }
            let v = rev[c as usize];
            if v == 255 {
                return None;
            }
            vals[i] = v;
            len = i + 1;
        }
        if len >= 2 {
            out.push((vals[0] << 2) | (vals[1] >> 4));
        }
        if len >= 3 {
            out.push((vals[1] << 4) | (vals[2] >> 2));
        }
        if len == 4 {
            out.push((vals[2] << 6) | vals[3]);
        }
    }
    Some(out)
}

// Pure helpers only, no worker or libse. End-to-end coverage against a real
// worker is in src-tauri/tests/{manager,dispatch}.rs.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_directory_components() {
        assert_eq!(sanitize("../../etc/passwd"), "passwd");
        assert_eq!(sanitize("/etc/passwd"), "passwd");
        assert_eq!(sanitize("a/b/c.fe"), "c.fe");
    }

    #[test]
    fn sanitize_replaces_unsafe_characters() {
        assert_eq!(sanitize("weird name?.fe"), "weird_name_.fe");
        assert_eq!(sanitize("ok-name_1.2.fe"), "ok-name_1.2.fe");
    }

    #[test]
    fn base64_round_trips_arbitrary_bytes() {
        for input in [&b""[..], b"a", b"ab", b"abc", b"surface evolver \0\xff"] {
            let encoded = base64_encode_for_test(input);
            assert_eq!(base64_decode(&encoded).unwrap(), input);
        }
    }

    #[test]
    fn base64_rejects_invalid_input() {
        assert!(base64_decode("not base64!!").is_none());
        assert!(base64_decode("@@@@").is_none());
    }

    fn base64_encode_for_test(bytes: &[u8]) -> String {
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

    #[test]
    fn now_iso_has_rfc3339_shape() {
        let s = now_iso();
        assert_eq!(s.len(), 20, "got: {s}");
        assert!(s.ends_with('Z'), "got: {s}");
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[7..8], "-");
        assert_eq!(&s[10..11], "T");
    }

    #[test]
    fn session_from_stats_copies_the_expected_fields() {
        let stats = json!({
            "energy": 1.5, "area": 2.5, "scale": 0.1, "sdim": 3,
            "vertex_count": 14, "edge_count": 24, "facet_count": 12,
            "lagrange_order": 1,
        });
        let session = session_from_stats("sid-1", "cube.fe", &stats);
        assert_eq!(session["session_id"], "sid-1");
        assert_eq!(session["fe_file"], "cube.fe");
        assert_eq!(session["vertex_count"], 14);
        assert!(session["last_accessed"].is_string());
    }

    #[test]
    fn get_session_rejects_a_stale_session_id() {
        let state = AppState::default();
        *lock(&state.session) = Some(json!({ "session_id": "current" }));
        let err = get_session(&state, &json!({ "sessionId": "stale" })).unwrap_err();
        assert!(err.contains("no longer loaded"), "got: {err}");
    }

    #[test]
    fn get_session_requires_a_loaded_file() {
        let state = AppState::default();
        let err = get_session(&state, &json!({ "sessionId": "anything" })).unwrap_err();
        assert_eq!(err, "No file is loaded");
    }

    #[test]
    fn update_session_is_a_noop_if_the_session_was_replaced() {
        let state = AppState::default();
        *lock(&state.session) = Some(json!({ "session_id": "current", "energy": 1.0 }));
        update_session(&state, "old", &json!({ "energy": 999.0 }));
        assert_eq!(lock(&state.session).as_ref().unwrap()["energy"], 1.0);

        update_session(&state, "current", &json!({ "energy": 2.0 }));
        assert_eq!(lock(&state.session).as_ref().unwrap()["energy"], 2.0);
    }

    #[test]
    fn resolve_fe_path_prefers_the_user_file_over_the_bundled_one() {
        let state_dir = std::env::temp_dir().join(format!("se-rpc-test-state-{}", std::process::id()));
        let fe_dir = std::env::temp_dir().join(format!("se-rpc-test-fe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&state_dir);
        let _ = std::fs::remove_dir_all(&fe_dir);
        std::fs::create_dir_all(state_dir.join("fe")).unwrap();
        std::fs::create_dir_all(&fe_dir).unwrap();
        std::fs::write(fe_dir.join("shared.fe"), "bundled").unwrap();
        std::fs::write(state_dir.join("fe/shared.fe"), "user").unwrap();

        // SAFETY: this is the only test in this module that touches these
        // env vars, so there is no cross-test race within this binary.
        unsafe {
            std::env::set_var("SE_STATE_DIR", &state_dir);
            std::env::set_var("SE_FE_DIR", &fe_dir);
        }

        let app = tauri::test::mock_app();
        let handle = app.handle();
        let resolved = resolve_fe_path(handle, "shared.fe");
        assert_eq!(std::fs::read_to_string(&resolved).unwrap(), "user");

        unsafe {
            std::env::remove_var("SE_STATE_DIR");
            std::env::remove_var("SE_FE_DIR");
        }
        let _ = std::fs::remove_dir_all(&state_dir);
        let _ = std::fs::remove_dir_all(&fe_dir);
    }
}
