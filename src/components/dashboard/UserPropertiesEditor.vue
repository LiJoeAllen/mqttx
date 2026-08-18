<script setup lang="ts">
const props = defineProps<{
  modelValue: { key: string; value: string }[]
}>()

const emit = defineEmits<{
  'update:modelValue': [value: { key: string; value: string }[]]
}>()

function addRow() {
  emit('update:modelValue', [...props.modelValue, { key: '', value: '' }])
}

function removeRow(index: number) {
  const arr = [...props.modelValue]
  arr.splice(index, 1)
  emit('update:modelValue', arr)
}

function updateRow(index: number, field: 'key' | 'value', val: string) {
  const arr = [...props.modelValue]
  arr[index] = { ...arr[index], [field]: val }
  emit('update:modelValue', arr)
}
</script>

<template>
  <div class="user-props-editor">
    <div class="editor-header">
      <span class="editor-title">User Properties</span>
      <el-button size="small" type="primary" link @click="addRow">
        + 添加
      </el-button>
    </div>
    <div
      v-for="(prop, index) in modelValue"
      :key="index"
      class="prop-row"
    >
      <el-input
        :model-value="prop.key"
        size="small"
        placeholder="Key"
        style="flex: 1"
        @input="(val: string) => updateRow(index, 'key', val)"
      />
      <el-input
        :model-value="prop.value"
        size="small"
        placeholder="Value"
        style="flex: 1"
        @input="(val: string) => updateRow(index, 'value', val)"
      />
      <el-button
        size="small"
        type="danger"
        link
        @click="removeRow(index)"
      >
        删除
      </el-button>
    </div>
  </div>
</template>

<style scoped>
.user-props-editor {
  margin-top: 8px;
}
.editor-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 4px;
}
.editor-title {
  font-size: 12px;
  color: var(--el-text-color-secondary);
}
.prop-row {
  display: flex;
  gap: 4px;
  margin-bottom: 4px;
  align-items: center;
}
</style>