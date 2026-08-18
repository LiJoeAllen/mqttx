<script setup lang="ts">
import { ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import type { MqttConnection } from '../../types/mqtt'
import { useConnections } from '../../stores/useConnections'
import { toConnectionDto } from '../../types/mqtt'
import { mqttConnect, mqttDisconnect } from '../../stores/useMqttBridge'
import ConnectionForm from './ConnectionForm.vue'

const { connections, add, update, remove, updateStatus } = useConnections()

const showForm = ref(false)
const editingConnection = ref<MqttConnection | null>(null)

// ─── Connection actions ────────────────────────────────────────────────────────

async function handleConnect(conn: MqttConnection) {
  try {
    updateStatus(conn.id, 'connecting')
    await mqttConnect(toConnectionDto(conn))
    // Status will be updated by the event listener
  } catch (e: any) {
    updateStatus(conn.id, 'error', String(e))
    ElMessage.error(`连接失败: ${e}`)
  }
}

async function handleDisconnect(conn: MqttConnection) {
  try {
    await mqttDisconnect(conn.id)
    updateStatus(conn.id, 'disconnected')
    ElMessage.success('已断开')
  } catch (e: any) {
    ElMessage.error(`断开失败: ${e}`)
  }
}

// ─── CRUD ──────────────────────────────────────────────────────────────────────

function handleAdd() {
  editingConnection.value = null
  showForm.value = true
}

function handleEdit(conn: MqttConnection) {
  editingConnection.value = conn
  showForm.value = true
}

function handleSave(data: MqttConnection) {
  if (editingConnection.value) {
    // Update existing
    update(editingConnection.value.id, data)
    ElMessage.success('连接已更新')
  } else {
    // Add new
    const newConn = { ...data, id: crypto.randomUUID() }
    add()
    // Replace the default with our data
    const idx = connections.value.length - 1
    connections.value[idx] = { ...connections.value[idx], ...newConn }
    ElMessage.success('连接已创建')
  }
  showForm.value = false
}

async function handleDelete(conn: MqttConnection) {
  try {
    await ElMessageBox.confirm(`确定删除连接 "${conn.name}"？`, '确认', {
      type: 'warning',
    })
    if (conn.status === 'connected') {
      await handleDisconnect(conn)
    }
    remove(conn.id)
    ElMessage.success('已删除')
  } catch {
    // cancelled
  }
}

// ─── Status helpers ────────────────────────────────────────────────────────────

function statusText(status: string): string {
  switch (status) {
    case 'connected':
      return '已连接'
    case 'connecting':
      return '连接中...'
    case 'error':
      return '错误'
    default:
      return '已断开'
  }
}

function statusType(status: string): string {
  switch (status) {
    case 'connected':
      return 'success'
    case 'connecting':
      return 'warning'
    case 'error':
      return 'danger'
    default:
      return 'info'
  }
}
</script>

<template>
  <div class="connection-list">
    <div class="list-header">
      <h3>连接管理</h3>
      <el-button type="primary" size="small" @click="handleAdd">
        + 新增连接
      </el-button>
    </div>

    <el-table :data="connections" style="width: 100%" stripe>
      <el-table-column prop="name" label="名称" min-width="120" />
      <el-table-column label="地址" min-width="180">
        <template #default="scope">
          <span>{{ scope.row.ssl ? 'mqtts' : 'mqtt' }}://{{ scope.row.host }}:{{ scope.row.port }}</span>
        </template>
      </el-table-column>
      <el-table-column label="协议" width="60">
        <template #default>
          <el-tag size="small">v5</el-tag>
        </template>
      </el-table-column>
      <el-table-column label="状态" width="120">
        <template #default="scope">
          <el-tag :type="statusType(scope.row.status)" size="small" effect="plain">
            <span :class="['status-indicator', scope.row.status]" />
            {{ statusText(scope.row.status) }}
          </el-tag>
        </template>
      </el-table-column>
      <el-table-column label="操作" width="240" fixed="right">
        <template #default="scope">
          <el-button
            v-if="scope.row.status === 'disconnected' || scope.row.status === 'error'"
            size="small"
            type="success"
            :loading="scope.row.status === 'connecting'"
            @click="handleConnect(scope.row)"
          >
            连接
          </el-button>
          <el-button
            v-else
            size="small"
            @click="handleDisconnect(scope.row)"
          >
            断开
          </el-button>
          <el-button
            size="small"
            @click="handleEdit(scope.row)"
          >
            编辑
          </el-button>
          <el-button
            size="small"
            type="danger"
            @click="handleDelete(scope.row)"
          >
            删除
          </el-button>
        </template>
      </el-table-column>
    </el-table>

    <div v-if="connections.length === 0" class="empty-hint">
      暂无连接，点击上方按钮创建
    </div>

    <ConnectionForm
      v-model="showForm"
      :connection="editingConnection"
      @save="handleSave"
    />
  </div>
</template>

<style scoped>
.connection-list {
  height: 100%;
}
.list-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 12px;
}
.list-header h3 {
  margin: 0;
  font-size: 16px;
}
.empty-hint {
  text-align: center;
  padding: 40px;
  color: var(--el-text-color-placeholder);
}
.status-indicator {
  display: inline-block;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  margin-right: 4px;
}
.status-indicator.connected {
  background: var(--el-color-success);
}
.status-indicator.connecting {
  background: var(--el-color-warning);
  animation: pulse 1s infinite;
}
.status-indicator.error {
  background: var(--el-color-danger);
}
.status-indicator.disconnected {
  background: var(--el-text-color-disabled);
}
@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.4; }
}
</style>