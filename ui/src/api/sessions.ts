import { rpc } from './client'

export interface SessionState {
  session_id:   string
  fe_file:      string
  energy:       number | null
  area:         number | null
  lagrange_order: number | null
  // createSession only: what the datafile's `read` section printed, or the
  // error that abandoned it (se_load runs that section, as SE itself does).
  output?:      string
}

export async function createSession(feFile: string): Promise<SessionState> {
  return rpc<SessionState>('createSession', { fe_file: feFile })
}

// The surface restored from the previous run's auto-saved snapshot, or null.
export async function getRestore(): Promise<SessionState | null> {
  return rpc<SessionState | null>('getRestore')
}

export interface CancelResult {
  cancelled:    boolean
  interrupted?: boolean
  /** Kill fallback only: the file's last snapshot, reloaded into a new
   *  session (#22), or null when there was no snapshot of that file. */
  restored?:    SessionState | null
}

/** Stop the running command. The backend sends SIGINT first: `interrupted`
 *  means the engine aborted the command and kept the worker and the evolved
 *  surface. Otherwise it fell back to killing the worker, and then reloads
 *  the file's last snapshot as `restored` (the in-memory surface is gone). */
export async function cancelSession(): Promise<CancelResult> {
  return rpc<CancelResult>('cancel')
}

/** The read section's output for the log, or null when it printed nothing. */
export function loadOutput(s: SessionState): string | null {
  const out = s.output?.trimEnd()
  return out ? out : null
}

/** Warning for curved-patch models the linear renderer draws wrong, else null. */
export function lagrangeWarning(file: string, s: SessionState): string | null {
  return (s.lagrange_order ?? 1) > 1
    ? `[warning] ${file}: Lagrange order ${s.lagrange_order} — curved patches render as straight edges`
    : null
}
