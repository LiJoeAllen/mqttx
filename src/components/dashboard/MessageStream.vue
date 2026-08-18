<script setup lang="ts">
import { ref, watch, nextTick } from 'vue'
import type { MqttMessage } from '../../types/mqtt'

const props = defineProps<{
  messages: MqttMessage[]
  paused: boolean
}>()

const emit = defineEmits<{
  'update:paused': [value: boolean]
}>()

const scrollRef = ref<HTMLDivElement | null>(null)
const autoScroll = ref(true)
const expandedPayloads = ref(new Set<string>())
const messageFormats = ref<Record<string, 'auto' | 'json' | 'hex' | 'base64' | 'plain'>>({})

const formatOptions: { value: string; label: string }[] = [
  { value: 'auto', label: '自动' },
  { value: 'json', label: 'JSON' },
  { value: 'hex', label: 'Hex' },
  { value: 'base64', label: 'Base64' },
  { value: 'plain', label: '文本' },
]

watch(
  () => props.messages.length,
  async () => {
    if (autoScroll.value && !props.paused) {
      await nextTick()
      if (scrollRef.value) {
        scrollRef.value.scrollTop = scrollRef.value.scrollHeight
      }
    }
  },
)

function onScroll() {
  if (!scrollRef.value) return
  const el = scrollRef.value
  const isAtBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 50
  autoScroll.value = isAtBottom
  if (!isAtBottom && !props.paused) {
    emit('update:paused', true)
  } else if (isAtBottom && props.paused) {
    emit('update:paused', false)
  }
}

// ─── Payload Format Helpers ────────────────────────────────────

function isJson(str: string): boolean {
  try { JSON.parse(str); return true }
  catch { return false }
}

function getEffectiveFormat(msg: MqttMessage): string {
  const fmt = messageFormats.value[msg.id] || 'auto'
  if (fmt === 'auto') return isJson(msg.payload) ? 'json' : 'plain'
  return fmt
}

function formatPayload(msg: MqttMessage): string {
  const fmt = getEffectiveFormat(msg)
  switch (fmt) {
    case 'json':
      try {
        const parsed = JSON.parse(msg.payload)
        return JSON.stringify(parsed, null, 2)
      } catch { return msg.payload }
    case 'hex':
      return bytesToHex(new TextEncoder().encode(msg.payload))
    case 'base64':
      return btoa(msg.payload)
    case 'plain':
    default:
      return msg.payload
  }
}

function bytesToHex(bytes: Uint8Array): string {
  const parts: string[] = []
  for (let i = 0; i < bytes.length; i++) {
    if (i > 0 && i % 16 === 0) parts.push('\n')
    else if (i > 0) parts.push(' ')
    parts.push(bytes[i].toString(16).padStart(2, '0'))
  }
  return parts.join('')
}

function setFormat(msgId: string, fmt: 'auto' | 'json' | 'hex' | 'base64' | 'plain') {
  messageFormats.value = { ...messageFormats.value, [msgId]: fmt }
}

function togglePayload(msgId: string) {
  const s = new Set(expandedPayloads.value)
  if (s.has(msgId)) s.delete(msgId)
  else s.add(msgId)
  expandedPayloads.value = s
}

// ─── Display helpers ───────────────────────────────────────────

function formatTime(ts: number): string {
  const d = new Date(ts)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}.${String(d.getMilliseconds()).padStart(3, '0')}`
}
</script>

<template>
  <div class="message-stream" ref="scrollRef" @scroll="onScroll">
    <div v-if="messages.length === 0" class="empty-hint">
      <div class="empty-icon">
        <svg width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" opacity="0.3">
          <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>
        </svg>
      </div>
      <div class="empty-text">暂无消息</div>
      <div class="empty-sub">连接后消息将自动出现在这里</div>
    </div>

    <div
      v-for="msg in messages"
      :key="msg.id"
      :class="['msg-card', msg.direction === 'in' ? 'msg-in' : 'msg-out']"
    >
      <!-- Header -->
      <div class="msg-header">
        <span :class="['dir-badge', msg.direction]">
          <svg v-if="msg.direction === 'in'" width="10" height="10" viewBox="0 0 24 24" fill="currentColor"><path d="M12 2a10 10 0 1 0 10 10A10 10 0 0 0 12 2zm-1 14.5v-9l5 4.5z"/></svg>
          <svg v-else width="10" height="10" viewBox="0 0 24 24" fill="currentColor"><path d="M12 2a10 10 0 1 0 10 10A10 10 0 0 0 12 2zm1 14.5v-9l5 4.5z"/></svg>
          {{ msg.direction === 'in' ? '收' : '发' }}
        </span>
        <span class="msg-time">{{ formatTime(msg.timestamp) }}</span>
        <span class="msg-topic">{{ msg.topic }}</span>
        <span class="qos-badge">Q{{ msg.qos }}</span>
        <span v-if="msg.retain" class="retain-badge">R</span>
      </div>

      <!-- v5 Properties -->
      <div v-if="msg.contentType || msg.reasonCode !== undefined || msg.messageExpiryInterval" class="v5-bar">
        <span v-if="msg.contentType" class="v5-chip">CT: {{ msg.contentType }}</span>
        <span v-if="msg.reasonCode !== undefined" class="v5-chip">RC: {{ msg.reasonCode }}</span>
        <span v-if="msg.messageExpiryInterval" class="v5-chip">Exp: {{ msg.messageExpiryInterval }}s</span>
      </div>
      <div v-if="msg.userProperties && msg.userProperties.length > 0" class="v5-bar">
        <span v-for="(up, i) in msg.userProperties.slice(0, 3)" :key="i" class="v5-chip">
          {{ up.key }}: {{ up.value }}
        </span>
        <span v-if="msg.userProperties.length > 3" class="v5-chip more">+{{ msg.userProperties.length - 3 }}</span>
      </div>

      <!-- Format Tabs + Expand -->
      <div class="payload-bar">
        <div class="format-tabs">
          <span
            v-for="opt in formatOptions"
            :key="opt.value"
            :class="['fmt-tab', { active: (messageFormats[msg.id] || 'auto') === opt.value }]"
            @click="setFormat(msg.id, opt.value as any)"
          >{{ opt.label }}</span>
        </div>
        <span
          class="expand-btn"
          @click="togglePayload(msg.id)"
        >
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <polyline v-if="expandedPayloads.has(msg.id)" points="18 15 12 9 6 15"/>
            <polyline v-else points="6 9 12 15 18 9"/>
          </svg>
          {{ expandedPayloads.has(msg.id) ? '收起' : '展开' }}
        </span>
      </div>

      <!-- Payload Content -->
      <div class="msg-payload" :class="{ expanded: expandedPayloads.has(msg.id) }">
        <pre
          v-if="expandedPayloads.has(msg.id)"
          :class="['payload-full', { 'is-json': getEffectiveFormat(msg) === 'json' }]"
        >{{ formatPayload(msg) }}</pre>
        <pre v-else class="payload-truncated">{{ msg.payload.substring(0, 200) }}{{ msg.payload.length > 200 ? '...' : '' }}</pre>
      </div>
    </div>
  </div>
</template>

<style scoped>
.message-stream {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 4px;
  font-family: 'Cascadia Code', 'Fira Code', 'Consolas', monospace;
  font-size: 13px;
}

/* ─── Empty State ────────────────────────────────────────────── */
.empty-hint {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  flex: 1;
  color: var(--el-text-color-placeholder);
  gap: 6px;
}
.empty-text {
  font-size: 14px;
  font-weight: 500;
}
.empty-sub {
  font-size: 12px;
  opacity: 0.7;
}

/* ─── Message Card ───────────────────────────────────────────── */
.msg-card {
  background: var(--el-bg-color);
  border: 1px solid var(--el-border-color-light);
  border-radius: 8px;
  padding: 8px 10px;
  transition: box-shadow 0.15s;
}
.msg-card:hover {
  box-shadow: 0 1px 4px rgba(0,0,0,0.06);
}
.msg-card.msg-in {
  border-left: 3px solid var(--el-color-primary);
}
.msg-card.msg-out {
  border-left: 3px solid var(--el-color-success);
}

/* ─── Header ─────────────────────────────────────────────────── */
.msg-header {
  display: flex;
  align-items: center;
  gap: 6px;
}
.dir-badge {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  font-size: 10px;
  font-weight: 600;
  padding: 1px 5px;
  border-radius: 4px;
  flex-shrink: 0;
}
.dir-badge.in {
  background: var(--el-color-primary-light-9);
  color: var(--el-color-primary);
}
.dir-badge.out {
  background: var(--el-color-success-light-9);
  color: var(--el-color-success);
}
.dir-badge svg {
  flex-shrink: 0;
}
.msg-time {
  font-size: 11px;
  color: var(--el-text-color-secondary);
  white-space: nowrap;
  flex-shrink: 0;
}
.msg-topic {
  font-weight: 500;
  color: var(--el-text-color-primary);
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
}
.qos-badge {
  font-size: 10px;
  color: var(--el-text-color-secondary);
  background: var(--el-fill-color);
  padding: 0 4px;
  border-radius: 3px;
  flex-shrink: 0;
}
.retain-badge {
  font-size: 10px;
  font-weight: 700;
  color: var(--el-color-warning);
  background: var(--el-color-warning-light-9);
  padding: 0 4px;
  border-radius: 3px;
  flex-shrink: 0;
}

/* ─── v5 Properties ──────────────────────────────────────────── */
.v5-bar {
  display: flex;
  gap: 4px;
  flex-wrap: wrap;
  margin-top: 4px;
}
.v5-chip {
  font-size: 10px;
  color: var(--el-color-info);
  background: var(--el-color-info-light-9);
  padding: 0 5px;
  border-radius: 3px;
  line-height: 18px;
}
.v5-chip.more {
  color: var(--el-text-color-secondary);
  background: var(--el-fill-color);
}

/* ─── Payload Bar ────────────────────────────────────────────── */
.payload-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: 4px;
}
.format-tabs {
  display: flex;
  gap: 1px;
  background: var(--el-fill-color);
  border-radius: 4px;
  padding: 1px;
}
.fmt-tab {
  font-size: 10px;
  padding: 1px 6px;
  cursor: pointer;
  border-radius: 3px;
  color: var(--el-text-color-secondary);
  transition: all 0.12s;
  line-height: 18px;
}
.fmt-tab:hover {
  color: var(--el-color-primary);
}
.fmt-tab.active {
  color: var(--el-color-primary);
  background: var(--el-bg-color);
  font-weight: 600;
  box-shadow: 0 1px 2px rgba(0,0,0,0.06);
}
.expand-btn {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  font-size: 11px;
  color: var(--el-color-primary);
  cursor: pointer;
  opacity: 0.6;
  transition: opacity 0.12s;
}
.expand-btn:hover {
  opacity: 1;
}

/* ─── Payload ────────────────────────────────────────────────── */
.msg-payload {
  margin-top: 2px;
}
.msg-payload pre {
  margin: 0;
  white-space: pre-wrap;
  word-break: break-all;
  color: var(--el-text-color-regular);
}
.payload-full {
  max-height: 300px;
  overflow: auto;
  background: var(--el-fill-color-lighter);
  padding: 6px 8px;
  border-radius: 6px;
  font-size: 12px;
  line-height: 1.5;
  border: 1px solid var(--el-border-color-extra-light);
}
.payload-full.is-json {
  color: var(--el-color-primary-dark-2);
}
.payload-truncated {
  max-height: 1.4em;
  overflow: hidden;
  font-size: 12px;
  color: var(--el-text-color-secondary);
}
</style>