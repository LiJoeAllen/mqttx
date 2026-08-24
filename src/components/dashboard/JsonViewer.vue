<script setup lang="ts">
import { JsonEditor } from '@visual-json/vue'
import type { JsonValue } from '@visual-json/core'

defineProps<{
  value: JsonValue
  height?: string | number
}>()

/**
 * visual-json 的 readOnly 只阻止 change 事件，并不会真的禁用编辑 UI
 * （双击值、Delete 键、右键菜单、拖拽重排都还能改树）。
 * 这里在捕获阶段拦截这些编辑入口，保留选中/复制/搜索/滚动等查看能力。
 */
const EDIT_KEYS = ['Delete', 'Backspace', 'Enter', 'F2', 'Insert']

function guardMutation(e: Event) {
  const target = e.target as HTMLElement | null
  const isFormField = !!target && (
    target.tagName === 'INPUT' ||
    target.tagName === 'TEXTAREA' ||
    target.isContentEditable
  )

  if (e instanceof KeyboardEvent) {
    // 搜索框里的输入/回车放行；其余位置的编辑键全部拦截
    if (isFormField) return
    if (EDIT_KEYS.includes(e.key)) {
      e.preventDefault()
      e.stopPropagation()
    }
    return
  }

  // dblclick / contextmenu / dragstart
  if (!isFormField) {
    e.preventDefault()
    e.stopPropagation()
  }
}
</script>

<template>
  <div
    class="json-viewer"
    @dblclick.capture="guardMutation"
    @contextmenu.capture="guardMutation"
    @dragstart.capture="guardMutation"
    @keydown.capture="guardMutation"
  >
    <JsonEditor :value="value" read-only :tree-show-values="true" :height="height" />
  </div>
</template>

<style scoped>
.json-viewer {
  border: 1px solid var(--el-border-color-extra-light);
  border-radius: 6px;
  overflow: hidden;
}
</style>
