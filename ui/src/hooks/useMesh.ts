import { useState, useEffect } from 'react'
import { getMesh, type MeshData } from '../api/simulation'
import { useStore } from '../store/useStore'

/** The active session's mesh, refetched whenever the surface changes (a `meshVersion` bump). */
export function useMesh() {
  const { sessionId, meshVersion, appendLog } = useStore()
  // Tagged with its session, so right after a file switch the previous file's
  // mesh isn't taken for the new one's (FitCamera would frame it).
  const [fetched, setFetched]       = useState<{ sid: string; mesh: MeshData } | null>(null)
  const setData = (mesh: MeshData | null) => setFetched(mesh && sessionId ? { sid: sessionId, mesh } : null)
  const data = fetched?.sid === sessionId ? fetched.mesh : null
  const [isFetching, setIsFetching] = useState(false)

  useEffect(() => {
    // No session: clear the loading state too, since an abandoned fetch never will.
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
