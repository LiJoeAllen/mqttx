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
const activePresetId = ref<string | null>(null)
const showVariables = ref(false)
const showV5Props = ref(false)
const showGlobalVarMgr = ref(false)
const sending = ref(false)

// Per-preset editor state (keyed by preset id)
const editorStates = ref<Record<string, {
  topic: string
  payload: string
  qos: 0 | 1 | 2
  retain: boolean
  variableBindings: Record<string, string>
  userProperties: { key: string; value: string }[]
  contentType: string
  messageExpiryInterval: number | null
  responseTopic: string
  correlationData: string
}>>({})

// ─── Computed ──────────────────────────────────────────────────

const connectedConnections = computed(() =>
  props.connections.filter((c) => c.status === 'connected'),
)

const activePreset = computed(() =>
  props.presets.find((p) => p.id === activePresetId.value),
)

const activeState = computed(() =>
  activePresetId.value ? editorStates.value[activePresetId.value] : null,
)

const presetVariables = computed(() => {
  if (!activePreset.value) return []
  const vars = extractVariables(activePreset.value.payloadTemplate)
  for (const v of extractVariables(activePreset.value.topic)) {
    if (!vars.includes(v)) vars.push(v)
  }
  for (const up of activePreset.value.userProperties) {
    for (const v of extractVariables(up.key)) if (!vars.includes(v)) vars.push(v)
    for (const v of extractVariables(up.value)) if (!vars.includes(v)) vars.push(v)
  }
  return vars
})

// ─── Init editor state for a preset ────────────────────────────

function initEditorState(preset: Preset) {
  if (editorStates.value[preset.id]) return

  const allVars = [...new Set([
    ...extractVariables(preset.payloadTemplate),
    ...extractVariables(preset.topic),
    ...preset.userProperties.flatMap(up => [
      ...extractVariables(up.key),
      ...extractVariables(up.value),
    ]),
  ])]

  const bindings: Record<string, string> = {}
  for (const v of allVars) {
    bindings[v] = globalVars.value[v] ?? ''
  }

  editorStates.value[preset.id] = {
    topic: preset.topic,
    payload: preset.payloadTemplate,
    qos: preset.qos,
    retain: preset.retain,
    variableBindings: bindings,
    userProperties: JSON.parse(JSON.stringify(preset.userProperties)),
    contentType: '',
    messageExpiryInterval: null,
    responseTopic: '',
    correlationData: '',
  }
}

// ─── Switch active preset ──────────────────────────────────────

function selectPreset(presetId: string) {
  const preset = props.presets.find((p) => p.id === presetId)
  if (!preset) return
  initEditorState(preset)
  activePresetId.value = presetId
  showVariables.value = presetVariables.value.length > 0
}

// Auto-select first preset when presets change
watch(() => props.presets, (ps) => {
  if (ps.length > 0) {
    // Init all presets
    for (const p of ps) initEditorState(p)
    // Select first if none selected
    if (!activePresetId.value || !ps.find((p) => p.id === activePresetId.value)) {
      selectPreset(ps[0].id)
    }
  } else {
    activePresetId.value = null
  }
}, { immediate: true, deep: true })

// ─── Send ──────────────────────────────────────────────────────

function getPublishDto(): PublishDto | null {
  if (!activePreset.value || !activeState.value) return null
  const st = activeState.value
  const resolvedTopic = resolveTemplate(st.topic, st.variableBindings)
  const resolvedPayload = resolveTemplate(st.payload, st.variableBindings)
  const resolvedProps = resolveUserProperties(st.userProperties, st.variableBindings)

  return {
    connection_id: selectedConnectionId.value,
    topic: resolvedTopic,
    payload: resolvedPayload,
    qos: st.qos,
    retain: st.retain,
    user_properties: resolvedProps,
    content_type: st.contentType || null,
    message_expiry_interval: st.messageExpiryInterval,
    response_topic: st.responseTopic || null,
    correlation_data: st.correlationData || null,
  }
}

async function send() {
  if (!selectedConnectionId.value) {
    ElMessage.warning('请先选择一个连接')
    return
  }
  if (!activePreset.value || !activeState.value) {
    ElMessage.warning('请选择一个预设')
    return
  }
  importFromBindings(activeState.value.variableBindings)
  sending.value = true
  try {
    const dto = getPublishDto()
    if (!dto) { ElMessage.warning('预设数据异常'); return }
    if (!dto.topic.trim()) { ElMessage.warning('主题为空'); return }
    if (!dto.payload.trim()) { ElMessage.warning('Payload 为空'); return }
    await mqttPublish(dto)
    emit('sent', dto)
    ElMessage.success(`"${activePreset.value.name}" 已发送`)
  } catch (e: any) {
    ElMessage.error(`发送失败: ${e}`)
  } finally {
    sending.value = false
  }
}

// ─── Variable binding changed ──────────────────────────────────

function onBindingChange(name: string, value: string) {
  if (!activePresetId.value || !activeState.value) return
  activeState.value.variableBindings = { ...activeState.value.variableBindings, [name]: value }
  if (value.trim()) {
    setGlobalVar(name, value)
  }
}

// ─── Close tab ─────────────────────────────────────────────────

function closePreset(id: string) {
  // Keep the editor state but deselect
  if (activePresetId.value === id) {
    const idx = props.presets.findIndex((p) => p.id === id)
    const next = props.presets[idx + 1] ?? props.presets[idx - 1] ?? null
    activePresetId.value = next?.id ?? null
  }
}
</script>

<template>
  <div class="send-panel">
    <!-- Connection selector -->
    <div class="conn-bar">
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

    <!-- Preset tabs -->
    <div v-if="presets.length > 0" class="preset-tabs">
      <button
        v-for="p in presets"
        :key="p.id"
        :class="['preset-tab', { active: activePresetId === p.id }]"
        @click="selectPreset(p.id)"
      >
        <span class="pt-name">{{ p.name }}</span>
        <span class="pt-topic">{{ p.topic }}</span>
        <span class="pt-qos">Q{{ p.qos }}</span>
        <span
          v-if="activePresetId === p.id && presets.length > 1"
          class="pt-close"
          @click.stop="closePreset(p.id)"
        >×</span>
      </button>
    </div>
    <div v-else class="no-presets">
      暂无预设，请先在「预设」页面创建
    </div>

    <!-- Editor area for active preset -->
    <div v-if="activePreset && activeState" class="editor-area">
      <!-- Row: Topic + QoS + Retain -->
      <div class="form-row">
        <div class="field" style="flex: 1">
          <label>主题</label>
          <el-input v-model="activeState.topic" size="small" placeholder="sensor/temp" />
        </div>
        <div class="field" style="flex: 0 0 90px">
          <label>QoS</label>
          <el-select v-model="activeState.qos" size="small">
            <el-option :value="0" label="QoS 0" />
            <el-option :value="1" label="QoS 1" />
            <el-option :value="2" label="QoS 2" />
          </el-select>
        </div>
        <div class="field" style="flex: 0 0 70px; align-items: center; justify-content: flex-end; padding-bottom: 0;">
          <el-checkbox v-model="activeState.retain" size="small" label="Retain" />
        </div>
      </div>

      <!-- Row: Payload -->
      <div class="form-row">
        <div class="field" style="flex: 1">
          <label>Payload</label>
          <HoverPreview :text="prettyJson(activeState.payload)" :delay="600">
            <el-input
              v-model="activeState.payload"
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
          :disabled="!selectedConnectionId || !activeState"
          @click="send"
          class="send-btn"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="22" y1="2" x2="11" y2="13"/><polygon points="22 2 15 22 11 13 2 9 22 2"/></svg>
          发送 {{ activePreset?.name }}
        </el-button>
      </div>

      <!-- Variable editor -->
      <transition name="slide">
        <div v-if="showVariables" class="expand-section">
          <div class="var-editor-header">
            <span class="var-editor-title">变量值</span>
            <span class="var-editor-hint">输入后自动保存为全局变量，每个预设独立维护</span>
          </div>
          <div class="var-grid">
            <div
              v-for="v in presetVariables"
              :key="v"
              class="var-item"
            >
              <span class="var-label">{{ v }}</span>
              <el-input
                :model-value="activeState.variableBindings[v] ?? ''"
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
              <el-input v-model="activeState.contentType" size="small" placeholder="json" />
            </div>
            <div class="field">
              <label>消息过期(秒)</label>
              <el-input-number
                v-model="activeState.messageExpiryInterval"
                :min="0"
                size="small"
                style="width: 100%"
              />
            </div>
            <div class="field">
              <label>Response Topic</label>
              <el-input v-model="activeState.responseTopic" size="small" placeholder="sensor/response" />
            </div>
            <div class="field">
              <label>Correlation Data</label>
              <el-input v-model="activeState.correlationData" size="small" placeholder="hex string" />
            </div>
          </div>
          <UserPropertiesEditor v-model="activeState.userProperties" />
        </div>
      </transition>
    </div>

    <!-- Empty state -->
    <div v-else-if="presets.length > 0" class="empty-state">
      <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" style="color: var(--comfort-text-muted)"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>
      <span class="empty-text">点击上方标签页开始编辑</span>
    </div>
  </div>

  <!-- Global Variable Manager Dialog -->
  <GlobalVariableManager v-model:show="showGlobalVarMgr" />
</template>

<style scoped>
.send-panel {
  border: 1px solid var(--comfort-border, var(--el-border-color));
  border-radius: 8px;
  background: var(--comfort-bg-card, var(--el-bg-color));
  flex-shrink: 0;
  overflow: hidden;
}

/* ─── Connection bar ─────────────────────────────────────────── */
.conn-bar {
  padding: 8px 12px;
  border-bottom: 1px solid var(--comfort-border-light, var(--el-border-color-light));
}

/* ─── Preset Tabs ────────────────────────────────────────────── */
.preset-tabs {
  display: flex;
  gap: 2px;
  padding: 6px 8px 0;
  overflow-x: auto;
  background: var(--comfort-bg-soft, var(--el-fill-color-light));
  border-bottom: 1px solid var(--comfort-border-light, var(--el-border-color-light));
  scrollbar-width: thin;
}
.preset-tab {
  display: flex;
  align-items: center;
  gap: 5px;
  padding: 4px 10px;
  border: 1px solid transparent;
  border-bottom: none;
  border-radius: 6px 6px 0 0;
  background: transparent;
  cursor: pointer;
  font-size: 12px;
  font-family: inherit;
  color: var(--comfort-text-secondary, var(--el-text-color-secondary));
  white-space: nowrap;
  flex-shrink: 0;
  transition: all 0.15s;
  position: relative;
  margin-bottom: -1px;
}
.preset-tab:hover {
  color: var(--comfort-text, var(--el-text-color-primary));
  background: var(--comfort-bg-card, var(--el-bg-color));
}
.preset-tab.active {
  color: var(--comfort-primary, var(--el-color-primary));
  background: var(--comfort-bg-card, var(--el-bg-color));
  border-color: var(--comfort-border-light, var(--el-border-color-light));
  font-weight: 600;
}
.pt-name {
  max-width: 100px;
  overflow: hidden;
  text-overflow: ellipsis;
}
.pt-topic {
  font-family: var(--comfort-font-mono, monospace);
  font-size: 10px;
  color: var(--comfort-text-muted, var(--el-text-color-placeholder));
  max-width: 120px;
  overflow: hidden;
  text-overflow: ellipsis;
}
.pt-qos {
  font-size: 10px;
  color: var(--comfort-text-muted, var(--el-text-color-placeholder));
  flex-shrink: 0;
}
.pt-close {
  font-size: 14px;
  line-height: 1;
  color: var(--comfort-text-muted, var(--el-text-color-placeholder));
  margin-left: 2px;
  flex-shrink: 0;
  width: 14px;
  height: 14px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 3px;
}
.pt-close:hover {
  color: var(--comfort-danger, var(--el-color-danger));
  background: var(--el-color-danger-light-9);
}
.no-presets {
  padding: 12px;
  text-align: center;
  font-size: 12px;
  color: var(--comfort-text-muted, var(--el-text-color-placeholder));
}

/* ─── Editor area ────────────────────────────────────────────── */
.editor-area {
  padding: 8px 12px 10px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
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

/* ─── Empty state ────────────────────────────────────────────── */
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 24px;
}
.empty-text {
  font-size: 12px;
  color: var(--comfort-text-muted, var(--el-text-color-placeholder));
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

/* ─── Variable Editor ────────────────────────────────────────── */
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