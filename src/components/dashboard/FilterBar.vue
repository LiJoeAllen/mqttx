<script setup lang="ts">
defineProps<{
  modelValue: string
}>()

const emit = defineEmits<{
  'update:modelValue': [value: string]
}>()
</script>

<template>
  <div class="filter-bar">
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="search-icon"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>
    <input
      :value="modelValue"
      @input="(e: any) => emit('update:modelValue', e.target.value)"
      placeholder="搜索主题..."
      class="filter-input"
    />
    <button
      v-if="modelValue"
      class="filter-clear"
      @click="emit('update:modelValue', '')"
    >✕</button>
  </div>
</template>

<style scoped>
.filter-bar {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 0 8px;
  border: 1px solid var(--el-border-color);
  border-radius: 6px;
  background: var(--el-bg-color);
  height: 28px;
  width: 180px;
  transition: border-color 0.15s;
}
.filter-bar:focus-within {
  border-color: var(--el-color-primary);
}
.search-icon {
  color: var(--el-text-color-placeholder);
  flex-shrink: 0;
}
.filter-input {
  flex: 1;
  border: none;
  outline: none;
  font-size: 12px;
  color: var(--el-text-color-regular);
  background: transparent;
  font-family: inherit;
}
.filter-input::placeholder {
  color: var(--el-text-color-placeholder);
}
.filter-clear {
  border: none;
  background: none;
  font-size: 12px;
  color: var(--el-text-color-secondary);
  cursor: pointer;
  padding: 0 2px;
  line-height: 1;
}
.filter-clear:hover {
  color: var(--el-text-color-primary);
}
</style>