<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'

export interface ContextMenuItem {
  label: string
  icon?: string
  danger?: boolean
  disabled?: boolean
  divider?: boolean
  action: () => void
}

const props = defineProps<{
  items: ContextMenuItem[]
}>()

const visible = ref(false)
const x = ref(0)
const y = ref(0)
let targetEl: HTMLElement | null = null

function show(ev: MouseEvent, el: HTMLElement) {
  x.value = ev.clientX
  y.value = ev.clientY
  targetEl = el
  visible.value = true
}

function hide() {
  visible.value = false
  targetEl = null
}

function handleAction(item: ContextMenuItem) {
  if (item.disabled) return
  hide()
  item.action()
}

function onClickOutside(e: MouseEvent) {
  const menu = document.querySelector('.context-menu')
  if (menu && !menu.contains(e.target as Node)) {
    hide()
  }
}

onMounted(() => {
  document.addEventListener('click', onClickOutside)
})

onUnmounted(() => {
  document.removeEventListener('click', onClickOutside)
})

defineExpose({ show, hide })
</script>

<template>
  <Teleport to="body">
    <div
      v-if="visible"
      class="context-menu"
      :style="{ left: x + 'px', top: y + 'px' }"
    >
      <div
        v-for="(item, idx) in items"
        :key="idx"
        :class="['ctx-item', { danger: item.danger, disabled: item.disabled, divider: item.divider }]"
        @click="handleAction(item)"
      >
        <span v-if="item.icon" class="ctx-icon">{{ item.icon }}</span>
        <span class="ctx-label">{{ item.label }}</span>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.context-menu {
  position: fixed;
  z-index: 9999;
  min-width: 140px;
  background: var(--el-bg-color);
  border: 1px solid var(--el-border-color);
  border-radius: 6px;
  padding: 4px;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
  font-size: 12px;
}
.ctx-item {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 10px;
  border-radius: 4px;
  cursor: pointer;
  color: var(--el-text-color-regular);
  transition: background 0.1s;
  user-select: none;
}
.ctx-item:hover {
  background: var(--el-fill-color-light);
}
.ctx-item.danger {
  color: var(--el-color-danger);
}
.ctx-item.danger:hover {
  background: var(--el-color-danger-light-9);
}
.ctx-item.disabled {
  opacity: 0.4;
  cursor: not-allowed;
}
.ctx-item.disabled:hover {
  background: transparent;
}
.ctx-item.divider {
  border-top: 1px solid var(--el-border-color-light);
  margin-top: 2px;
  padding-top: 8px;
}
.ctx-icon {
  font-size: 14px;
  width: 18px;
  text-align: center;
  flex-shrink: 0;
}
.ctx-label {
  flex: 1;
  white-space: nowrap;
}
</style>