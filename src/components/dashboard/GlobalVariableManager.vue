<script setup lang="ts">
import { ref } from 'vue'
import { ElMessage } from 'element-plus'
import { useGlobalVariables } from '../../stores/useGlobalVariables'

const { variables, setVariable, deleteVariable, clear } = useGlobalVariables()

const showPanel = defineModel<boolean>('show', { default: false })

const newVarName = ref('')
const newVarValue = ref('')

function addVariable() {
  const name = newVarName.value.trim()
  if (!name) {
    ElMessage.warning('请输入变量名')
    return
  }
  if (!/^\w+$/.test(name)) {
    ElMessage.warning('变量名只能包含字母、数字和下划线')
    return
  }
  setVariable(name, newVarValue.value.trim())
  newVarName.value = ''
  newVarValue.value = ''
  ElMessage.success(`已添加变量: {{${name}}}`)
}

function handleDelete(name: string) {
  deleteVariable(name)
  ElMessage.success(`已删除变量: {{${name}}}`)
}

function handleClear() {
  clear()
  ElMessage.success('已清空所有全局变量')
}

const entries = ref<{ name: string; value: string }[]>([])

// Refresh entries when panel opens
function refreshEntries() {
  const vars = useGlobalVariables().getAllVariables()
  entries.value = Object.entries(vars).map(([name, value]) => ({ name, value }))
}
</script>

<template>
  <el-dialog
    :model-value="showPanel"
    title="全局变量管理"
    width="500px"
    @update:model-value="(val) => showPanel = val"
    @open="refreshEntries"
    @closed="newVarName = ''; newVarValue = ''"
  >
    <div class="global-var-mgr">
      <!-- Add new variable -->
      <div class="add-row">
        <el-input
          v-model="newVarName"
          size="small"
          placeholder="变量名"
          style="width: 140px"
          @keyup.enter="addVariable"
        />
        <span class="equals-sign">=</span>
        <el-input
          v-model="newVarValue"
          size="small"
          placeholder="值"
          style="flex: 1"
          @keyup.enter="addVariable"
        />
        <el-button size="small" type="primary" @click="addVariable">添加</el-button>
      </div>

      <div class="hint-text">
        变量名只能包含字母、数字和下划线。在预设模板中使用 <code v-pre>{{变量名}}</code> 引用。
      </div>

      <!-- Variable list -->
      <div v-if="entries.length === 0" class="empty-state">
        暂无全局变量，在上方添加
      </div>

      <div v-else class="var-list">
        <div
          v-for="(entry, idx) in entries"
          :key="entry.name"
          class="var-row"
        >
          <span class="var-name">{{ entry.name }}</span>
          <span class="var-eq">=</span>
          <el-input
            :model-value="entry.value"
            size="small"
            style="flex: 1"
            @input="(val: string) => { setVariable(entry.name, val); entries[idx] = { ...entries[idx], value: val } }"
          />
          <el-button
            size="small"
            text
            type="danger"
            @click="handleDelete(entry.name)"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
          </el-button>
        </div>
      </div>
    </div>

    <template #footer>
      <el-button @click="showPanel = false">关闭</el-button>
      <el-button
        v-if="entries.length > 0"
        type="danger"
        plain
        @click="handleClear"
      >
        清空全部
      </el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.global-var-mgr {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.add-row {
  display: flex;
  align-items: center;
  gap: 6px;
}
.equals-sign {
  font-family: monospace;
  color: var(--el-text-color-secondary);
  font-size: 14px;
}
.hint-text {
  font-size: 12px;
  color: var(--el-text-color-secondary);
  line-height: 1.4;
}
.hint-text code {
  background: var(--el-fill-color-light);
  padding: 0 4px;
  border-radius: 2px;
  font-family: monospace;
}
.empty-state {
  text-align: center;
  padding: 24px;
  color: var(--el-text-color-placeholder);
  font-size: 13px;
}
.var-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: 300px;
  overflow-y: auto;
}
.var-row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 6px;
  background: var(--el-fill-color-lighter);
  border-radius: 6px;
}
.var-name {
  font-family: monospace;
  font-size: 13px;
  font-weight: 600;
  color: var(--el-color-primary);
  min-width: 80px;
  max-width: 120px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.var-eq {
  font-family: monospace;
  color: var(--el-text-color-secondary);
}
</style>