import { useEffect, useRef } from 'react'

/**
 * Calls `handler` with every native menu action (an `se-menu` event, see
 * src-tauri/src/menu.rs); each consumer acts on the prefixes it owns. The latest
 * handler is kept in a ref, so an inline one doesn't re-bind the listener.
 */
export function useMenuAction(handler: (action: string) => void): void {
  const ref = useRef(handler)
  ref.current = handler
  useEffect(() => {
    const onMenu = (e: Event) => {
      const action = (e as CustomEvent<string>).detail
      if (typeof action === 'string') ref.current(action)
    }
    window.addEventListener('se-menu', onMenu)
    return () => window.removeEventListener('se-menu', onMenu)
  }, [])
}
