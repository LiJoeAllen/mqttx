import { onMounted, onUnmounted } from 'vue'

const contextMenuHandlers = new Set<(e: MouseEvent) => void>()

/**
 * Register a custom right-click handler.
 * The handler should call `e.preventDefault()` if it handles the event.
 */
export function registerContextMenu(handler: (e: MouseEvent) => void) {
  contextMenuHandlers.add(handler)
}

export function unregisterContextMenu(handler: (e: MouseEvent) => void) {
  contextMenuHandlers.delete(handler)
}

/**
 * Global context menu controller.
 * In production: prevents default browser context menu everywhere.
 * In debug (dev mode): allows native context menu.
 * Custom handlers registered via registerContextMenu take priority.
 */
export function useContextMenuController() {
  function onContextMenu(e: MouseEvent) {
    for (const handler of contextMenuHandlers) {
      handler(e)
      if (e.defaultPrevented) return
    }
    if (import.meta.env.PROD) {
      e.preventDefault()
    }
  }

  onMounted(() => {
    document.addEventListener('contextmenu', onContextMenu)
  })

  onUnmounted(() => {
    document.removeEventListener('contextmenu', onContextMenu)
  })
}