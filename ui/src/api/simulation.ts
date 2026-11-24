import { rpc } from './client'

export interface MeshData {
  vertices: number[][]
  facets: number[][]
  edges: number[][]
  body_cms?: (number[] | null)[]
  facet_colors?: number[]
  /** Periodic surfaces only, when non-zero: edges dropped for crossing a period boundary. */
  wrapped_edges_hidden?: number
  /**
   * Periodic surfaces only: the unwrapped corners appended after the engine's
   * vertices. Entry i is the engine position of vertex `vertices.length - length + i`.
   */
  unwrap_source?: number[]
}

export interface RunCommandResponse {
  output: string
  energy: number | null
  area: number | null
  /** Whether what the viewer draws differs from the mesh last fetched. */
  geometry_changed?: boolean
}

export async function runCommand(id: string, command: string): Promise<RunCommandResponse> {
  return rpc<RunCommandResponse>('runCommand', { sessionId: id, command })
}

export type TopoOp = 'refine' | 'equi' | 'vertex_avg' | 'pop'

export interface TopoResponse {
  output: string
  counts: Record<string, number>
  energy: number
  energy_delta: number
  area: number
  /** Whether what the viewer draws differs from the mesh last fetched. */
  geometry_changed?: boolean
}

/** One pass of a topology op; repeated passes are `u 5` in the CLI. */
export async function runTopo(id: string, op: TopoOp): Promise<TopoResponse> {
  return rpc<TopoResponse>('topo', { sessionId: id, op })
}

/** The active surface's mesh, with SE's per-element colours. */
export async function getMesh(id: string): Promise<MeshData> {
  return rpc<MeshData>('getMesh', { sessionId: id, colors: true })
}

/** `attr` bits (se_api.h): FIXED=0x40, BOUNDARY=0x80, CONSTRAINT=0x400. */
export interface VertexInfo {
  id: number
  xyz: number[]
  attr: number
  constraints: { idx: number; name: string }[]
}

export async function getVertexInfo(id: string, vpos: number): Promise<VertexInfo> {
  return rpc<VertexInfo>('vertexInfo', { sessionId: id, vpos })
}
