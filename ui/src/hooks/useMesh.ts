import { useState, useEffect } from 'react'
import { getMesh, type MeshData } from '../api/simulation'
import { useStore } from '../store/useStore'

// Fetches the mesh (with native SE per-element colors) for the active session,
// refetching whenever the surface changes (meshVersion bump).
export function useMesh() {
  const { sessionId, meshVersion, appendLog } = useStore()
  // Tagged with the session it came from: right after a switch, the previous
  // file's mesh must not count as this one's. FitCamera framed it, marked the
  // new session fitted, and opened phelanc.fe from inside the foam.
  const [fetched, setFetched]       = useState<{ sid: string; mesh: MeshData } | null>(null)
  const setData = (mesh: MeshData | null) => setFetched(mesh && sessionId ? { sid: sessionId, mesh } : null)
  const data = fetched?.sid === sessionId ? fetched.mesh : null
  const [isFetching, setIsFetching] = useState(false)

  useEffect(() => {
    // No session (Stop, crash, closed tab): clear the loading state too. A
    // fetch abandoned by the cleanup below never gets to, so the badge stuck (#34).
    if (!sessionId) { setData(null); setIsFetching(false); return }
    let cancelled = false
    setIsFetching(true)
    getMesh(sessionId)
      .then(mesh  => { if (!cancelled) setData(mesh) })
      .catch((err: unknown) => {
        if (cancelled) return
        setData(null)
        appendLog(`[error] mesh fetch failed: ${err instanceof Error ? err.message : String(err)}`)
      })
      .finally(() => { if (!cancelled) setIsFetching(false) })
    return () => { cancelled = true }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId, meshVersion])

  return { data, isFetching }
}
