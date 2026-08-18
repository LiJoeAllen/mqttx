<script setup lang="ts">
import { ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import type { MqttConnection } from '../../types/mqtt'
import { defaultConnection } from '../../types/mqtt'
import { mqttTestConnection } from '../../stores/useMqttBridge'
import { toConnectionDto } from '../../types/mqtt'

const props = defineProps<{
  modelValue: boolean
  connection: MqttConnection | null
}>()

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
  save: [data: MqttConnection]
}>()

const form = ref<MqttConnection>(defaultConnection())
const testing = ref(false)

watch(
  () => props.connection,
  (conn) => {
    if (conn) {
      form.value = { ...conn }
    } else {
      form.value = defaultConnection()
    }
  },
  { immediate: true },
)

function generateClientId() {
  form.value.clientId = `mqttx_${Math.random().toString(36).slice(2, 10)}`
}

function handleSave() {
  if (!form.value.name.trim()) {
    ElMessage.warning('请输入连接名称')
    return
  }
  if (!form.value.host.trim()) {
    ElMessage.warning('请输入主机地址')
    return
  }
  if (!form.value.clientId.trim()) {
    ElMessage.warning('请输入 Client ID')
    return
  }
  emit('save', { ...form.value })
  emit('update:modelValue', false)
}

function handleCancel() {
  emit('update:modelValue', false)
}

async function handleTestConnection() {
  if (!form.value.host.trim()) {
    ElMessage.warning('请输入主机地址')
    return
  }
  testing.value = true
  try {
    const msg = await mqttTestConnection(toConnectionDto(form.value))
    ElMessage.success(msg)
  } catch (e: unknown) {
    const err = e instanceof Error ? e.message : String(e)
    ElMessage.error(err)
  } finally {
    testing.value = false
  }
}
</script>

<template>
  <el-dialog
    :model-value="modelValue"
    :title="connection ? '编辑连接' : '新增连接'"
    width="600px"
    @update:model-value="(val: boolean) => emit('update:modelValue', val)"
  >
    <el-form
      ref="formRef"
      :model="form"
      label-width="110px"
      label-position="top"
      size="small"
    >
      <el-form-item label="名称">
        <el-input v-model="form.name" placeholder="给连接起个名字" />
      </el-form-item>
      <el-row :gutter="12">
        <el-col :span="6">
          <el-form-item label="协议">
            <el-select v-model="form.ssl" :placeholder="false">
              <el-option :value="false" label="mqtt://" />
              <el-option :value="true" label="mqtts://" />
            </el-select>
          </el-form-item>
        </el-col>
        <el-col :span="12">
          <el-form-item label="主机">
            <el-input v-model="form.host" placeholder="localhost" />
          </el-form-item>
        </el-col>
        <el-col :span="6">
          <el-form-item label="端口">
            <el-input-number
              v-model="form.port"
              :min="1"
              :max="65535"
              style="width: 100%"
            />
          </el-form-item>
        </el-col>
      </el-row>
      <el-row :gutter="12">
        <el-col :span="12">
          <el-form-item label="用户名">
            <el-input v-model="form.username" placeholder="(可选)" />
          </el-form-item>
        </el-col>
        <el-col :span="12">
          <el-form-item label="密码">
            <el-input
              v-model="form.password"
              type="password"
              show-password
              placeholder="(可选)"
            />
          </el-form-item>
        </el-col>
      </el-row>
      <el-form-item label="Client ID">
        <el-input v-model="form.clientId" placeholder="自动生成即可">
          <template #append>
            <el-button @click="generateClientId">重新生成</el-button>
          </template>
        </el-input>
      </el-form-item>
      <el-divider border-style="dashed" />
      <div class="section-title">MQTT v5 选项</div>
      <el-row :gutter="12">
        <el-col :span="8">
          <el-form-item label="Clean Start">
            <el-switch v-model="form.cleanStart" />
          </el-form-item>
        </el-col>
        <el-col :span="8">
          <el-form-item label="Session Expiry">
            <el-input-number
              v-model="form.sessionExpiryInterval"
              :min="0"
              style="width: 100%"
            />
            <span class="unit">秒</span>
          </el-form-item>
        </el-col>
        <el-col :span="8">
          <el-form-item label="Keep Alive">
            <el-input-number
              v-model="form.keepAlive"
              :min="10"
              :max="3600"
              style="width: 100%"
            />
            <span class="unit">秒</span>
          </el-form-item>
        </el-col>
      </el-row>
      <el-row :gutter="12">
        <el-col :span="8">
          <el-form-item label="Receive Max">
            <el-input-number
              v-model="form.receiveMaximum"
              :min="0"
              :value-on-clear="null"
              style="width: 100%"
            />
          </el-form-item>
        </el-col>
        <el-col :span="8">
          <el-form-item label="Max Packet Size">
            <el-input-number
              v-model="form.maximumPacketSize"
              :min="0"
              :value-on-clear="null"
              style="width: 100%"
            />
          </el-form-item>
        </el-col>
        <el-col :span="8">
          <el-form-item label="Topic Alias Max">
            <el-input-number
              v-model="form.topicAliasMaximum"
              :min="0"
              :value-on-clear="null"
              style="width: 100%"
            />
          </el-form-item>
        </el-col>
      </el-row>
    </el-form>
    <template #footer>
      <el-button @click="handleCancel">取消</el-button>
      <el-button
        :loading="testing"
        :disabled="testing"
        @click="handleTestConnection"
      >
        测试连接
      </el-button>
      <el-button type="primary" @click="handleSave">保存</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.section-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--el-text-color-primary);
  margin-bottom: 8px;
}
.unit {
  font-size: 12px;
  color: var(--el-text-color-secondary);
  margin-left: 4px;
}
</style>