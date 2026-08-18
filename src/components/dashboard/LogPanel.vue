<script setup lang="ts">
import { ref, computed, watch, nextTick } from 'vue'
import { useLogs } from '../../stores/useLogs'
import { mqttGetLogDir } from '../../stores/useMqttBridge'
import { prettyJson } from '../../utils/format'
import { ElMessage } from 'element-plus'

const { logs, clear } = useLogs()

const filterLevel = ref('')
const filterConnId = ref('')
const showPanel = defineModel<boolean>('show', { default: false })
const expandedDetails = ref(new Set<string>())
const logDirPath = ref('')

function toggleDetails(id: string) {
  const s = new Set(expandedDetails.value)
  if (s.has(id)) s.delete(id)
  else s.add(id)
  expandedDetails.value = s
}

const filteredLogs = computed(() => {
  let result = logs.value
  if (filterLevel.value) result = result.filter((l) => l.level === filterLevel.value)
  if (filterConnId.value) result = result.filter((l) => l.connectionId === filterConnId.value)
  return result
})

const connectionIds = computed(() => [...new Set(logs.value.map((l) => l.connectionId))])

const logContainer = ref<HTMLDivElement | null>(null)
const autoScroll = ref(true)

function onScroll() {
  if (!logContainer.value) return
  const el = logContainer.value
  autoScroll.value = el.scrollHeight - el.scrollTop - el.clientHeight < 30
}

watch(filteredLogs, async () => {
  if (autoScroll.value) {
    await nextTick()
    if (logContainer.value) logContainer.value.scrollTop = logContainer.value.scrollHeight
  }
})

function formatTime(ts: number): string {
  const d = new Date(ts)
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}:${String(d.getSeconds()).padStart(2, '0')}.${String(d.getMilliseconds()).padStart(3, '0')}`
}

async function handleOpenLogDir() {
  try {
    if (!logDirPath.value) {
      logDirPath.value = await mqttGetLogDir()
    }
    // Try to open the directory using the opener plugin
    const { open } = await import('@tauri-apps/plugin-opener')
    await open(logDirPath.value)
  } catch (e: any) {
    ElMessage.warning(`日志目录: ${logDirPath.value || '未知'}`)
  }
}

function eventIcon(event: string): string {
  switch (event) {
    case 'connect': return '🔌'
    case 'connack': return '✅'
    case 'disconnect': case 'disconnect_sent': case 'disconnect_received': case 'disconnect_cancelled': return '🔌'
    case 'event_loop_started': return '▶️'
    case 'publish_sent': return '📤'
    case 'publish_received': return '📥'
    case 'publish_error': case 'subscribe_error': case 'unsubscribe_error': case 'connection_error': return '❌'
    case 'subscribe': return '📋'
    case 'suback': case 'unsuback': return '✅'
    case 'unsubscribe': return '🗑️'
    case 'puback': case 'pubrec': case 'pubcomp': return '📨'
    case 'requests_done': return '🏁'
    default: return '📝'
  }
}
</script>

<template>
  <div class="log-panel">
    <div class="log-header">
      <div class="log-title-row">
        <span class="log-title">事件日志</span>
        <span class="log-count">{{ filteredLogs.length }}</span>
      </div>
      <div class="log-tools">
        <el-select v-model="filterLevel" size="small" placeholder="级别" style="width: 80px" clearable>
          <el-option value="" label="全部" />
          <el-option value="info" label="信息" />
          <el-option value="warn" label="警告" />
          <el-option value="error" label="错误" />
        </el-select>
        <el-select v-model="filterConnId" size="small" placeholder="连接" style="width: 100px" clearable>
          <el-option value="" label="全部" />
          <el-option v-for="cid in connectionIds" :key="cid" :value="cid" :label="cid.substring(0, 6) + '…'" />
        </el-select>
        <el-button size="small" class="log-action-btn" @click="handleOpenLogDir" title="打开日志文件夹">
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
          文件夹
        </el-button>
        <el-button size="small" class="log-action-btn" @click="clear()">清空</el-button>
        <el-button size="small" class="log-action-btn" @click="showPanel = false">✕</el-button>
      </div>
    </div>
    <div class="log-list" ref="logContainer" @scroll="onScroll">
      <div v-if="filteredLogs.length === 0" class="log-empty">暂无日志</div>
      <div
        v-for="log in filteredLogs"
        :key="log.id"
      >
        <div :class="['log-entry', `level-${log.level}`]">
          <span class="log-time">{{ formatTime(log.timestamp) }}</span>
          <span :class="['log-lvl', `lvl-${log.level}`]">{{ { info: 'INF', warn: 'WRN', error: 'ERR' }[log.level] }}</span>
          <span class="log-ico">{{ eventIcon(log.event) }}</span>
          <span class="log-msg">{{ log.message }}</span>
          <span v-if="log.details" class="log-dtl-btn" @click="toggleDetails(log.id)">{{ expandedDetails.has(log.id) ? '收起' : '详情' }}</span>
        </div>
        <div v-if="log.details && expandedDetails.has(log.id)" class="log-dtl-content">
          {{ prettyJson(log.details) }}
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.log-panel {
  border: 1px solid var(--el-border-color);
  border-radius: 8px;
  background: var(--el-bg-color);
  display: flex;
  flex-direction: column;
  max-height: 220px;
  font-size: 12px;
  font-family: 'Cascadia Code', 'Fira Code', 'Consolas', monospace;
  flex-shrink: 0;
}
.log-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 10px;
  border-bottom: 1px solid var(--el-border-color-light);
  flex-shrink: 0;
  gap: 8px;
}
.log-title-row {
  display: flex;
  align-items: center;
  gap: 6px;
}
.log-title {
  font-size: 13px;
  font-weight: 600;
  white-space: nowrap;
}
.log-count {
  font-size: 10px;
  color: var(--el-text-color-secondary);
  background: var(--el-fill-color);
  padding: 0 5px;
  border-radius: 6px;
  line-height: 16px;
}
.log-tools {
  display: flex;
  align-items: center;
  gap: 4px;
}
.log-action-btn {
  --el-button-bg-color: transparent;
  --el-button-border-color: var(--el-border-color);
  --el-button-hover-bg-color: var(--el-fill-color);
  font-size: 11px;
  height: 24px;
  padding: 0 6px !important;
  border-radius: 4px;
  display: inline-flex;
  align-items: center;
  gap: 3px;
}
.log-action-btn svg {
  flex-shrink: 0;
}
.log-list {
  flex: 1;
  overflow-y: auto;
  padding: 2px 0;
}
.log-empty {
  padding: 24px;
  text-align: center;
  color: var(--el-text-color-placeholder);
}
.log-entry {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 2px 8px;
  transition: background 0.1s;
}
.log-entry:hover { background: var(--el-fill-color-light); }
.log-time { font-size: 11px; color: var(--el-text-color-secondary); white-space: nowrap; flex-shrink: 0; }
.log-lvl { font-size: 10px; font-weight: 700; padding: 0 3px; border-radius: 2px; flex-shrink: 0; }
.lvl-info { color: var(--el-color-primary); background: var(--el-color-primary-light-9); }
.lvl-warn { color: var(--el-color-warning); background: var(--el-color-warning-light-9); }
.lvl-err { color: var(--el-color-danger); background: var(--el-color-danger-light-9); }
.log-ico { font-size: 11px; flex-shrink: 0; }
.log-msg { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--el-text-color-regular); }
.log-dtl-btn { font-size: 11px; color: var(--el-color-primary); cursor: pointer; flex-shrink: 0; opacity: 0; }
.log-entry:hover .log-dtl-btn { opacity: 1; }
.log-dtl-content {
  padding: 2px 8px 4px 20px;
  font-size: 11px;
  color: var(--el-text-color-secondary);
  background: var(--el-fill-color-lighter);
  word-break: break-all;
  white-space: pre-wrap;
  max-height: 80px;
  overflow-y: auto;
}
</style>