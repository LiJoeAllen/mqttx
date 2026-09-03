import { ref, watch } from 'vue'
import type { MqttConnection } from '../types/mqtt'
import { defaultConnection } from '../types/mqtt'

const STORAGE_KEY = 'mqttx_connections'

function loadConnections(): MqttConnection[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return []
    const parsed = JSON.parse(raw) as MqttConnection[]
    return parsed.map((c) => ({
      ...c,
      protocolVersion: c.protocolVersion ?? '5.0',
      group: c.group ?? '',
      status: 'disconnected' as const,
      lastError: '',
    }))
  } catch {
    return []
  }
}

function saveConnections(connections: MqttConnection[]) {
  try {
    const toSave = connections.map(({ status: _, lastError: __, ...rest }) => rest)
    localStorage.setItem(STORAGE_KEY, JSON.stringify(toSave))
  } catch {
    // localStorage full or unavailable
  }
}

const connections = ref<MqttConnection[]>(loadConnections())

watch(
  connections,
  (val) => {
    saveConnections(val)
  },
  { deep: true },
)

export function useConnections() {
  function add(): MqttConnection {
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

  // ─── 分组操作 ──────────────────────────────────────────────────────────────

  /** 获取所有分组名称（去重） */
  function getGroups(): string[] {
    const set = new Set<string>()
    for (const c of connections.value) {
      if (c.group) set.add(c.group)
    }
    return Array.from(set).sort()
  }

  /** 按分组获取连接列表 */
  function getByGroup(group: string): MqttConnection[] {
    return connections.value.filter((c) => c.group === group)
  }

  /** 重命名分组 */
  function renameGroup(oldName: string, newName: string) {
    for (const c of connections.value) {
      if (c.group === oldName) {
        c.group = newName
      }
    }
  }

  /** 删除分组（将组内连接移到未分组） */
  function deleteGroup(group: string) {
    for (const c of connections.value) {
      if (c.group === group) {
        c.group = ''
      }
    }
  }

  // ─── 导入 / 导出 ──────────────────────────────────────────────────────────

  /** 导出为 JSON 字符串（去除运行时字段） */
  function exportConnections(ids?: string[]): string {
    const list = ids
      ? connections.value.filter((c) => ids.includes(c.id))
      : connections.value
    const data = list.map(({ status: _, lastError: __, ...rest }) => rest)
    return JSON.stringify(data, null, 2)
  }

  /** 从 JSON 导入，按名称+地址+端口+ClientID 去重，返回导入/跳过数量 */
  function importConnections(json: string): { imported: number; skipped: number } {
    const parsed = JSON.parse(json) as Partial<MqttConnection>[]
    const seen = new Set(
      connections.value.map((c) => `${c.name}|${c.host}|${c.port}|${c.clientId}`),
    )
    let imported = 0
    let skipped = 0
    for (const item of parsed) {
      const conn: MqttConnection = {
        ...defaultConnection(),
        ...item,
        id: crypto.randomUUID(), // 始终生成新 ID 避免冲突
        status: 'disconnected',
        lastError: '',
      }
      const key = `${conn.name}|${conn.host}|${conn.port}|${conn.clientId}`
      if (seen.has(key)) {
        skipped++
        continue
      }
      seen.add(key)
      connections.value.push(conn)
      imported++
    }
    return { imported, skipped }
  }

  return {
    connections,
    add,
    update,
    remove,
    getById,
    updateStatus,
    getConnected,
    getGroups,
    getByGroup,
    renameGroup,
    deleteGroup,
    exportConnections,
    importConnections,
  }
}
