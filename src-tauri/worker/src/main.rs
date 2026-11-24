//! The SE worker sidecar: one libse per process, so the app spawns a fresh one
//! for every loaded file (src-tauri/src/worker.rs).
//!
//! Protocol: line-delimited JSON on stdin/stdout.
//!   stdin  <- {"cmd":"load"|"run"|"mesh"|..., ...}
//!   stdout -> {"type":"result","ok":true|false, ...}
//!          -> {"type":"fatal","error":...}  if dlopen / se_init fails
//!
//! The app cancels with SIGINT, which aborts an iteration and keeps the surface,
//! and kills the process only if that doesn't stop it.

mod ffi;
mod handlers;

use handlers::{Worker, OUT_BUF_SIZE};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::sync::{Mutex, OnceLock};

/// The JSON reply channel. Not plain stdout: see `take_json_pipe`.
static PIPE: OnceLock<Mutex<Box<dyn Write + Send>>> = OnceLock::new();

/// Takes the reply pipe off fd 1. SE printf()s some output straight to fd 1, and
/// one stray line would corrupt the protocol, so replies go to a private dup and
/// fd 1 is pointed at stderr, where engine output lands in the app's log.
fn take_json_pipe() -> Box<dyn Write + Send> {
    use std::os::fd::AsFd;
    extern "C" { fn dup2(src: i32, dst: i32) -> i32; }
    let pipe = std::io::stdout().as_fd().try_clone_to_owned().expect("dup stdout");
    if unsafe { dup2(2, 1) } < 0 {
        eprintln!("se-worker: dup2(stderr, stdout) failed; engine output may corrupt IPC");
    }
    Box::new(std::fs::File::from(pipe))
}

/// Takes the request pipe off fd 0. SE's prompt() reads stdin, so a prompt the
/// INTERACTIVE_CMDS guard can't see (`j; g`, a procedure) would block on the pipe
/// and swallow the next request. Requests come in on a private dup, and fd 0
/// becomes /dev/null, where every prompt gets EOF at once and returns.
fn take_ipc_input() -> std::fs::File {
    use std::os::fd::{AsFd, AsRawFd};
    extern "C" { fn dup2(src: i32, dst: i32) -> i32; }
    let ipc = std::io::stdin().as_fd().try_clone_to_owned().expect("dup stdin");
    match std::fs::File::open("/dev/null") {
        Ok(null) => { unsafe { dup2(null.as_raw_fd(), 0) }; }
        Err(e) => eprintln!("se-worker: can't open /dev/null ({e}); engine prompts may block"),
    }
    std::fs::File::from(ipc)
}

/// Writes one reply line in a single `write_all`. The pipe is an unbuffered
/// `File`, and writing through `Display` would cost a syscall per JSON token.
fn send(v: &Value) {
    let mut out = PIPE.get_or_init(|| Mutex::new(take_json_pipe())).lock().unwrap_or_else(|e| e.into_inner());
    let mut line = serde_json::to_vec(v).unwrap_or_default();
    line.push(b'\n');
    let _ = out.write_all(&line);
    let _ = out.flush();
}

fn fatal(error: String) -> ! {
    send(&json!({ "type": "fatal", "error": error }));
    std::process::exit(1);
}

fn main() {
    // Before dlopen: nothing may write fd 1 or read fd 0 until the pipes are moved.
    PIPE.get_or_init(|| Mutex::new(take_json_pipe()));
    let ipc_input = std::io::BufReader::new(take_ipc_input());
    let lib_path = std::env::var("SE_LIB_PATH").unwrap_or_else(|_| "/app/libse.so".to_string());

    let se = match ffi::Se::load_library(&lib_path) {
        Ok(se) => se,
        Err(e) => fatal(format!("Failed to open library \"{lib_path}\": {e}")),
    };
    if unsafe { (se.init)() } != 0 {
        fatal("se_init() failed".to_string());
    }

    let mut w = Worker {
        se,
        out_buf: vec![0u8; OUT_BUF_SIZE],
        err_buf: vec![0u8; OUT_BUF_SIZE],
        loaded_file: None,
        last_mesh_hash: None,
    };

    // Handlers are synchronous, so requests serialise: a long se_run blocks the loop.
    for line in ipc_input.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let req: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => {
                send(&json!({ "type": "result", "ok": false, "error": "invalid JSON" }));
                continue;
            }
        };

        let cmd = req["cmd"].as_str().unwrap_or("");
        let result = match cmd {
            "load" => w.load(&req),
            "run" => w.run(&req),
            "mesh" => w.mesh(&req),
            "topo" => w.topo(&req),
            "vertex_info" => w.vertex_info(&req),
            "dump" => w.dump(),
            other => json!({ "ok": false, "error": format!("unknown cmd: {other}") }),
        };

        // {"type":"result", ...result}, with type first.
        let mut msg = serde_json::Map::new();
        msg.insert("type".into(), json!("result"));
        if let Value::Object(m) = result {
            for (k, v) in m {
                msg.insert(k, v);
            }
        }
        send(&Value::Object(msg));
    }
}
