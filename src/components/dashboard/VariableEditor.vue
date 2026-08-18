<script setup lang="ts">
import { computed } from 'vue'
import { extractVariables } from '../../utils/envVarResolver'

const props = defineProps<{
  payloadTemplate: string
  modelValue: Record<string, string>
}>()

const emit = defineEmits<{
  'update:modelValue': [value: Record<string, string>]
}>()

const variables = computed(() => {
  return extractVariables(props.payloadTemplate)
})

function updateValue(name: string, value: string) {
  emit('update:modelValue', { ...props.modelValue, [name]: value })
}
function formatVariableName(name: string): string {
  return '\u007B\u007B' + name + '\u007D\u007D'
}
</script>

<template>
  <div v-if="variables.length > 0" class="variable-editor">
    <div class="variable-editor-title">变量</div>
    <div class="variable-grid">
      <div
        v-for="v in variables"
        :key="v"
        class="variable-item"
      >
        <span class="variable-label">{{ formatVariableName(v) }}</span>
        <el-input
          :model-value="modelValue[v] ?? ''"
          size="small"
          placeholder="输入值..."
          @input="(val: string) => updateValue(v, val)"
        />
      </div>
    </div>
  </div>
</template>

<style scoped>
.variable-editor {
  margin-top: 8px;
  padding: 8px;
  background: var(--el-fill-color-light);
  border-radius: 4px;
}
.variable-editor-title {
  font-size: 12px;
  color: var(--el-text-color-secondary);
  margin-bottom: 6px;
}
.variable-grid {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.variable-item {
  display: flex;
  align-items: center;
  gap: 8px;
}
.variable-label {
  min-width: 80px;
  font-family: monospace;
  font-size: 13px;
  color: var(--el-color-primary);
}
</style>