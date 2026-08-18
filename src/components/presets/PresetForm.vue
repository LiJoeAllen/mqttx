<script setup lang="ts">
import { ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import type { Preset } from '../../types/mqtt'
import { defaultPreset } from '../../types/mqtt'
import UserPropertiesEditor from '../dashboard/UserPropertiesEditor.vue'

const props = defineProps<{
  modelValue: boolean
  preset: Preset | null
}>()

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
  save: [data: Preset]
}>()

const form = ref<Preset>(defaultPreset())

watch(
  () => props.preset,
  (preset) => {
    if (preset) {
      form.value = { ...preset, userProperties: JSON.parse(JSON.stringify(preset.userProperties)) }
    } else {
      form.value = defaultPreset()
    }
  },
  { immediate: true },
)

function handleSave() {
  if (!form.value.name.trim()) {
    ElMessage.warning('请输入预设名称')
    return
  }
  if (!form.value.topic.trim()) {
    ElMessage.warning('请输入主题')
    return
  }
  emit('save', { ...form.value })
  emit('update:modelValue', false)
}

function handleCancel() {
  emit('update:modelValue', false)
}
</script>

<template>
  <el-dialog
    :model-value="modelValue"
    :title="preset ? '编辑预设' : '新增预设'"
    width="550px"
    @update:model-value="(val: boolean) => emit('update:modelValue', val)"
  >
    <el-form
      :model="form"
      label-position="top"
      size="small"
    >
      <el-form-item label="名称">
        <el-input v-model="form.name" placeholder="温度上报" />
      </el-form-item>
      <el-form-item label="主题">
        <el-input v-model="form.topic" placeholder="sensor/{{deviceId}}/temp" />
        <div class="form-hint">
          支持 <code v-pre>{{变量名}}</code> 占位符，发送时自动替换
        </div>
      </el-form-item>
      <el-form-item label="Payload 模板">
        <el-input
          v-model="form.payloadTemplate"
          type="textarea"
          :rows="4"
          placeholder='{"temp":{{temp}},"device":"{{deviceId}}"}'
        />
        <div class="form-hint">
          使用 <code v-pre>{{变量名}}</code> 作为占位符，发送时自动替换
        </div>
      </el-form-item>
      <el-row :gutter="12">
        <el-col :span="12">
          <el-form-item label="QoS">
            <el-select v-model="form.qos">
              <el-option :value="0" label="QoS 0 (至多一次)" />
              <el-option :value="1" label="QoS 1 (至少一次)" />
              <el-option :value="2" label="QoS 2 (正好一次)" />
            </el-select>
          </el-form-item>
        </el-col>
        <el-col :span="12" style="padding-top: 24px">
          <el-form-item>
            <el-checkbox v-model="form.retain">Retain</el-checkbox>
          </el-form-item>
        </el-col>
      </el-row>
      <el-divider border-style="dashed" />
      <div class="section-title">MQTT v5 User Properties (模板)</div>
      <UserPropertiesEditor v-model="form.userProperties" />
    </el-form>
    <template #footer>
      <el-button @click="handleCancel">取消</el-button>
      <el-button type="primary" @click="handleSave">保存</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.form-hint {
  font-size: 12px;
  color: var(--el-text-color-secondary);
  margin-top: 4px;
}
.form-hint code {
  background: var(--el-fill-color-light);
  padding: 0 4px;
  border-radius: 2px;
  font-family: monospace;
}
.section-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--el-text-color-primary);
  margin-bottom: 4px;
}
</style>