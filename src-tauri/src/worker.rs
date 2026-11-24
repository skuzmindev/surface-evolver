//! Spawns, talks to and kills the se-worker sidecar.
//!
//! Each load starts a fresh worker, because libse can't be initialised twice in
//! one process. `io` serialises request/response I/O; the separate `proc` lets a
//! load or Stop reach a hung worker without waiting on `io`.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct WorkerIo {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    pub active_session: String,
}

#[derive(Default)]
pub struct Manager {
    /// Held for a whole request/response exchange.
    pub io: Mutex<Option<WorkerIo>>,
    /// The process handle, so `kill` never touches `io`.
    pub proc: Mutex<Option<Child>>,
    /// Requests in flight, including those still waiting for `io`. `interrupt`
    /// waits for zero: an unlocked `io` also occurs between a finished request
    /// and the next one queued behind it.
    pending: AtomicUsize,
}

/// Locks `m`, recovering from poison: a panic mid-lock leaves these `Option`s
/// valid, and `unwrap()` would break every later call.
pub fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Reads the worker's next result line, skipping blank and progress lines.
/// EOF means the worker died.
fn read_result(stdout: &mut BufReader<ChildStdout>) -> Result<Value, String> {
    loop {
        let mut line = String::new();
        let n = stdout
            .read_line(&mut line)
            .map_err(|e| format!("SE worker read failed: {e}"))?;
        if n == 0 {
            return Err("SE engine process crashed — reload the file to continue"
                .to_string());
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let msg: Value =
            serde_json::from_str(line).map_err(|e| format!("SE worker sent bad JSON: {e}"))?;
        match msg["type"].as_str() {
            Some("result") => return Ok(msg),
            // dlopen or se_init failed.
            Some("fatal") => {
                return Err(format!(
                    "SE engine failed to start: {}",
                    msg["error"].as_str().unwrap_or("unknown error")
                ))
            }
            _ => continue,
        }
    }
}

fn check_result(msg: &Value) -> Result<(), String> {
    if msg["ok"].as_bool() == Some(true) {
        return Ok(());
    }
    let text = msg["se_error"].as_str()
        .or_else(|| msg["error"].as_str())
        .unwrap_or("SE worker returned an error");
    Err(text.to_string())
}

impl Manager {
    /// Kills and reaps the worker. A request blocked on its output then gets EOF.
    pub fn kill(&self) {
        if let Some(mut child) = lock(&self.proc).take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Asks the running command to stop while keeping the surface: SE's SIGINT
    /// handler aborts it with ERROR 2073, and the worker survives. Returns true
    /// once no request is in flight (an idle worker gets no signal), false if
    /// the command didn't stop within `grace`, so the caller can kill instead.
    /// Takes only `proc`, never `io`.
    ///
    /// SE honours SIGINT only while it iterates, so one that lands during a mesh
    /// fetch is lost and a `g` queued behind it runs on. The signal is re-sent
    /// every `RESEND` until idle; a stray one at an idle worker is harmless.
    // shortcut: a request started after Stop also counts as pending; the UI blocks
    // new commands while one runs, so it can't happen from there.
    pub fn interrupt(&self, grace: Duration) -> bool {
        let idle = || self.pending.load(Ordering::SeqCst) == 0;
        if idle() {
            return true;
        }
        let Some(pid) = lock(&self.proc).as_ref().map(|c| c.id()) else { return false };
        extern "C" { fn kill(pid: i32, sig: i32) -> i32; }
        const SIGINT: i32 = 2; // the same on macOS and Linux
        const RESEND: Duration = Duration::from_millis(250);
        let deadline = Instant::now() + grace;
        let mut next_signal = Instant::now();
        while Instant::now() < deadline {
            if idle() {
                return true;
            }
            if Instant::now() >= next_signal {
                unsafe { kill(pid as i32, SIGINT) };
                next_signal = Instant::now() + RESEND;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        false
    }

    /// Whether a worker process is alive (or not yet reaped). Takes only `proc`,
    /// so it never waits behind a running command.
    pub fn has_worker(&self) -> bool {
        lock(&self.proc).is_some()
    }

    /// Spawns a fresh worker for `session_id` and loads `fe_path` into it,
    /// killing any previous worker. A failed load kills the new worker too:
    /// engine state after a failed load is undefined.
    pub fn load_session(
        &self,
        session_id: &str,
        fe_path: &str,
        worker_bin: &PathBuf,
        lib_path: &PathBuf,
    ) -> Result<Value, String> {
        // Kill before taking `io`: a hung command holds `io` until its read fails.
        self.kill();
        let mut io = lock(&self.io);
        // Again: another load (a startup restore) may have spawned one meanwhile,
        // which would otherwise be overwritten in `proc` and outlive the app.
        self.kill();
        *io = None;

        let mut cmd = Command::new(worker_bin);
        cmd.env("SE_LIB_PATH", lib_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("failed to spawn SE worker ({}): {e}", worker_bin.display()))?;
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        *lock(&self.proc) = Some(child);
        *io = Some(WorkerIo { stdin, stdout, active_session: session_id.to_string() });

        let w = io.as_mut().unwrap();
        let result = (|| {
            send(&mut w.stdin, &json!({ "cmd": "load", "path": fe_path }))?;
            let msg = read_result(&mut w.stdout)?;
            check_result(&msg)?;
            Ok(msg)
        })();
        if result.is_err() {
            *io = None;
            self.kill();
        }
        result
    }

    /// Sends `req` to the worker serving `session_id` and returns its result.
    pub fn request(&self, session_id: &str, req: Value) -> Result<Value, String> {
        // Counted before waiting for `io`, so a queued request never looks idle.
        self.pending.fetch_add(1, Ordering::SeqCst);
        struct Done<'a>(&'a AtomicUsize);
        impl Drop for Done<'_> { fn drop(&mut self) { self.0.fetch_sub(1, Ordering::SeqCst); } }
        let _done = Done(&self.pending);
        let mut io = lock(&self.io);
        let w = io.as_mut().ok_or_else(|| {
            format!("No active SE worker for session {session_id}")
        })?;
        if w.active_session != session_id {
            return Err(format!(
                "Session {session_id} is not currently loaded (active: {})",
                w.active_session
            ));
        }
        send(&mut w.stdin, &req)?;
        match read_result(&mut w.stdout) {
            Ok(msg) => {
                check_result(&msg)?;
                Ok(msg)
            }
            Err(e) => {
                // Reap on any read error, not just EOF, or a live worker holds
                // libse until the next load.
                *io = None;
                self.kill();
                Err(e)
            }
        }
    }
}

fn send(stdin: &mut ChildStdin, msg: &Value) -> Result<(), String> {
    writeln!(stdin, "{msg}").map_err(|e| format!("SE worker write failed: {e}"))
}
