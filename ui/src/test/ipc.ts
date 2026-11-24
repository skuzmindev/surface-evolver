import { vi } from 'vitest'
import { mockIPC } from '@tauri-apps/api/mocks'

/**
 * Every invoke() in a test lands here, through Tauri's own IPC mock, so tests
 * assert exactly what would reach Rust. Installed once per test file, before
 * anything imports api/client.ts: another mockIPC() would drop client.ts's
 * `se-menu` listener, so reset `ipc` instead of re-mocking.
 */
export const ipc = vi.fn<(cmd: string, args?: unknown) => unknown>()

mockIPC((cmd, args) => ipc(cmd, args), { shouldMockEvents: true })

/** Answer `rpc(method)` calls from a table; anything else resolves to null. */
export function answerRpc(table: Record<string, unknown>): void {
  ipc.mockImplementation((cmd, args) => {
    const { method } = (args ?? {}) as { method?: string }
    if (cmd !== 'rpc' || !method || !(method in table)) return null
    const v = table[method]
    if (v instanceof Error) throw v.message // Tauri rejects with a plain string
    return v
  })
}
