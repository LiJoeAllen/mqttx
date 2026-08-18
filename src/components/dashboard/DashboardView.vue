<script setup lang="ts">
import { ref, computed } from 'vue'
import { useConnections } from '../../stores/useConnections'
import { usePresets } from '../../stores/usePresets'
import { useMessages } from '../../stores/useMessages'
import { mqttConnect, mqttDisconnect } from '../../stores/useMqttBridge'
import { toConnectionDto } from '../../types/mqtt'
import { ElMessage } from 'element-plus'
import FilterBar from './FilterBar.vue'
import TopicTree from './TopicTree.vue'
import MessageStream from './MessageStream.vue'
import SendPanel from './SendPanel.vue'
import LogPanel from './LogPanel.vue'
import type { PublishDto } from '../../types/mqtt'

const { connections, updateStatus, getConnected } = useConnections()
const { presets } = usePresets()
const { messages, clear, filterByTopic, filterByConnection, messageCount } = useMessages()

const topicFilter = ref('')
const selectedConnectionId = ref('')
const selectedTopic = ref('')
const paused = ref(false)
const showOutgoing = ref(true)
const showTopicTree = ref(true)
const showLogPanel = ref(false)
const connecting = ref(false)

const filteredMessages = computed(() => {
  let result = messages.value
  if (topicFilter.value.trim()) {
    result = filterByTopic(topicFilter.value)
  }
  if (selectedConnectionId.value) {
    result = filterByConnection(selectedConnectionId.value)
  }
  if (selectedTopic.value) {
    result = result.filter((m) => m.topic === selectedTopic.value || m.topic.startsWith(selectedTopic.value + '/'))
  }
  if (!showOutgoing.value) {
    result = result.filter((m) => m.direction === 'in')
  }
  return result
})

// Selected connection object
const selectedConn = computed(() =>
  connections.value.find((c) => c.id === selectedConnectionId.value),
)

const isConnected = computed(() => selectedConn.value?.status === 'connected')

async function handleToggleConnect() {
  const conn = selectedConn.value
  if (!conn) return

  if (isConnected.value) {
    try {
      await mqttDisconnect(conn.id)
      updateStatus(conn.id, 'disconnected')
      ElMessage.success('已断开')
    } catch (e: any) {
      ElMessage.error(`断开失败: ${e}`)
    }
  } else {
    connecting.value = true
    try {
      updateStatus(conn.id, 'connecting')
      await mqttConnect(toConnectionDto(conn))
      // Status will be updated by the event listener
    } catch (e: any) {
      updateStatus(conn.id, 'error', String(e))
      ElMessage.error(`连接失败: ${e}`)
    } finally {
      connecting.value = false
    }
  }
}

function handleClear() {
  clear()
  ElMessage.success('消息已清空')
}

function handleSent(_publish: PublishDto) {
  // Outgoing message is already handled by the backend echo
}

function handleTopicSelect(topic: string) {
  selectedTopic.value = topic
}
</script>

<template>
  <div class="dashboard">
    <!-- Toolbar -->
    <div class="toolbar-card">
      <div class="toolbar-left">
        <!-- Connection Selector -->
        <div class="toolbar-group">
          <div class="conn-select-wrap">
            <el-select
              v-model="selectedConnectionId"
              placeholder="选择连接"
              size="small"
              style="width: 180px"
              clearable
            >
              <el-option
                v-for="c in connections"
                :key="c.id"
                :label="c.name"
                :value="c.id"
              >
                <div class="conn-option">
                  <span>{{ c.name }}</span>
                  <span :class="['status-badge', c.status]" />
                </div>
              </el-option>
            </el-select>
            <button
              v-if="selectedConn"
              :class="['conn-toggle-btn', { connected: isConnected, connecting: selectedConn.status === 'connecting' }]"
              :disabled="selectedConn.status === 'connecting' || connecting"
              @click="handleToggleConnect"
              :title="isConnected ? '断开连接' : '连接'"
            >
              <svg v-if="isConnected" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/></svg>
              <svg v-else width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="1" y1="1" x2="23" y2="23"/><path d="M16.72 3.7A9 9 0 0 1 21 6.5"/><path d="M3.5 6.5a9 9 0 0 1 5.22-3.2"/><path d="M9 12a3 3 0 0 1 3-3 3 3 0 0 1 1.24.26"/><path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/></svg>
            </button>
          </div>
          <FilterBar v-model="topicFilter" />
        </div>

        <!-- View Toggles -->
        <div class="toolbar-group">
          <el-button
            size="small"
            :class="['view-btn', { active: showTopicTree }]"
            @click="showTopicTree = !showTopicTree"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M5 3h14v4H5z"/><path d="M3 10h7v11H3z"/><path d="M14 10h7v11h-7z"/></svg>
            主题树
          </el-button>
          <el-button
            size="small"
            :class="['view-btn', { active: showLogPanel }]"
            @click="showLogPanel = !showLogPanel"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="16" y1="13" x2="8" y2="13"/><line x1="16" y1="17" x2="8" y2="17"/></svg>
            日志
          </el-button>
        </div>
      </div>

      <div class="toolbar-right">
        <el-checkbox v-model="showOutgoing" size="small">
          <span class="checkbox-label">显示发送</span>
        </el-checkbox>
        <div class="msg-stat">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>
          <span>{{ messageCount }}</span>
        </div>
        <div class="toolbar-divider" />
        <el-button
          size="small"
          :class="['action-btn', { 'is-paused': paused }]"
          @click="paused = !paused"
        >
          <svg v-if="paused" width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><polygon points="5 3 19 12 5 21 5 3"/></svg>
          <svg v-else width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="4" width="4" height="16"/><rect x="14" y="4" width="4" height="16"/></svg>
          {{ paused ? '继续' : '暂停' }}
        </el-button>
        <el-button
          size="small"
          class="action-btn danger"
          @click="handleClear"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
          清空
        </el-button>
      </div>
    </div>

    <!-- Connection not connected hint -->
    <div
      v-if="selectedConn && !isConnected && selectedConn.status !== 'connecting'"
      class="connect-hint"
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/></svg>
      连接「{{ selectedConn.name }}」未连接，请先
      <button class="connect-hint-btn" @click="handleToggleConnect">点击连接</button>
    </div>

    <!-- Main Content -->
    <div class="content-area">
      <TopicTree
        v-if="showTopicTree"
        :messages="messages"
        :selected-topic="selectedTopic"
        :connection-id="selectedConnectionId"
        :connected="isConnected"
        @select="handleTopicSelect"
      />
      <MessageStream
        :messages="filteredMessages"
        :paused="paused"
        @update:paused="paused = $event"
      />
    </div>

    <!-- Log Panel -->
    <LogPanel v-if="showLogPanel" v-model:show="showLogPanel" />

    <!-- Send Panel -->
    <SendPanel
      :connections="connections"
      :presets="presets"
      @sent="handleSent"
    />
  </div>
</template>

<style scoped>
.dashboard {
  display: flex;
  flex-direction: column;
  height: 100%;
  gap: 10px;
}

/* ─── Toolbar ─────────────────────────────────────────────────── */
.toolbar-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 10px 14px;
  background: var(--el-bg-color);
  border: 1px solid var(--el-border-color);
  border-radius: 8px;
  flex-shrink: 0;
}
.toolbar-left {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}
.toolbar-right {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}
.toolbar-group {
  display: flex;
  align-items: center;
  gap: 6px;
}
.toolbar-divider {
  width: 1px;
  height: 20px;
  background: var(--el-border-color);
}

/* Connection selector with toggle button */
.conn-select-wrap {
  display: flex;
  align-items: center;
  gap: 4px;
}
.conn-option {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
}
.status-badge {
  display: inline-block;
  width: 7px;
  height: 7px;
  border-radius: 50%;
  flex-shrink: 0;
}
.status-badge.connected { background: var(--el-color-success); }
.status-badge.disconnected { background: var(--el-text-color-disabled); }
.status-badge.connecting { background: var(--el-color-warning); }
.status-badge.error { background: var(--el-color-danger); }

.conn-toggle-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: 1px solid var(--el-border-color);
  border-radius: 6px;
  background: var(--el-bg-color);
  cursor: pointer;
  color: var(--el-text-color-secondary);
  transition: all 0.15s;
  flex-shrink: 0;
}
.conn-toggle-btn:hover {
  border-color: var(--el-color-primary);
  color: var(--el-color-primary);
}
.conn-toggle-btn.connected {
  color: var(--el-color-success);
  border-color: var(--el-color-success);
}
.conn-toggle-btn.connecting {
  opacity: 0.5;
  cursor: not-allowed;
}

/* View toggle buttons */
.view-btn {
  --el-button-bg-color: transparent;
  --el-button-border-color: var(--el-border-color);
  --el-button-hover-bg-color: var(--el-fill-color);
  --el-button-hover-border-color: var(--el-border-color);
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
  color: var(--el-text-color-secondary);
  padding: 0 10px !important;
  height: 28px;
  border-radius: 6px;
}
.view-btn.active {
  --el-button-bg-color: var(--el-color-primary-light-9);
  --el-button-border-color: var(--el-color-primary-light-5);
  color: var(--el-color-primary);
}
.view-btn svg {
  flex-shrink: 0;
}

/* Action buttons */
.action-btn {
  --el-button-bg-color: transparent;
  --el-button-border-color: var(--el-border-color);
  --el-button-hover-bg-color: var(--el-fill-color);
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
  padding: 0 10px !important;
  height: 28px;
  border-radius: 6px;
}
.action-btn.is-paused {
  --el-button-bg-color: var(--el-color-warning-light-9);
  --el-button-border-color: var(--el-color-warning-light-5);
  color: var(--el-color-warning);
}
.action-btn.danger {
  color: var(--el-color-danger);
}
.action-btn svg {
  flex-shrink: 0;
}

/* Message stat */
.msg-stat {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
  color: var(--el-text-color-secondary);
  white-space: nowrap;
}
.msg-stat svg {
  flex-shrink: 0;
}

.checkbox-label {
  font-size: 12px;
}

/* ─── Connection hint ──────────────────────────────────────────── */
.connect-hint {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  background: var(--el-color-warning-light-9);
  border: 1px solid var(--el-color-warning-light-7);
  border-radius: 6px;
  font-size: 12px;
  color: var(--el-color-warning);
  flex-shrink: 0;
}
.connect-hint svg {
  flex-shrink: 0;
}
.connect-hint-btn {
  background: none;
  border: none;
  color: var(--el-color-primary);
  cursor: pointer;
  font-size: 12px;
  font-weight: 600;
  text-decoration: underline;
  font-family: inherit;
  padding: 0;
}
.connect-hint-btn:hover {
  color: var(--el-color-primary-dark-2);
}

/* ─── Content Area ────────────────────────────────────────────── */
.content-area {
  display: flex;
  flex: 1;
  gap: 10px;
  min-height: 0;
}
</style>