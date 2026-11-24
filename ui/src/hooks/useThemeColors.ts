import { useState, useEffect } from 'react'
import * as THREE from 'three'

export interface ThemeColors {
  surface:  THREE.Color   // default surface fill (theme --mesh)
  specular: THREE.Color   // phong highlight
  line:     THREE.Color   // edge overlay, drawn on facets (theme --mesh-edge)
  wire:     THREE.Color   // wireframe, drawn on the background (base-content)
}

// A canvas turns any CSS colour into sRGB bytes: fillStyle parses every colour
// form, and a painted pixel reads back plain. Reading fillStyle back would not
// do: Chromium returns "oklch(…)", which THREE parses as white.
const ctx = typeof document !== 'undefined'
  ? document.createElement('canvas').getContext('2d')
  : null

/** A theme colour variable (e.g. `--mesh`) from :root, as a THREE.Color. */
function varColor(name: string, fallback: string): THREE.Color {
  if (!ctx) return new THREE.Color(fallback)
  const raw = getComputedStyle(document.documentElement).getPropertyValue(name).trim()
  ctx.fillStyle = fallback
  if (raw) ctx.fillStyle = raw
  ctx.fillRect(0, 0, 1, 1)
  const [r, g, b] = ctx.getImageData(0, 0, 1, 1).data
  return new THREE.Color().setRGB(r / 255, g / 255, b / 255, THREE.SRGBColorSpace)
}

function read(): ThemeColors {
  // --mesh and --mesh-edge come from styles/global.css: light facets, dark edges, as SE draws them.
  const surface = varColor('--mesh', '#d4d4d4')
  return {
    surface,
    specular: surface.clone().lerp(new THREE.Color('#ffffff'), 0.6),
    line:     varColor('--mesh-edge', '#0a0a0a'),
    // The foreground colour: a wireframe has no facets behind it, so the dark
    // edge colour would vanish on the dark theme.
    wire:     varColor('--color-base-content', '#737373'),
  }
}

/** Mesh material colours from the active theme, updated when the theme changes. */
export function useThemeColors(): ThemeColors {
  const [colors, setColors] = useState<ThemeColors>(read)
  useEffect(() => {
    const obs = new MutationObserver(() => setColors(read()))
    obs.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] })
    return () => obs.disconnect()
  }, [])
  return colors
}
