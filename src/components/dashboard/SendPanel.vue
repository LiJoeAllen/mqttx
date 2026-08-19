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
const selectedPresetIds = ref<string[]>([])
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
const batchMode = ref(false)
const batchProgress = ref<{ presetName: string; status: 'pending' | 'sending' | 'success' | 'error'; message?: string }[]>([])

// ─── Computed ──────────────────────────────────────────────────

const connectedConnections = computed(() =>
  props.connections.filter((c) => c.status === 'connected'),
)

const selectedPresets = computed(() =>
  props.presets.filter((p) => selectedPresetIds.value.includes(p.id)),
)

const selectedPreset = computed(() =>
  props.presets.find((p) => p.id === selectedPresetIds.value[selectedPresetIds.value.length - 1]),
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
  if (preset && !batchMode.value) {
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
      newBindings[v] = globalVars.value[v] ?? variableBindings.value[v] ?? ''
    }
    variableBindings.value = newBindings
    showVariables.value = allVars.length > 0
  }
})

// ─── Send Single ───────────────────────────────────────────────

function getPublishDto(preset?: Preset): PublishDto {
  const t = preset?.topic ?? topic.value
  const p = preset?.payloadTemplate ?? payload.value
  const q = preset?.qos ?? qos.value
  const r = preset?.retain ?? retain.value
  const up = preset ? JSON.parse(JSON.stringify(preset.userProperties)) : userProperties.value

  const resolvedTopic = resolveTemplate(t, variableBindings.value)
  const resolvedPayload = resolveTemplate(p, variableBindings.value)
  const resolvedProps = resolveUserProperties(up, variableBindings.value)

  return {
    connection_id: selectedConnectionId.value,
    topic: resolvedTopic,
    payload: resolvedPayload,
    qos: q,
    retain: r,
    user_properties: resolvedProps,
    content_type: contentType.value || null,
    message_expiry_interval: messageExpiryInterval.value,
    response_topic: responseTopic.value || null,
    correlation_data: correlationData.value || null,
  }
}

async function sendSingle(preset?: Preset) {
  if (!selectedConnectionId.value) {
    ElMessage.warning('请先选择一个连接')
    return false
  }
  const dto = getPublishDto(preset)
  if (!dto.topic.trim()) { ElMessage.warning('请输入主题'); return false }
  if (!dto.payload.trim()) { ElMessage.warning('请输入消息内容'); return false }

  try {
    await mqttPublish(dto)
    emit('sent', dto)
    return true
  } catch (e: any) {
    ElMessage.error(`发送失败: ${e}`)
    return false
  }
}

async function send() {
  importFromBindings(variableBindings.value)
  sending.value = true
  if (batchMode.value && selectedPresets.value.length > 0) {
    // Batch send all selected presets
    batchProgress.value = selectedPresets.value.map((p) => ({
      presetName: p.name,
      status: 'pending' as const,
    }))
    for (let i = 0; i < selectedPresets.value.length; i++) {
      const preset = selectedPresets.value[i]
      batchProgress.value[i].status = 'sending'
      try {
        const dto = getPublishDto(preset)
        if (!dto.topic.trim()) {
          batchProgress.value[i].status = 'error'
          batchProgress.value[i].message = '主题为空'
          continue
        }
        if (!dto.payload.trim()) {
          batchProgress.value[i].status = 'error'
          batchProgress.value[i].message = 'Payload 为空'
          continue
        }
        await mqttPublish(dto)
        emit('sent', dto)
        batchProgress.value[i].status = 'success'
      } catch (e: any) {
        batchProgress.value[i].status = 'error'
        batchProgress.value[i].message = String(e)
      }
    }
    const successCount = batchProgress.value.filter((p) => p.status === 'success').length
    const errorCount = batchProgress.value.filter((p) => p.status === 'error').length
    ElMessage.success(`批量发送完成: ${successCount} 成功, ${errorCount} 失败`)
  } else {
    // Single send
    const ok = await sendSingle()
    if (ok) ElMessage.success('消息已发送')
  }
  sending.value = false
}

// ─── Save as global variable when a binding value changes ──────

function onBindingChange(name: string, value: string) {
  variableBindings.value = { ...variableBindings.value, [name]: value }
  if (value.trim()) {
    setGlobalVar(name, value)
  }
}

function toggleBatchMode() {
  batchMode.value = !batchMode.value
  if (!batchMode.value) {
    selectedPresetIds.value = []
    batchProgress.value = []
  }
}
</script>

<template>
  <div class="send-panel">
    <div class="panel-header">
      <span class="panel-title">发送消息</span>
      <div class="panel-actions">
        <el-button
          size="small"
          :class="['batch-toggle', { active: batchMode }]"
          @click="toggleBatchMode"
        >
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M23 21v-2a4 4 0 0 0-3-3.87"/><path d="M16 3.13a4 4 0 0 1 0 7.75"/></svg>
          {{ batchMode ? '批量模式' : '单条模式' }}
        </el-button>
      </div>
    </div>

    <div class="panel-body">
      <!-- Row 1: Connection + Preset -->
      <div class="form-row">
        <div class="field" style="flex: 0 0 150px">
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
          <label>预设</label>
          <el-select
            v-model="selectedPresetIds"
            :multiple="batchMode"
            :collapse-tags="batchMode"
            :collapse-tags-tooltip="batchMode"
            placeholder="选择预设..."
            size="small"
            style="width: 100%"
            :max-collapse-tags="3"
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

      <!-- Batch mode: show selected preset list -->
      <div v-if="batchMode && selectedPresets.length > 0" class="batch-preset-list">
        <div
          v-for="(p, idx) in selectedPresets"
          :key="p.id"
          class="batch-preset-item"
        >
          <span class="bp-name">{{ p.name }}</span>
          <span class="bp-topic">{{ p.topic }}</span>
          <span class="bp-qos">Q{{ p.qos }}</span>
          <span v-if="p.retain" class="bp-retain">R</span>
          <span class="bp-status-icon">
            <svg v-if="batchProgress[idx]?.status === 'success'" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="var(--comfort-success)" stroke-width="2"><polyline points="20 6 9 17 4 12"/></svg>
            <svg v-else-if="batchProgress[idx]?.status === 'error'" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="var(--comfort-danger)" stroke-width="2" :title="batchProgress[idx]?.message"><circle cx="12" cy="12" r="10"/><line x1="15" y1="9" x2="9" y2="15"/><line x1="9" y1="9" x2="15" y2="15"/></svg>
            <svg v-else-if="batchProgress[idx]?.status === 'sending'" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="var(--comfort-text-secondary)" stroke-width="2" class="spinner"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>
          </span>
        </div>
      </div>

      <!-- Row 2: Topic (single mode) -->
      <div v-if="!batchMode" class="form-row">
        <div class="field" style="flex: 1">
          <label>主题</label>
          <el-input v-model="topic" size="small" placeholder="sensor/temp" />
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

      <!-- Row 3: Payload (single mode) -->
      <div v-if="!batchMode" class="form-row">
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
          :disabled="!selectedConnectionId || (batchMode ? selectedPresets.length === 0 : !topic.trim())"
          @click="send"
          class="send-btn"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="22" y1="2" x2="11" y2="13"/><polygon points="22 2 15 22 11 13 2 9 22 2"/></svg>
          {{ batchMode ? `发送 ${selectedPresets.length} 个预设` : '发送' }}
        </el-button>
      </div>

      <!-- Variable editor -->
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
  border: 1px solid var(--comfort-border, var(--el-border-color));
  border-radius: 8px;
  background: var(--comfort-bg-card, var(--el-bg-color));
  flex-shrink: 0;
}
.panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 12px;
  border-bottom: 1px solid var(--comfort-border-light, var(--el-border-color-light));
}
.panel-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--comfort-text, var(--el-text-color-primary));
}
.panel-actions {
  display: flex;
  align-items: center;
  gap: 6px;
}
.batch-toggle {
  --el-button-bg-color: transparent;
  --el-button-border-color: var(--comfort-border, var(--el-border-color));
  --el-button-hover-bg-color: var(--el-fill-color);
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  color: var(--comfort-text-secondary, var(--el-text-color-secondary));
  padding: 0 8px !important;
  height: 24px;
  border-radius: 6px;
}
.batch-toggle.active {
  --el-button-bg-color: var(--el-color-primary-light-9);
  --el-button-border-color: var(--el-color-primary-light-5);
  color: var(--el-color-primary);
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
  color: var(--comfort-text-secondary, var(--el-text-color-secondary));
  font-weight: 500;
}

/* ─── Batch preset list ──────────────────────────────────────── */
.batch-preset-list {
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding: 6px 8px;
  background: var(--comfort-bg-soft, var(--el-fill-color-light));
  border-radius: 6px;
  max-height: 120px;
  overflow-y: auto;
}
.batch-preset-item {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 3px 6px;
  background: var(--comfort-bg-card, var(--el-bg-color));
  border-radius: 4px;
  font-size: 12px;
}
.bp-name {
  font-weight: 600;
  color: var(--comfort-text, var(--el-text-color-primary));
  min-width: 60px;
  max-width: 100px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.bp-topic {
  flex: 1;
  font-family: var(--comfort-font-mono, monospace);
  font-size: 11px;
  color: var(--comfort-text-secondary, var(--el-text-color-secondary));
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.bp-qos {
  font-size: 10px;
  color: var(--comfort-text-muted, var(--el-text-color-placeholder));
  flex-shrink: 0;
}
.bp-retain {
  font-size: 10px;
  color: var(--comfort-warning, var(--el-color-warning));
  flex-shrink: 0;
}
.bp-status-icon {
  flex-shrink: 0;
  width: 14px;
  display: flex;
  align-items: center;
  justify-content: center;
}
@keyframes spin {
  to { transform: rotate(360deg); }
}
.spinner {
  animation: spin 1s linear infinite;
}

/* ─── Toggle Row ─────────────────────────────────────────────── */
.toggle-row {
  display: flex;
  align-items: center;
  gap: 4px;
}
.toggle-btn {
  --el-button-bg-color: transparent;
  --el-button-border-color: var(--comfort-border, var(--el-border-color));
  --el-button-hover-bg-color: var(--el-fill-color);
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
  color: var(--comfort-text-secondary, var(--el-text-color-secondary));
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
  background: var(--comfort-bg-soft, var(--el-fill-color-light));
  border-radius: 6px;
  border: 1px solid var(--comfort-border-light, var(--el-border-color-extra-light));
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
  color: var(--comfort-text, var(--el-text-color-primary));
}
.var-editor-hint {
  font-size: 11px;
  color: var(--comfort-text-muted, var(--el-text-color-placeholder));
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
  font-family: var(--comfort-font-mono, monospace);
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