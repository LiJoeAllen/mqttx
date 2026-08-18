import { ref } from 'vue'

export interface LogEntry {
  id: string
  timestamp: number
  connectionId: string
  level: 'info' | 'warn' | 'error'
  event: string
  message: string
  details?: string
}

const MAX_LOGS = 1000

const logs = ref<LogEntry[]>([])

export function useLogs() {
  function push(entry: LogEntry) {
    logs.value.push(entry)
    if (logs.value.length > MAX_LOGS) {
      logs.value.splice(0, logs.value.length - MAX_LOGS)
    }
  }

  function clear() {
    logs.value = []
  }

  function filterByConnection(connectionId: string): LogEntry[] {
    if (!connectionId) return logs.value
    return logs.value.filter((l) => l.connectionId === connectionId)
  }

  function filterByLevel(level: string): LogEntry[] {
    if (!level) return logs.value
    return logs.value.filter((l) => l.level === level)
  }

  return {
    logs,
    push,
    clear,
    filterByConnection,
    filterByLevel,
  }
}