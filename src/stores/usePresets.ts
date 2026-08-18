import { ref, watch } from 'vue'
import type { Preset } from '../types/mqtt'
import { defaultPreset } from '../types/mqtt'

const STORAGE_KEY = 'mqttx_presets'

function loadPresets(): Preset[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return []
    return JSON.parse(raw) as Preset[]
  } catch {
    return []
  }
}

function savePresets(presets: Preset[]) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(presets))
  } catch {
    // silently ignore
  }
}

const presets = ref<Preset[]>(loadPresets())

watch(
  presets,
  (val) => {
    savePresets(val)
  },
  { deep: true },
)

export function usePresets() {
  function add() {
    const preset = defaultPreset()
    presets.value.push(preset)
    return preset
  }

  function update(id: string, data: Partial<Preset>) {
    const idx = presets.value.findIndex((p) => p.id === id)
    if (idx !== -1) {
      presets.value[idx] = { ...presets.value[idx], ...data }
    }
  }

  function remove(id: string) {
    const idx = presets.value.findIndex((p) => p.id === id)
    if (idx !== -1) {
      presets.value.splice(idx, 1)
    }
  }

  function getById(id: string): Preset | undefined {
    return presets.value.find((p) => p.id === id)
  }

  return {
    presets,
    add,
    update,
    remove,
    getById,
  }
}