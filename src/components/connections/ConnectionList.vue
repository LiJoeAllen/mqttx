<script setup lang="ts">
import { ref, computed } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import type { MqttConnection } from '../../types/mqtt'
import { useConnections } from '../../stores/useConnections'
import { toConnectionDto } from '../../types/mqtt'
import { mqttConnect, mqttDisconnect } from '../../stores/useMqttBridge'
import ConnectionForm from './ConnectionForm.vue'

const {
  connections,
  add,
  update,
  remove,
  updateStatus,
  getGroups,
  renameGroup,
  deleteGroup,
  exportConnections,
  importConnections,
} = useConnections()

const showForm = ref(false)
const editingConnection = ref<MqttConnection | null>(null)
const selectedGroup = ref<string | null>(null) // null = 全部
const importInput = ref<HTMLInputElement | null>(null)

// ─── 分组 ──────────────────────────────────────────────────────────────────

const groups = computed(() => getGroups())

const filteredConnections = computed(() => {
  if (selectedGroup.value === null) return connections.value
  if (selectedGroup.value === '__ungrouped__') {
    return connections.value.filter((c) => !c.group)
  }
  return connections.value.filter((c) => c.group === selectedGroup.value)
})

const groupCounts = computed(() => {
  const map: Record<string, number> = {}
  for (const c of connections.value) {
    const key = c.group || '__ungrouped__'
    map[key] = (map[key] || 0) + 1
  }
  return map
})

/** 列表内就地修改连接分组（清空 = 未分组，输入新名称 = 直接创建分组） */
function handleGroupChange(conn: MqttConnection, group: string | undefined) {
  update(conn.id, { group: group || '' })
}

async function handleRenameGroup(group: string) {
  const { value } = await ElMessageBox.prompt('重命名分组', '重命名', {
    inputValue: group,
    inputPattern: /\S+/,
    inputErrorMessage: '分组名称不能为空',
  }).catch(() => ({ value: undefined }))
  if (value && value !== group) {
    renameGroup(group, value)
    if (selectedGroup.value === group) selectedGroup.value = value
    ElMessage.success('分组已重命名')
  }
}

async function handleDeleteGroup(group: string) {
  const count = groupCounts.value[group] || 0
  try {
    await ElMessageBox.confirm(
      `删除分组 "${group}"？组内 ${count} 个连接将变为未分组`,
      '确认删除',
      { type: 'warning' },
    )
  } catch {
    return
  }
  deleteGroup(group)
  if (selectedGroup.value === group) selectedGroup.value = null
  ElMessage.success('分组已删除')
}

// ─── 连接操作 ──────────────────────────────────────────────────────────────

async function handleConnect(conn: MqttConnection) {
  try {
    updateStatus(conn.id, 'connecting')
    await mqttConnect(toConnectionDto(conn))
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

// ─── CRUD ──────────────────────────────────────────────────────────────────

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
    update(editingConnection.value.id, data)
    ElMessage.success('连接已更新')
  } else {
    const newConn = { ...data, id: crypto.randomUUID() }
    add()
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

// ─── 导入 / 导出 ──────────────────────────────────────────────────────────

function handleExport() {
  const json = exportConnections()
  const blob = new Blob([json], { type: 'application/json' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = `mqttx_connections_${new Date().toISOString().slice(0, 10)}.json`
  a.click()
  URL.revokeObjectURL(url)
  ElMessage.success(`已导出 ${connections.value.length} 个连接（含明文密码，请妥善保管）`)
}

function handleImportClick() {
  importInput.value?.click()
}

async function handleImportFile(event: Event) {
  const file = (event.target as HTMLInputElement).files?.[0]
  if (!file) return
  try {
    const text = await file.text()
    const { imported, skipped } = importConnections(text)
    if (imported === 0 && skipped > 0) {
      ElMessage.warning(`全部 ${skipped} 个连接与现有内容重复，未导入`)
    } else {
      ElMessage.success(skipped > 0 ? `导入 ${imported} 个连接，跳过 ${skipped} 个重复` : `成功导入 ${imported} 个连接`)
    }
  } catch (e) {
    ElMessage.error(`导入失败: ${e}`)
  } finally {
    if (importInput.value) importInput.value.value = ''
  }
}

// ─── Status helpers ────────────────────────────────────────────────────────

function statusText(status: string): string {
  switch (status) {
    case 'connected': return '已连接'
    case 'connecting': return '连接中...'
    case 'error': return '错误'
    default: return '已断开'
  }
}

function statusType(status: string): string {
  switch (status) {
    case 'connected': return 'success'
    case 'connecting': return 'warning'
    case 'error': return 'danger'
    default: return 'info'
  }
}
</script>

<template>
  <div class="connection-list">
    <div class="list-header">
      <h3>连接管理</h3>
      <div class="header-actions">
        <el-button size="small" @click="handleImportClick">导入</el-button>
        <el-button size="small" :disabled="connections.length === 0" @click="handleExport">导出</el-button>
        <el-button type="primary" size="small" @click="handleAdd">+ 新增连接</el-button>
      </div>
      <input ref="importInput" type="file" accept=".json" style="display: none" @change="handleImportFile" />
    </div>

    <!-- 分组侧栏 -->
    <div v-if="groups.length > 0" class="group-tabs">
      <div
        :class="['group-tab', selectedGroup === null && 'active']"
        @click="selectedGroup = null"
      >
        全部
        <span class="badge">{{ connections.length }}</span>
      </div>
      <div
        :class="['group-tab', selectedGroup === '__ungrouped__' && 'active']"
        @click="selectedGroup = '__ungrouped__'"
      >
        未分组
        <span class="badge">{{ groupCounts['__ungrouped__'] || 0 }}</span>
      </div>
      <div
        v-for="g in groups"
        :key="g"
        :class="['group-tab', selectedGroup === g && 'active']"
        @click="selectedGroup = g"
      >
        <span class="group-name">{{ g }}</span>
        <span class="badge">{{ groupCounts[g] || 0 }}</span>
        <el-dropdown trigger="click" @command="(cmd: string) => cmd === 'rename' ? handleRenameGroup(g) : handleDeleteGroup(g)">
          <span class="group-menu" @click.stop>&#8230;</span>
          <template #dropdown>
            <el-dropdown-menu>
              <el-dropdown-item command="rename">重命名</el-dropdown-item>
              <el-dropdown-item command="delete" divided>删除分组</el-dropdown-item>
            </el-dropdown-menu>
          </template>
        </el-dropdown>
      </div>
    </div>

    <!-- 连接表格 -->
    <el-table :data="filteredConnections" style="width: 100%" stripe>
      <el-table-column prop="name" label="名称" min-width="120" />
      <el-table-column label="分组" width="130">
        <template #default="scope">
          <el-select
            :model-value="scope.row.group"
            placeholder="未分组"
            size="small"
            filterable
            allow-create
            default-first-option
            clearable
            style="width: 100%"
            @change="(g: string) => handleGroupChange(scope.row, g)"
          >
            <el-option v-for="g in groups" :key="g" :label="g" :value="g" />
          </el-select>
        </template>
      </el-table-column>
      <el-table-column label="地址" min-width="180">
        <template #default="scope">
          <span>{{ scope.row.ssl ? 'mqtts' : 'mqtt' }}://{{ scope.row.host }}:{{ scope.row.port }}</span>
        </template>
      </el-table-column>
      <el-table-column label="协议" width="60">
        <template #default="scope">
          <el-tag size="small">{{ scope.row.protocolVersion === '3.1.1' ? 'v3' : 'v5' }}</el-tag>
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
          >连接</el-button>
          <el-button v-else size="small" @click="handleDisconnect(scope.row)">断开</el-button>
          <el-button size="small" @click="handleEdit(scope.row)">编辑</el-button>
          <el-button size="small" type="danger" @click="handleDelete(scope.row)">删除</el-button>
        </template>
      </el-table-column>
    </el-table>

    <div v-if="filteredConnections.length === 0" class="empty-hint">
      {{ connections.length === 0 ? '暂无连接，点击上方按钮创建' : '当前分组下暂无连接' }}
    </div>

    <ConnectionForm
      v-model="showForm"
      :connection="editingConnection"
      @save="handleSave"
    />
  </div>
</template>

<style scoped>
.connection-list { height: 100%; }

.list-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 12px;
}
.list-header h3 { margin: 0; font-size: 16px; }
.header-actions { display: flex; gap: 6px; }

/* 分组标签 */
.group-tabs {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-bottom: 12px;
}
.group-tab {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 4px 10px;
  border-radius: 4px;
  cursor: pointer;
  font-size: 13px;
  background: var(--el-fill-color-light);
  transition: all 0.15s;
}
.group-tab:hover { background: var(--el-fill-color); }
.group-tab.active { background: var(--el-color-primary-light-9); color: var(--el-color-primary); font-weight: 500; }
.group-name { max-width: 120px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.badge {
  font-size: 11px;
  padding: 0 5px;
  border-radius: 8px;
  background: var(--el-fill-color-dark);
  color: var(--el-text-color-secondary);
  line-height: 16px;
}
.group-tab.active .badge { background: var(--el-color-primary); color: #fff; }
.group-menu { cursor: pointer; font-size: 14px; opacity: 0.5; }
.group-menu:hover { opacity: 1; }

.empty-hint { text-align: center; padding: 40px; color: var(--el-text-color-placeholder); }
.status-indicator { display: inline-block; width: 6px; height: 6px; border-radius: 50%; margin-right: 4px; }
.status-indicator.connected { background: var(--el-color-success); }
.status-indicator.connecting { background: var(--el-color-warning); animation: pulse 1s infinite; }
.status-indicator.error { background: var(--el-color-danger); }
.status-indicator.disconnected { background: var(--el-text-color-disabled); }
@keyframes pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.4; } }
</style>
