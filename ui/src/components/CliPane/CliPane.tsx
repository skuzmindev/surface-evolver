import { useState, useRef } from 'react'
import { runCommand, runTopo, type TopoOp } from '../../api/simulation'
import { cancelSession } from '../../api/sessions'
import { useStore } from '../../store/useStore'
import { useMenuAction } from '../../hooks/useMenuAction'
import OutputLog from './OutputLog'

/** Topology operations the native Run menu triggers. */
const TOPO_OPS: { op: TopoOp; label: string }[] = [
  { op: 'refine',     label: 'Refine'  },
  { op: 'equi',       label: 'Equiang' },
  { op: 'vertex_avg', label: 'V-Avg'   },
  { op: 'pop',        label: 'Pop'     },
]

/** A topology op's change counts as one line, e.g. "vertices +12, edgeswap +3". */
function summarize(counts: Record<string, number>): string {
  const parts = Object.entries(counts).map(([k, v]) => `${k.replace(/_/g, ' ')} ${v > 0 ? '+' : ''}${v}`)
  return parts.length ? parts.join(', ') : 'no change'
}

/** The command line: runs SE commands and topology ops, shows their output, and stops them. */
export default function CliPane() {
  const { sessionId, outputLog, appendLog, setStats, bumpMeshVersion, clearSession, setSession } = useStore()
  const [input, setInput] = useState('')
  const [busy, setBusy]   = useState(false)
  const inputRef = useRef<HTMLInputElement>(null)

  // Set by Stop: the stopped command's own failure is expected, not worth reporting.
  const stoppedRef = useRef(false)
  function logCommandError(err: unknown) {
    if (stoppedRef.current) return
    const msg = err instanceof Error ? err.message : String(err)
    // ERROR 2073 only ever answers our own SIGINT (Stop, or closing the tab).
    if (/\bERROR 2073\b/.test(msg)) return
    appendLog(`[error] ${msg}`)
  }

  /**
   * Stop. The backend interrupts the command and keeps the evolved surface. If
   * that fails it kills the worker and reloads the file's last snapshot when
   * there is one; otherwise the tab stays so the file can be reopened.
   */
  async function handleStop() {
    if (!sessionId) return
    stoppedRef.current = true
    try {
      const res = await cancelSession()
      if (res?.interrupted) {
        appendLog('[stopped] command interrupted — the evolved surface is kept')
        // An empty command reports the energy and area as the surface stands.
        const now = await runCommand(sessionId, '')
        setStats(now.energy, now.area)
        if (now.geometry_changed !== false) bumpMeshVersion()
        return
      }
      const r = res?.restored
      if (r) {
        setSession(r.session_id, r.fe_file)
        setStats(r.energy, r.area)
        bumpMeshVersion()
        appendLog('[stopped] engine killed — restored the surface from before this command')
        return
      }
      clearSession()
      appendLog('[stopped] engine killed — reopen the file to continue')
    } catch (err: unknown) {
      appendLog(`[error] ${err instanceof Error ? err.message : String(err)}`)
    }
  }

  /** The Run menu's iterate items: `g N` through the same path as a typed command. */
  async function runG(n: number) {
    if (!sessionId || busy) return
    appendLog(`> g ${n}`)
    stoppedRef.current = false
    setBusy(true)
    try {
      const res = await runCommand(sessionId, `g ${n}`)
      if (res.output) appendLog(res.output)
      setStats(res.energy, res.area)
      if (res.geometry_changed !== false) bumpMeshVersion()
    } catch (err: unknown) {
      logCommandError(err)
    } finally {
      setBusy(false)
    }
  }

  useMenuAction(a => {
    if (a.startsWith('run:')) {
      const op = a.slice(4) as TopoOp
      const t  = TOPO_OPS.find(x => x.op === op)
      if (t) void handleTopo(op, t.label)
    } else if (a.startsWith('iterate:')) {
      void runG(Number(a.slice(8)))
    }
  })

  async function handleTopo(op: TopoOp, label: string) {
    if (!sessionId || busy) return
    appendLog(`> ${label}`)
    stoppedRef.current = false
    setBusy(true)
    try {
      const res = await runTopo(sessionId, op)
      appendLog(`  ${summarize(res.counts)} · ΔE ${res.energy_delta >= 0 ? '+' : ''}${res.energy_delta.toExponential(2)}`)
      setStats(res.energy, res.area)
      if (res.geometry_changed !== false) bumpMeshVersion()
    } catch (err: unknown) {
      logCommandError(err)
    } finally {
      setBusy(false)
    }
  }

  async function handleRun() {
    if (!sessionId || !input.trim() || busy) return
    const cmd = input.trim()
    setInput('')
    appendLog(`> ${cmd}`)
    stoppedRef.current = false
    setBusy(true)
    try {
      const res = await runCommand(sessionId, cmd)
      if (res.output) appendLog(res.output)
      if (res.energy !== null || res.area !== null) {
        setStats(res.energy, res.area)
        // Refetch only when the drawn geometry changed, not after `print` and the like.
        if (res.geometry_changed !== false) bumpMeshVersion()
      }
    } catch (err: unknown) {
      logCommandError(err)
    } finally {
      setBusy(false)
      inputRef.current?.focus()
    }
  }

  return (
    <div className="flex flex-col h-full bg-base-100">
      <OutputLog lines={outputLog} />

      {/* Input row */}
      <div className="shrink-0 flex items-center gap-2 px-3 py-2 border-t border-base-300 bg-base-200">
        <span className="text-success text-sm font-mono select-none shrink-0">$</span>
        <input
          ref={inputRef}
          className="flex-1 bg-transparent border-none outline-hidden text-sm font-mono text-base-content caret-primary placeholder:text-base-content/30 disabled:cursor-not-allowed"
          value={input}
          onChange={e => setInput(e.target.value)}
          onKeyDown={e => {
            if (e.key === 'Enter') handleRun()
            else if (e.key === 'Escape' && busy) handleStop()
          }}
          placeholder={sessionId ? 'SE command…' : 'Load a file first'}
          aria-label="Surface Evolver command"
          // readOnly, not disabled, while busy: a disabled input loses focus to the page.
          disabled={!sessionId}
          readOnly={busy}
          aria-busy={busy}
          spellCheck={false}
          autoComplete="off"
        />
        <button
          className={`btn btn-xs shrink-0 ${busy ? 'btn-error' : 'btn-primary'}`}
          onClick={busy ? handleStop : handleRun}
          disabled={!sessionId || (!busy && !input.trim())}
          title={busy ? 'Stop the running command (Esc)' : 'Run command'}
        >
          {busy ? 'Stop' : 'Run'}
        </button>
      </div>
    </div>
  )
}
