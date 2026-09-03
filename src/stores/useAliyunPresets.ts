import { ref, watch } from 'vue'
import type { AliyunPreset } from '../types/mqtt'
import { defaultAliyunPreset } from '../types/mqtt'

const STORAGE_KEY = 'mqttx_aliyun_presets'

function loadPresets(): AliyunPreset[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return []
    const parsed = JSON.parse(raw) as AliyunPreset[]
    return parsed.map((p) => ({
      ...defaultAliyunPreset(),
      ...p,
      id: p.id || crypto.randomUUID(),
      group: p.group ?? '',
    }))
  } catch {
    return []
  }
}

function savePresets(presets: AliyunPreset[]) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(presets))
  } catch {}
}

const presets = ref<AliyunPreset[]>(loadPresets())

watch(presets, (val) => savePresets(val), { deep: true })

export function useAliyunPresets() {
  function add(preset?: Partial<AliyunPreset>): AliyunPreset {
    const p: AliyunPreset = { ...defaultAliyunPreset(), ...preset }
    presets.value.push(p)
    return p
  }

  function update(id: string, data: Partial<AliyunPreset>) {
    const idx = presets.value.findIndex((p) => p.id === id)
    if (idx !== -1) {
      presets.value[idx] = { ...presets.value[idx], ...data }
    }
  }

  function remove(id: string) {
    const idx = presets.value.findIndex((p) => p.id === id)
    if (idx !== -1) presets.value.splice(idx, 1)
  }

  function getById(id: string): AliyunPreset | undefined {
    return presets.value.find((p) => p.id === id)
  }

  // ─── 分组 ────────────────────────────────────────────────────────────────

  function getGroups(): string[] {
    const set = new Set<string>()
    for (const p of presets.value) {
      if (p.group) set.add(p.group)
    }
    return Array.from(set).sort()
  }

  function renameGroup(oldName: string, newName: string) {
    for (const p of presets.value) {
      if (p.group === oldName) p.group = newName
    }
  }

  function deleteGroup(group: string) {
    for (const p of presets.value) {
      if (p.group === group) p.group = ''
    }
  }

  // ─── 导入 / 导出 ────────────────────────────────────────────────────────

  function exportPresets(ids?: string[]): string {
    const list = ids
      ? presets.value.filter((p) => ids.includes(p.id))
      : presets.value
    return JSON.stringify(list, null, 2)
  }

  /** 按 实例ID+GroupID+设备ID+AK 去重，返回导入/跳过数量 */
  function importPresets(json: string): { imported: number; skipped: number } {
    const parsed = JSON.parse(json) as Partial<AliyunPreset>[]
    const seen = new Set(
      presets.value.map((p) => `${p.instanceId}|${p.groupId}|${p.deviceId}|${p.accessKeyId}`),
    )
    let imported = 0
    let skipped = 0
    for (const item of parsed) {
      const p: AliyunPreset = {
        ...defaultAliyunPreset(),
        ...item,
        id: crypto.randomUUID(),
      }
      const key = `${p.instanceId}|${p.groupId}|${p.deviceId}|${p.accessKeyId}`
      if (seen.has(key)) {
        skipped++
        continue
      }
      seen.add(key)
      presets.value.push(p)
      imported++
    }
    return { imported, skipped }
  }

  return {
    presets,
    add,
    update,
    remove,
    getById,
    getGroups,
    renameGroup,
    deleteGroup,
    exportPresets,
    importPresets,
  }
}
