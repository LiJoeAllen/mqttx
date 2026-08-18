import { ref, watch } from 'vue'
import type { MqttConnection } from '../types/mqtt'
import { defaultConnection } from '../types/mqtt'

const STORAGE_KEY = 'mqttx_connections'

function loadConnections(): MqttConnection[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return []
    const parsed = JSON.parse(raw) as MqttConnection[]
    // Ensure each connection has runtime status fields
    return parsed.map((c) => ({
      ...c,
      status: 'disconnected' as const,
      lastError: '',
    }))
  } catch {
    return []
  }
}

function saveConnections(connections: MqttConnection[]) {
  try {
    // Strip runtime fields before persisting
    const toSave = connections.map(({ status: _, lastError: __, ...rest }) => rest)
    localStorage.setItem(STORAGE_KEY, JSON.stringify(toSave))
  } catch {
    // localStorage full or unavailable — silently ignore
  }
}

const connections = ref<MqttConnection[]>(loadConnections())

// Auto-persist on changes
watch(
  connections,
  (val) => {
    saveConnections(val)
  },
  { deep: true },
)

export function useConnections() {
  function add() {
    const conn = defaultConnection()
    connections.value.push(conn)
    return conn
  }

  function update(id: string, data: Partial<MqttConnection>) {
    const idx = connections.value.findIndex((c) => c.id === id)
    if (idx !== -1) {
      connections.value[idx] = { ...connections.value[idx], ...data }
    }
  }

  function remove(id: string) {
    const idx = connections.value.findIndex((c) => c.id === id)
    if (idx !== -1) {
      connections.value.splice(idx, 1)
    }
  }

  function getById(id: string): MqttConnection | undefined {
    return connections.value.find((c) => c.id === id)
  }

  function updateStatus(
    id: string,
    status: MqttConnection['status'],
    lastError = '',
  ) {
    const conn = getById(id)
    if (conn) {
      conn.status = status
      conn.lastError = lastError
    }
  }

  function getConnected(): MqttConnection[] {
    return connections.value.filter((c) => c.status === 'connected')
  }

  return {
    connections,
    add,
    update,
    remove,
    getById,
    updateStatus,
    getConnected,
  }
}