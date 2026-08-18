import { ref, watch } from 'vue'

const STORAGE_KEY = 'mqttx_global_variables'

function loadVariables(): Record<string, string> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return {}
    return JSON.parse(raw) as Record<string, string>
  } catch {
    return {}
  }
}

function saveVariables(vars: Record<string, string>) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(vars))
  } catch {
    // silently ignore
  }
}

const variables = ref<Record<string, string>>(loadVariables())

watch(
  variables,
  (val) => {
    saveVariables(val)
  },
  { deep: true },
)

export function useGlobalVariables() {
  function setVariable(name: string, value: string) {
    if (value.trim() === '') {
      delete variables.value[name]
    } else {
      variables.value = { ...variables.value, [name]: value }
    }
  }

  function deleteVariable(name: string) {
    const newVars = { ...variables.value }
    delete newVars[name]
    variables.value = newVars
  }

  function getVariable(name: string): string {
    return variables.value[name] ?? ''
  }

  function clear() {
    variables.value = {}
  }

  function importFromBindings(bindings: Record<string, string>) {
    const newVars = { ...variables.value }
    for (const [key, value] of Object.entries(bindings)) {
      if (value.trim()) {
        newVars[key] = value
      }
    }
    variables.value = newVars
  }

  function getAllVariables(): Record<string, string> {
    return { ...variables.value }
  }

  return {
    variables,
    setVariable,
    deleteVariable,
    getVariable,
    clear,
    importFromBindings,
    getAllVariables,
  }
}