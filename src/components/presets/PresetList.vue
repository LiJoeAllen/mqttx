<script setup lang="ts">
import { ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import type { Preset } from '../../types/mqtt'
import { usePresets } from '../../stores/usePresets'
import PresetForm from './PresetForm.vue'

// eslint-disable-next-line @typescript-eslint/no-unused-vars
const _PresetForm = PresetForm

const { presets, add, update, remove } = usePresets()

const showForm = ref(false)
const editingPreset = ref<Preset | null>(null)

function handleAdd() {
  editingPreset.value = null
  showForm.value = true
}

function handleEdit(preset: Preset) {
  editingPreset.value = preset
  showForm.value = true
}

function handleSave(data: Preset) {
  if (editingPreset.value) {
    update(editingPreset.value.id, data)
    ElMessage.success('预设已更新')
  } else {
    const newPreset = { ...data, id: crypto.randomUUID() }
    add()
    const idx = presets.value.length - 1
    presets.value[idx] = { ...presets.value[idx], ...newPreset }
    ElMessage.success('预设已创建')
  }
  showForm.value = false
}

async function handleDelete(preset: Preset) {
  try {
    await ElMessageBox.confirm(`确定删除预设 "${preset.name}"？`, '确认', {
      type: 'warning',
    })
    remove(preset.id)
    ElMessage.success('已删除')
  } catch {
    // cancelled
  }
}

const qosLabel = (qos: number) => `QoS ${qos}`
</script>

<template>
  <div class="preset-list">
    <div class="list-header">
      <h3>预设管理</h3>
      <el-button type="primary" size="small" @click="handleAdd">
        + 新增预设
      </el-button>
    </div>

    <el-table :data="presets" style="width: 100%" stripe>
      <el-table-column prop="name" label="名称" min-width="120" />
      <el-table-column prop="topic" label="主题" min-width="200" />
      <el-table-column label="QoS" width="80">
        <template #default="{ row }">
          <el-tag size="small">{{ qosLabel(row.qos) }}</el-tag>
        </template>
      </el-table-column>
      <el-table-column label="Retain" width="70">
        <template #default="{ row }">
          <el-tag v-if="row.retain" size="small" type="warning">R</el-tag>
        </template>
      </el-table-column>
      <el-table-column label="Payload 预览" min-width="200">
        <template #default="{ row }">
          <span class="payload-preview">{{ row.payloadTemplate.substring(0, 50) }}{{ row.payloadTemplate.length > 50 ? '...' : '' }}</span>
        </template>
      </el-table-column>
      <el-table-column label="操作" width="160" fixed="right">
        <template #default="{ row }">
          <el-button size="small" @click="handleEdit(row)">编辑</el-button>
          <el-button size="small" type="danger" @click="handleDelete(row)">删除</el-button>
        </template>
      </el-table-column>
    </el-table>

    <div v-if="presets.length === 0" class="empty-hint">
      暂无预设，点击上方按钮创建
    </div>

    <PresetForm
      v-model="showForm"
      :preset="editingPreset"
      @save="handleSave"
    />
  </div>
</template>

<style scoped>
.preset-list {
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
.payload-preview {
  font-family: monospace;
  font-size: 12px;
  color: var(--el-text-color-secondary);
}
</style>