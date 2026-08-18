<script setup lang="ts">
import { ref } from 'vue'

const props = withDefaults(defineProps<{
  /** Full text to show in the preview */
  text: string
  /** Maximum width of the preview popup */
  maxWidth?: number
  /** Maximum height of the preview popup */
  maxHeight?: number
  /** Delay before showing (ms) */
  delay?: number
}>(), {
  maxWidth: 480,
  maxHeight: 360,
  delay: 400,
})

const visible = ref(false)
const x = ref(0)
const y = ref(0)
let timer: ReturnType<typeof setTimeout> | null = null

function onMouseEnter(ev: MouseEvent) {
  timer = setTimeout(() => {
    visible.value = true
    updatePosition(ev)
  }, props.delay)
}

function onMouseMove(ev: MouseEvent) {
  if (visible.value) {
    updatePosition(ev)
  }
}

function onMouseLeave() {
  if (timer) {
    clearTimeout(timer)
    timer = null
  }
  visible.value = false
}

function updatePosition(ev: MouseEvent) {
  const pad = 14
  const popupW = Math.min(props.maxWidth, 480)
  const popupH = Math.min(props.maxHeight, 360)

  let left = ev.clientX + pad
  let top = ev.clientY + pad

  // Flip to left side if too close to right edge
  if (left + popupW + 16 > window.innerWidth) {
    left = ev.clientX - popupW - pad
  }
  // Flip to top side if too close to bottom edge
  if (top + popupH + 16 > window.innerHeight) {
    top = ev.clientY - popupH - pad
  }
  // Clamp to safe bounds
  left = Math.max(8, Math.min(left, window.innerWidth - popupW - 8))
  top = Math.max(8, Math.min(top, window.innerHeight - popupH - 8))

  x.value = left
  y.value = top
}
</script>

<template>
  <span
    class="hover-preview-wrap"
    @mouseenter="onMouseEnter"
    @mousemove="onMouseMove"
    @mouseleave="onMouseLeave"
  >
    <slot />
    <Teleport to="body">
      <div
        v-if="visible && text"
        class="hover-preview-popup"
        :style="{
          left: x + 'px',
          top: y + 'px',
          maxWidth: maxWidth + 'px',
          maxHeight: maxHeight + 'px',
        }"
      >
        <pre class="hover-preview-content"><code>{{ text }}</code></pre>
      </div>
    </Teleport>
  </span>
</template>

<style scoped>
.hover-preview-wrap {
  display: inline;
  cursor: pointer;
}
.hover-preview-popup {
  position: fixed;
  z-index: 10000;
  background: var(--comfort-bg-card, var(--el-bg-color-overlay));
  border: 1px solid var(--comfort-border, var(--el-border-color));
  border-radius: 6px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.12), 0 1px 4px rgba(0, 0, 0, 0.08);
  padding: 8px 10px;
  overflow: auto;
  pointer-events: none;
}
.hover-preview-content {
  margin: 0;
  font-family: var(--comfort-font-mono, 'Cascadia Code', 'Fira Code', 'Consolas', monospace);
  font-size: 12px;
  line-height: 1.5;
  color: var(--comfort-text, var(--el-text-color-regular));
  white-space: pre-wrap;
  word-break: break-all;
}
</style>