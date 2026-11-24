import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useStore } from '../store/useStore'

/** Tauri rejects with a plain string; callers expect an Error. */
export function toError(e: unknown): Error {
  return e instanceof Error ? e : new Error(String(e))
}

/** Calls the backend: one Tauri command that dispatches on `method` (src-tauri/src/rpc.rs). */
export function rpc<T>(method: string, params?: unknown): Promise<T> {
  return invoke<T>('rpc', { method, params: params ?? null }).catch((e) => {
    const err = toError(e)
    // The backend ended the session (no worker survived), so drop ours too and
    // keep the tab to reopen. The prefix is a signal, not text for the user.
    if (err.message.startsWith(SESSION_ENDED)) {
      useStore.getState().clearSession()
      throw new Error(err.message.slice(SESSION_ENDED.length).trimStart())
    }
    throw err
  })
}

/** Must match SESSION_ENDED in src-tauri/src/rpc.rs. */
export const SESSION_ENDED = 'Session ended:'

// Native menu clicks arrive as a Tauri event; re-dispatch them as the DOM event useMenuAction listens for.
listen<string>('se-menu', (e) => {
  window.dispatchEvent(new CustomEvent('se-menu', { detail: e.payload }))
})
