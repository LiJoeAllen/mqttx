<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import type { MqttConnection, Preset, PublishDto } from '../../types/mqtt'
import { extractVariables, resolveTemplate, resolveUserProperties } from '../../utils/envVarResolver'
import { mqttPublish } from '../../stores/useMqttBridge'
import { useGlobalVariables } from '../../stores/useGlobalVariables'
import { prettyJson } from '../../utils/format'
import { ElMessage } from 'element-plus'
import HoverPreview from '../common/HoverPreview.vue'
import VariableEditor from './VariableEditor.vue'
import UserPropertiesEditor from './UserPropertiesEditor.vue'
import GlobalVariableManager from './GlobalVariableManager.vue'

const props = defineProps<{
  connections: MqttConnection[]
  presets: Preset[]
}>()

const emit = defineEmits<{
  sent: [message: PublishDto]
}>()

const { variables: globalVars, setVariable: setGlobalVar, importFromBindings } = useGlobalVariables()

// ─── State ─────────────────────────────────────────────────────

const selectedConnectionId = ref('')
const selectedPresetId = ref('')
const topic = ref('')
const payload = ref('')
const qos = ref<0 | 1 | 2>(0)
const retain = ref(false)
const showVariables = ref(false)
const showV5Props = ref(false)
const variableBindings = ref<Record<string, string>>({})
const userProperties = ref<{ key: string; value: string }[]>([])
const contentType = ref('')
const messageExpiryInterval = ref<number | null>(null)
const responseTopic = ref('')
const correlationData = ref('')
const sending = ref(false)
const showGlobalVarMgr = ref(false)

// ─── Computed ──────────────────────────────────────────────────

const connectedConnections = computed(() =>
  props.connections.filter((c) => c.status === 'connected'),
)

const selectedPreset = computed(() =>
  props.presets.find((p) => p.id === selectedPresetId.value),
)

const presetVariables = computed(() => {
  if (!selectedPreset.value) return []
  const vars = extractVariables(selectedPreset.value.payloadTemplate)
  for (const v of extractVariables(selectedPreset.value.topic)) {
    if (!vars.includes(v)) vars.push(v)
  }
  for (const up of selectedPreset.value.userProperties) {
    for (const v of extractVariables(up.key)) if (!vars.includes(v)) vars.push(v)
    for (const v of extractVariables(up.value)) if (!vars.includes(v)) vars.push(v)
  }
  return vars
})

// ─── Watch preset ──────────────────────────────────────────────

watch(selectedPreset, (preset) => {
  if (preset) {
    topic.value = preset.topic
    payload.value = preset.payloadTemplate
    qos.value = preset.qos
    retain.value = preset.retain
    userProperties.value = JSON.parse(JSON.stringify(preset.userProperties))
    const newBindings: Record<string, string> = {}
    const presetVars = extractVariables(preset.payloadTemplate)
    const topicVars = extractVariables(preset.topic)
    const allVars = [...new Set([...presetVars, ...topicVars])]
    for (const v of allVars) {
      // Auto-fill from global variables first, then keep existing
      newBindings[v] = globalVars.value[v] ?? variableBindings.value[v] ?? ''
    }
    variableBindings.value = newBindings
    showVariables.value = allVars.length > 0
  }
})

// ─── Send ──────────────────────────────────────────────────────

async function send() {
  if (!selectedConnectionId.value) {
    ElMessage.warning('请先选择一个连接')
    return
  }
  if (!topic.value.trim()) {
    ElMessage.warning('请输入主题')
    return
  }
  if (!payload.value.trim()) {
    ElMessage.warning('请输入消息内容')
    return
  }

  const resolvedTopic = resolveTemplate(topic.value, variableBindings.value)
  const resolvedPayload = resolveTemplate(payload.value, variableBindings.value)
  const resolvedProps = resolveUserProperties(userProperties.value, variableBindings.value)

  // Save current bindings to global variables
  importFromBindings(variableBindings.value)

  const publishDto: PublishDto = {
    connection_id: selectedConnectionId.value,
    topic: resolvedTopic,
    payload: resolvedPayload,
    qos: qos.value,
    retain: retain.value,
    user_properties: resolvedProps,
    content_type: contentType.value || null,
    message_expiry_interval: messageExpiryInterval.value,
    response_topic: responseTopic.value || null,
    correlation_data: correlationData.value || null,
  }

  sending.value = true
  try {
    await mqttPublish(publishDto)
    ElMessage.success('消息已发送')
    emit('sent', publishDto)
  } catch (e: any) {
    ElMessage.error(`发送失败: ${e}`)
  } finally {
    sending.value = false
  }
}

// ─── Save as global variable when a binding value changes ──────

function onBindingChange(name: string, value: string) {
  variableBindings.value = { ...variableBindings.value, [name]: value }
  // Auto-save to global if value is non-empty
  if (value.trim()) {
    setGlobalVar(name, value)
  }
}
</script>

<template>
  <div class="send-panel">
    <div class="panel-header">
      <span class="panel-title">发送消息</span>
      <div class="panel-actions">
        <el-select
          v-model="selectedPresetId"
          placeholder="选择预设..."
          size="small"
          style="width: 160px"
          clearable
        >
          <el-option
            v-for="p in presets"
            :key="p.id"
            :label="p.name"
            :value="p.id"
          />
        </el-select>
      </div>
    </div>

    <div class="panel-body">
      <!-- Row 1: Connection + Topic -->
      <div class="form-row">
        <div class="field" style="flex: 0 0 160px">
          <label>连接</label>
          <el-select
            v-model="selectedConnectionId"
            placeholder="选择连接"
            size="small"
            style="width: 100%"
          >
            <el-option
              v-for="c in connectedConnections"
              :key="c.id"
              :label="c.name"
              :value="c.id"
            />
          </el-select>
        </div>
        <div class="field" style="flex: 1">
          <label>主题</label>
          <el-input
            v-model="topic"
            size="small"
            placeholder="sensor/temp"
          />
        </div>
        <div class="field" style="flex: 0 0 90px">
          <label>QoS</label>
          <el-select v-model="qos" size="small">
            <el-option :value="0" label="QoS 0" />
            <el-option :value="1" label="QoS 1" />
            <el-option :value="2" label="QoS 2" />
          </el-select>
        </div>
        <div class="field" style="flex: 0 0 70px; align-items: center; justify-content: flex-end; padding-bottom: 0;">
          <el-checkbox v-model="retain" size="small" label="Retain" />
        </div>
      </div>

      <!-- Row 2: Payload -->
      <div class="form-row">
        <div class="field" style="flex: 1">
          <label>Payload</label>
          <HoverPreview :text="prettyJson(payload)" :delay="600">
            <el-input
              v-model="payload"
              type="textarea"
              :rows="2"
              placeholder='{"temp":{{temp}},"device":"{{deviceId}}"}'
            />
          </HoverPreview>
        </div>
      </div>

      <!-- Toggle buttons -->
      <div class="toggle-row">
        <el-button
          size="small"
          :class="['toggle-btn', { active: showVariables }]"
          @click="showVariables = !showVariables"
        >
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M4 7V4h16v3"/><path d="M9 20h6"/><path d="M12 4v16"/></svg>
          变量
          <span v-if="presetVariables.length" class="toggle-badge">{{ presetVariables.length }}</span>
        </el-button>
        <el-button
          size="small"
          :class="['toggle-btn', { active: showV5Props }]"
          @click="showV5Props = !showV5Props"
        >
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/></svg>
          v5 属性
        </el-button>
        <el-button
          size="small"
          class="toggle-btn"
          @click="showGlobalVarMgr = true"
        >
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="3"/><path d="M12 1v2M12 21v2M4.22 4.22l1.42 1.42M18.36 18.36l1.42 1.42M1 12h2M21 12h2M4.22 19.78l1.42-1.42M18.36 5.64l1.42-1.42"/></svg>
          全局变量
        </el-button>
        <div class="toggle-spacer" />
        <el-button
          type="primary"
          size="small"
          :loading="sending"
          :disabled="!selectedConnectionId || !topic.trim()"
          @click="send"
          class="send-btn"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="22" y1="2" x2="11" y2="13"/><polygon points="22 2 15 22 11 13 2 9 22 2"/></svg>
          发送
        </el-button>
      </div>

      <!-- Variable editor (now uses onBindingChange to auto-save) -->
      <transition name="slide">
        <div v-if="showVariables" class="expand-section">
          <div class="var-editor-header">
            <span class="var-editor-title">变量值</span>
            <span class="var-editor-hint">输入后自动保存为全局变量</span>
          </div>
          <div class="var-grid">
            <div
              v-for="v in presetVariables"
              :key="v"
              class="var-item"
            >
              <span class="var-label">{{ v }}</span>
              <el-input
                :model-value="variableBindings[v] ?? ''"
                size="small"
                placeholder="输入值..."
                @input="(val: string) => onBindingChange(v, val)"
              />
            </div>
          </div>
        </div>
      </transition>

      <!-- v5 Props -->
      <transition name="slide">
        <div v-if="showV5Props" class="expand-section">
          <div class="v5-grid">
            <div class="field">
              <label>Content Type</label>
              <el-input v-model="contentType" size="small" placeholder="json" />
            </div>
            <div class="field">
              <label>消息过期(秒)</label>
              <el-input-number
                v-model="messageExpiryInterval"
                :min="0"
                size="small"
                style="width: 100%"
              />
            </div>
            <div class="field">
              <label>Response Topic</label>
              <el-input v-model="responseTopic" size="small" placeholder="sensor/response" />
            </div>
            <div class="field">
              <label>Correlation Data</label>
              <el-input v-model="correlationData" size="small" placeholder="hex string" />
            </div>
          </div>
          <UserPropertiesEditor v-model="userProperties" />
        </div>
      </transition>
    </div>

    <!-- Global Variable Manager Dialog -->
    <GlobalVariableManager v-model:show="showGlobalVarMgr" />
  </div>
</template>

<style scoped>
.send-panel {
  border: 1px solid var(--el-border-color);
  border-radius: 8px;
  background: var(--el-bg-color);
  flex-shrink: 0;
}
.panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 12px;
  border-bottom: 1px solid var(--el-border-color-light);
}
.panel-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--el-text-color-primary);
}
.panel-actions {
  display: flex;
  align-items: center;
  gap: 6px;
}
.panel-body {
  padding: 8px 12px 10px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

/* ─── Form Row ───────────────────────────────────────────────── */
.form-row {
  display: flex;
  gap: 8px;
  align-items: flex-start;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.field label {
  font-size: 11px;
  color: var(--el-text-color-secondary);
  font-weight: 500;
}

/* ─── Toggle Row ─────────────────────────────────────────────── */
.toggle-row {
  display: flex;
  align-items: center;
  gap: 4px;
}
.toggle-btn {
  --el-button-bg-color: transparent;
  --el-button-border-color: var(--el-border-color);
  --el-button-hover-bg-color: var(--el-fill-color);
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
  color: var(--el-text-color-secondary);
  padding: 0 8px !important;
  height: 26px;
  border-radius: 6px;
}
.toggle-btn.active {
  --el-button-bg-color: var(--el-color-primary-light-9);
  --el-button-border-color: var(--el-color-primary-light-5);
  color: var(--el-color-primary);
}
.toggle-btn svg {
  flex-shrink: 0;
}
.toggle-badge {
  font-size: 10px;
  background: var(--el-color-primary);
  color: var(--comfort-text, var(--el-color-white));
  padding: 0 4px;
  border-radius: 6px;
  line-height: 14px;
  min-width: 14px;
  text-align: center;
}
.toggle-spacer {
  flex: 1;
}
.send-btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  height: 28px;
  padding: 0 14px !important;
  border-radius: 6px;
}
.send-btn svg {
  flex-shrink: 0;
}

/* ─── Variable Editor (inline) ───────────────────────────────── */
.expand-section {
  padding: 8px;
  background: var(--el-fill-color-light);
  border-radius: 6px;
  border: 1px solid var(--el-border-color-extra-light);
}
.var-editor-header {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 6px;
}
.var-editor-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--el-text-color-primary);
}
.var-editor-hint {
  font-size: 11px;
  color: var(--el-text-color-placeholder);
}
.var-grid {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.var-item {
  display: flex;
  align-items: center;
  gap: 8px;
}
.var-label {
  min-width: 80px;
  font-family: monospace;
  font-size: 13px;
  font-weight: 600;
  color: var(--el-color-primary);
}

/* ─── v5 Props ───────────────────────────────────────────────── */
.v5-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 6px;
  margin-bottom: 6px;
}

/* ─── Transition ─────────────────────────────────────────────── */
.slide-enter-active,
.slide-leave-active {
  transition: all 0.2s ease;
  overflow: hidden;
}
.slide-enter-from,
.slide-leave-to {
  opacity: 0;
  max-height: 0;
  padding-top: 0;
  padding-bottom: 0;
  margin-bottom: 0;
  border-width: 0;
}
.slide-enter-to,
.slide-leave-from {
  opacity: 1;
  max-height: 500px;
}
</style>