import { rpc } from './client'

export interface SessionState {
  session_id:   string
  fe_file:      string
  energy:       number | null
  area:         number | null
  lagrange_order: number | null
  /** createSession only: what the datafile's `read` section printed, or its error. */
  output?:      string
}

export async function createSession(feFile: string): Promise<SessionState> {
  return rpc<SessionState>('createSession', { fe_file: feFile })
}

/** The previous run's surface, restored from its snapshot, or null. */
export async function getRestore(): Promise<SessionState | null> {
  return rpc<SessionState | null>('getRestore')
}

export interface CancelResult {
  cancelled:    boolean
  interrupted?: boolean
  /** After a kill only: the file's last snapshot in a new session, or null if there was none. */
  restored?:    SessionState | null
}

/**
 * Stops the running command. `interrupted` means it aborted and the evolved
 * surface is kept; otherwise the worker was killed and `restored` is the file's
 * last snapshot, if any.
 */
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
