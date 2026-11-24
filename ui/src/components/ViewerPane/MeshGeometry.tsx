import { useEffect, useMemo } from 'react'
import * as THREE from 'three'
import { useThree, type ThreeEvent } from '@react-three/fiber'
import type { MeshData } from '../../api/simulation'
import type { ThemeColors } from '../../hooks/useThemeColors'

export type RenderMode = 'solid' | 'wireframe' | 'xray'

interface Props {
  mesh:          MeshData
  mode:          RenderMode
  elementColors: boolean        // render per-facet SE colour-table colours
  themeColors:   ThemeColors
}

/** SE's default facet colour. Treated as uncoloured: it takes the theme surface colour. */
export const SE_COLOR_WHITE = 15

/** SE's 16-colour palette, index → [r, g, b] in 0..1. */
const SE_PALETTE: [number, number, number][] = [
  [0,0,0],[0,0,1],[0,1,0],[0,1,1],[1,0,0],[1,0,1],[0.55,0.27,0.07],[0.75,0.75,0.75],
  [0.5,0.5,0.5],[0.5,0.5,1],[0.5,1,0.5],[0.5,1,1],[1,0.5,0.5],[1,0.5,1],[1,1,0],[1,1,1],
]
function paletteRGB(idx: number, neutral: [number, number, number]): [number, number, number] {
  if (idx < 0 || idx === SE_COLOR_WHITE) return neutral
  return idx < SE_PALETTE.length ? SE_PALETTE[idx] : neutral
}

/**
 * Non-indexed geometry with each facet's SE colour on all three of its corners,
 * so shading stays flat per facet. Uncoloured facets take `neutral`.
 */
function buildElementColorGeometry(mesh: MeshData, neutral: THREE.Color): THREE.BufferGeometry {
  const geo = new THREE.BufferGeometry()
  const nf  = mesh.facets.length
  const nrgb: [number, number, number] = [neutral.r, neutral.g, neutral.b]
  const pos = new Float32Array(nf * 9)
  const col = new Float32Array(nf * 9)
  for (let f = 0; f < nf; f++) {
    const tri = mesh.facets[f]
    const [r, g, b] = paletteRGB(mesh.facet_colors?.[f] ?? -1, nrgb)
    for (let k = 0; k < 3; k++) {
      const v = mesh.vertices[tri[k]]
      pos[f * 9 + k * 3] = v[0]; pos[f * 9 + k * 3 + 1] = v[1]; pos[f * 9 + k * 3 + 2] = v[2]
      col[f * 9 + k * 3] = r;    col[f * 9 + k * 3 + 1] = g;    col[f * 9 + k * 3 + 2] = b
    }
  }
  geo.setAttribute('position', new THREE.BufferAttribute(pos, 3))
  geo.setAttribute('color', new THREE.BufferAttribute(col, 3))
  geo.computeVertexNormals()
  return geo
}

function buildGeometry(mesh: MeshData): THREE.BufferGeometry {
  const geo = new THREE.BufferGeometry()
  geo.setAttribute('position', new THREE.BufferAttribute(new Float32Array(mesh.vertices.flat()), 3))
  geo.setIndex(new THREE.BufferAttribute(new Uint32Array(mesh.facets.flat()), 1))
  geo.computeVertexNormals()
  return geo
}

/** The edges on their own, for a mesh with no facets. */
export function EdgeLines({ mesh, themeColors }: { mesh: MeshData; themeColors: ThemeColors }) {
  const geometry = useMemo(() => {
    const geo = new THREE.BufferGeometry()
    geo.setAttribute('position', new THREE.BufferAttribute(new Float32Array(mesh.vertices.flat()), 3))
    geo.setIndex(new THREE.BufferAttribute(new Uint32Array(mesh.edges.flat()), 1))
    return geo
  }, [mesh])

  useEffect(() => () => { geometry.dispose() }, [geometry])

  return (
    <lineSegments geometry={geometry}>
      <lineBasicMaterial color={themeColors.line} />
    </lineSegments>
  )
}

/**
 * Every engine edge drawn over the facets, including those between coplanar
 * facets, which a feature-edge helper would hide.
 */
function EdgeOverlay({ mesh, color }: { mesh: MeshData; color: THREE.Color | string }) {
  const geometry = useMemo(() => {
    const geo = new THREE.BufferGeometry()
    geo.setAttribute('position', new THREE.BufferAttribute(new Float32Array(mesh.vertices.flat()), 3))
    geo.setIndex(new THREE.BufferAttribute(new Uint32Array(mesh.edges.flat()), 1))
    return geo
  }, [mesh])
  useEffect(() => () => { geometry.dispose() }, [geometry])
  return (
    <lineSegments geometry={geometry}>
      <lineBasicMaterial color={color} />
    </lineSegments>
  )
}

/**
 * Scales the raycaster's point-hit threshold with the mesh; the default of one
 * world unit makes every point a hit on a small mesh.
 */
export function RaycasterConfig({ threshold }: { threshold: number }) {
  const raycaster = useThree(s => s.raycaster)
  useEffect(() => {
    raycaster.params.Points = { threshold }
  }, [raycaster, threshold])
  return null
}

/** An invisible, clickable point per vertex; `e.index` is its buffer position (vpos). */
export function PickPoints({ mesh, onPick }: { mesh: MeshData; onPick: (vpos: number) => void }) {
  const geometry = useMemo(() => {
    const geo = new THREE.BufferGeometry()
    geo.setAttribute('position', new THREE.BufferAttribute(new Float32Array(mesh.vertices.flat()), 3))
    return geo
  }, [mesh])
  useEffect(() => () => { geometry.dispose() }, [geometry])

  return (
    <points
      geometry={geometry}
      onClick={(e: ThreeEvent<MouseEvent>) => {
        if (e.index == null) return
        e.stopPropagation()
        onPick(e.index)
      }}
    >
      <pointsMaterial size={0.01} transparent opacity={0.0} depthWrite={false} />
    </points>
  )
}

/** A highlight sphere on the picked vertex. */
export function VertexMarker({ position, radius }: { position: number[]; radius: number }) {
  return (
    <mesh position={position as [number, number, number]}>
      <sphereGeometry args={[radius, 16, 16]} />
      <meshBasicMaterial color="#ffcc33" depthTest={false} transparent opacity={0.9} />
    </mesh>
  )
}

/** A marker on each body's centre of mass. */
export function BodyMarkers({ cms, radius }: { cms: (number[] | null)[]; radius: number }) {
  return (
    <>
      {cms.map((cm, i) => cm && (
        <mesh key={i} position={cm as [number, number, number]}>
          <sphereGeometry args={[radius, 12, 12]} />
          <meshBasicMaterial color="#33ddff" depthTest={false} transparent opacity={0.85} />
        </mesh>
      ))}
    </>
  )
}

export default function MeshGeometry({ mesh, mode, elementColors, themeColors }: Props) {
  const useElementColors = elementColors && (mesh.facet_colors?.length ?? 0) > 0
  const geometry = useMemo(
    () => useElementColors
      ? buildElementColorGeometry(mesh, themeColors.surface)
      : buildGeometry(mesh),
    [mesh, useElementColors, themeColors],
  )

  useEffect(() => () => { geometry.dispose() }, [geometry])

  const showEdges = mesh.facets.length <= 200_000 && mesh.edges.length > 0

  if (mode === 'wireframe') {
    return (
      <mesh geometry={geometry}>
        <meshBasicMaterial color={themeColors.wire} wireframe side={THREE.DoubleSide} />
      </mesh>
    )
  }

  if (mode === 'xray') {
    return (
      <mesh geometry={geometry}>
        <meshPhongMaterial
          key="xray"
          color={themeColors.surface}
          specular={themeColors.specular}
          shininess={40}
          side={THREE.DoubleSide}
          transparent opacity={0.22}
          depthWrite={false}
          polygonOffset polygonOffsetFactor={1} polygonOffsetUnits={1}
        />
        {showEdges && <EdgeOverlay mesh={mesh} color={themeColors.line} />}
      </mesh>
    )
  }

  // Solid: SE colours when present, else the theme surface. Keyed per mode and
  // colour state because X-ray and solid share a tree position, and three.js
  // applies a vertexColors change only on a shader recompile.
  return (
    <mesh geometry={geometry}>
      <meshPhongMaterial
        key={useElementColors ? 'solid-element' : 'solid-theme'}
        color={useElementColors ? '#ffffff' : themeColors.surface}
        specular={useElementColors ? '#555555' : themeColors.specular}
        shininess={useElementColors ? 25 : 40}
        vertexColors={useElementColors}
        side={THREE.DoubleSide}
        polygonOffset polygonOffsetFactor={1} polygonOffsetUnits={1}
      />
      {showEdges && <EdgeOverlay mesh={mesh} color={themeColors.line} />}
    </mesh>
  )
}
