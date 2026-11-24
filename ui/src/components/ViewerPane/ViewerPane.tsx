import { useRef, useEffect, useState, useMemo, type MutableRefObject } from 'react'
import * as THREE from 'three'
import { Canvas, useFrame, useThree } from '@react-three/fiber'
import { OrbitControls as OrbitControlsImpl } from 'three/examples/jsm/controls/OrbitControls.js'
import { useMesh } from '../../hooks/useMesh'
import { useMenuAction } from '../../hooks/useMenuAction'
import { useThemeColors } from '../../hooks/useThemeColors'
import { useStore } from '../../store/useStore'
import { getVertexInfo, type MeshData, type VertexInfo } from '../../api/simulation'
import MeshGeometry, {
  EdgeLines, PickPoints, VertexMarker, BodyMarkers, RaycasterConfig, type RenderMode,
} from './MeshGeometry'
import VertexInspector from './VertexInspector'

const MODES: RenderMode[] = ['solid', 'wireframe', 'xray']
const MODE_LABEL: Record<RenderMode, string> = { solid: 'Solid', wireframe: 'Wire', xray: 'X-Ray' }

// Heroicons 24/outline (MIT), inlined: two icons didn't justify a dependency.
function Icon({ d }: { d: string }) {
  return (
    <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" aria-hidden="true">
      <path strokeLinecap="round" strokeLinejoin="round" d={d} />
    </svg>
  )
}
const ARROW_PATH = 'M16.023 9.348h4.992v-.001M2.985 19.644v-4.992m0 0h4.992m-4.993 0 3.181 3.183a8.25 8.25 0 0 0 13.803-3.7M4.031 9.865a8.25 8.25 0 0 1 13.803-3.7l3.181 3.182m0-4.991v4.99'
const CURSOR_RAYS = 'M15.042 21.672 13.684 16.6m0 0-2.51 2.225.569-9.47 5.227 7.917-3.286-.672ZM12 2.25V4.5m5.834.166-1.591 1.591M20.25 10.5H18M7.757 14.743l-1.59 1.59M6 10.5H3.75m4.007-4.243-1.59-1.59'

// three's own OrbitControls, wired the way drei's wrapper did (damped, updated
// every frame, registered as R3F's default `controls` for FitCamera) without
// drei's ~20 transitive dependencies.
function Controls({ controlsRef }: { controlsRef: MutableRefObject<OrbitControlsImpl | null> }) {
  const camera = useThree(s => s.camera)
  const dom    = useThree(s => s.gl.domElement)
  const set    = useThree(s => s.set)
  const controls = useMemo(() => new OrbitControlsImpl(camera, dom), [camera, dom])
  useFrame(() => controls.update(), -1)
  useEffect(() => {
    controls.enableDamping = true
    controlsRef.current = controls
    set({ controls })
    return () => { set({ controls: null }); controlsRef.current = null; controls.dispose() }
  }, [controls, controlsRef, set])
  return null
}

// Frames the camera on the first mesh of each session (a new file, a reload,
// a restore), not on every refetch: a `g 1` between orbits used to throw away
// the user's view (#21). Runs *inside* the Canvas and reads the controls from
// R3F state (not a ref) so it re-fires once OrbitControls has mounted —
// otherwise the first load runs before controls exist, the orbit target stays
// at the origin, and you end up pivoting around a corner vertex.
function FitCamera({ mesh, fitKey }: { mesh: MeshData | null; fitKey: string | null }) {
  const camera = useThree(s => s.camera)
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const controls = useThree(s => s.controls) as any
  const fittedFor = useRef<string | null>(null)
  useEffect(() => {
    if (!controls || !mesh || mesh.vertices.length === 0) return
    if (fittedFor.current === fitKey) return
    fittedFor.current = fitKey
    const geo = new THREE.BufferGeometry()
    geo.setAttribute('position', new THREE.BufferAttribute(new Float32Array(mesh.vertices.flat()), 3))
    geo.computeBoundingBox(); geo.computeBoundingSphere()
    const center = new THREE.Vector3()
    geo.boundingBox!.getCenter(center)
    const radius   = geo.boundingSphere!.radius || 1
    const distance = radius * 3.5
    camera.position.set(center.x + distance * 0.7, center.y + distance * 0.6, center.z + distance * 0.7)
    controls.target.copy(center)
    controls.update()
    controls.saveState()
    geo.dispose()
  }, [mesh, camera, controls, fitKey])
  return null
}

export default function ViewerPane() {
  const { sessionId } = useStore()

  const controlsRef = useRef<OrbitControlsImpl | null>(null)
  const [mode,      setMode]      = useState<RenderMode>('solid')
  const [inspect,    setInspect]    = useState(false)
  const [picked,     setPicked]     = useState<VertexInfo | null>(null)
  const [pickedPos,  setPickedPos]  = useState<number[] | null>(null)

  const { data: mesh, isFetching } = useMesh()
  const themeColors = useThemeColors()

  // Characteristic mesh size → raycaster threshold + marker radii (scale-aware).
  const meshRadius = useMemo(() => {
    if (!mesh || mesh.vertices.length === 0) return 1
    const lo = [Infinity, Infinity, Infinity], hi = [-Infinity, -Infinity, -Infinity]
    for (const v of mesh.vertices) for (let k = 0; k < 3; k++) {
      if (v[k] < lo[k]) lo[k] = v[k]; if (v[k] > hi[k]) hi[k] = v[k]
    }
    return Math.max(hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]) || 1
  }, [mesh])

  async function handlePick(vpos: number) {
    if (!sessionId || !mesh) return
    setPickedPos(mesh.vertices[vpos] ?? null)
    // An unwrapped periodic duplicate has no engine position of its own (#17).
    const src = mesh.unwrap_source ?? []
    const base = mesh.vertices.length - src.length
    const enginePos = vpos >= base ? src[vpos - base] : vpos
    try { setPicked(await getVertexInfo(sessionId, enginePos)) }
    catch { setPicked(null) }
  }

  // Native View menu → render mode, panel toggles.
  useMenuAction(a => {
    if (!sessionId) return
    if (a.startsWith('render:'))      setMode(a.slice(7) as RenderMode)
    else if (a === 'panel:inspect')   { setInspect(i => !i); setPicked(null); setPickedPos(null) }
  })

  useEffect(() => {
    if (!sessionId) { setInspect(false) }
  }, [sessionId])

  // A picked vertex's position index goes stale when the mesh changes (refine,
  // iterate, etc.) — clear the selection so we never highlight the wrong vertex.
  useEffect(() => { setPicked(null); setPickedPos(null) }, [mesh])

  return (
    <div className="relative h-full bg-base-100 min-w-0">

      {/* Empty state */}
      {!sessionId && (
        <div className="absolute inset-0 flex items-center justify-center pointer-events-none z-10">
          <div className="badge badge-ghost badge-lg text-base-content/40">Select a .fe file to begin</div>
        </div>
      )}
      {sessionId && mesh && mesh.facets.length === 0 && mesh.edges.length === 0 && (
        <div className="absolute inset-0 flex items-center justify-center pointer-events-none z-10">
          <div className="badge badge-ghost badge-lg text-base-content/40">No geometry in this model</div>
        </div>
      )}

      {/* Fetching indicator */}
      {isFetching && (
        <div className="absolute top-2.5 right-3 z-20 badge badge-neutral gap-1 pointer-events-none">
          <span className="loading loading-xs loading-dots" />
          Loading
        </div>
      )}

      {/* Toolbar (top-left) */}
      <div className="absolute top-2 left-2 z-20 flex items-center gap-1">
        <button
          className="btn btn-xs bg-base-300/80 border-base-300 hover:bg-base-300 text-base-content"
          onClick={() => controlsRef.current?.reset()} title="Reset camera"
        ><Icon d={ARROW_PATH} /></button>
        <button
          className="btn btn-xs bg-base-300/80 border-base-300 hover:bg-base-300 text-base-content"
          onClick={() => setMode(m => MODES[(MODES.indexOf(m) + 1) % MODES.length])}
          title="Cycle render mode"
        >{MODE_LABEL[mode]}</button>
        <button
          className={`btn btn-xs border-base-300 text-base-content ${inspect ? 'bg-base-300' : 'bg-base-300/80 hover:bg-base-300'}`}
          onClick={() => { setInspect(i => !i); setPicked(null); setPickedPos(null) }}
          disabled={!sessionId}
          title="Inspect: click a vertex; show body centroids"
        ><Icon d={CURSOR_RAYS} /></button>
      </div>

      {inspect && picked && (
        <VertexInspector info={picked} onClose={() => { setPicked(null); setPickedPos(null) }} />
      )}
      {inspect && !picked && mesh && (
        <div className="absolute bottom-3 right-3 z-30 badge bg-base-300/80 border-base-300 text-base-content text-[10px] pointer-events-none">
          Click a vertex to inspect
        </div>
      )}

      {/* Mesh stats (bottom-left — bottom-right is the Inspect panel's spot) */}
      {mesh && (
        <div className="absolute bottom-3 left-3 z-20 badge bg-base-300/80 border-base-300 text-base-content font-mono text-[10px] pointer-events-none">
          {(mesh.vertices.length - (mesh.unwrap_source?.length ?? 0)).toLocaleString()} v · {mesh.edges.length.toLocaleString()} e · {mesh.facets.length.toLocaleString()} f
          {mesh.wrapped_edges_hidden ? (
            <span
              className="ml-1 text-warning"
              title={`Periodic surface: ${mesh.wrapped_edges_hidden} edges wrap around the domain and are not drawn. Run \`detorus\` to unwrap the geometry — note that is irreversible.`}
            >· {mesh.wrapped_edges_hidden} wrapped</span>
          ) : null}
        </div>
      )}

      <Canvas style={{ height: '100%' }} camera={{ position: [2.5, 2, 2.5], fov: 40 }} gl={{ antialias: true, alpha: true }}>
        <ambientLight intensity={0.35} />
        <directionalLight position={[4, 8, 4]} intensity={0.9} />
        <directionalLight position={[-4, -2, -4]} intensity={0.2} color="#4488cc" />
        {mesh && mesh.facets.length > 0 && (
          <MeshGeometry mesh={mesh} mode={mode} elementColors themeColors={themeColors} />
        )}
        {mesh && mesh.facets.length === 0 && mesh.edges.length > 0 && (
          <EdgeLines mesh={mesh} themeColors={themeColors} />
        )}
        {inspect && mesh && (
          <>
            <RaycasterConfig threshold={meshRadius * 0.02} />
            <PickPoints mesh={mesh} onPick={handlePick} />
            {mesh.body_cms && mesh.body_cms.some(Boolean) &&
              <BodyMarkers cms={mesh.body_cms} radius={meshRadius * 0.02} />}
            {pickedPos && <VertexMarker position={pickedPos} radius={meshRadius * 0.025} />}
          </>
        )}
        <Controls controlsRef={controlsRef} />
        <FitCamera mesh={mesh} fitKey={sessionId} />
      </Canvas>
    </div>
  )
}
