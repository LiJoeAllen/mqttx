<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import type { MqttConnection, AliyunPreset } from '../../types/mqtt'
import { defaultConnection, defaultAliyunPreset, generateAliyunConnection } from '../../types/mqtt'
import { mqttTestConnection, mqttReadDebugLog } from '../../stores/useMqttBridge'
import { useConnections } from '../../stores/useConnections'
import { useAliyunPresets } from '../../stores/useAliyunPresets'
import { toConnectionDto } from '../../types/mqtt'

const { getGroups: getConnectionGroups } = useConnections()
const { presets, add: addPreset, update: updatePreset, remove: removePreset, getGroups: getPresetGroups, renameGroup: renamePresetGroup, deleteGroup: deletePresetGroup, exportPresets, importPresets } = useAliyunPresets()

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
const importInput = ref<HTMLInputElement | null>(null)

// ─── 连接分组 ──────────────────────────────────────────────────────────────

const connectionGroupOptions = computed(() => {
  const existing = getConnectionGroups()
  const opts = [{ label: '未分组', value: '' }]
  for (const g of existing) opts.push({ label: g, value: g })
  return opts
})

// ─── 阿里云预设 ─────────────────────────────────────────────────────────────

type CloudPreset = '' | 'aliyun'
const cloudPreset = ref<CloudPreset>('')
const selectedPresetId = ref<string>('')
const aliyunPreset = ref<AliyunPreset | null>(null)

// 预设分组
const presetGroupFilter = ref<string | null>(null)
const presetGroups = computed(() => getPresetGroups())

const filteredPresets = computed(() => {
  if (presetGroupFilter.value === null) return presets.value
  if (presetGroupFilter.value === '__ungrouped__') {
    return presets.value.filter((p) => !p.group)
  }
  return presets.value.filter((p) => p.group === presetGroupFilter.value)
})

const presetGroupCounts = computed(() => {
  const map: Record<string, number> = {}
  for (const p of presets.value) {
    const key = p.group || '__ungrouped__'
    map[key] = (map[key] || 0) + 1
  }
  return map
})

/** 预设内容签名，用于判断用户是否真的修改过（避免把空白/未改动的预设写进库） */
function presetSignature(p: AliyunPreset): string {
  return JSON.stringify([
    p.name, p.group, p.instanceId, p.authMode, p.accessKeyId,
    p.accessKeySecret, p.groupId, p.deviceId,
    p.deviceAccessKeyId, p.deviceAccessKeySecret,
  ])
}
let presetBaseline = ''

function markPresetBaseline() {
  if (aliyunPreset.value) presetBaseline = presetSignature(aliyunPreset.value)
}

function handleSelectPreset(id: string) {
  const found = id ? presets.value.find((p) => p.id === id) : undefined
  if (found) {
    selectedPresetId.value = id
    aliyunPreset.value = { ...found }
  } else {
    // 清空选择 → 空白预设，字段保持可编辑
    selectedPresetId.value = ''
    aliyunPreset.value = defaultAliyunPreset()
  }
  markPresetBaseline()
  applyAliyunPreset()
}

function handleNewPreset() {
  // 不强制先命名：直接给空白预设，填入有效内容后自动入库
  selectedPresetId.value = ''
  aliyunPreset.value = {
    ...defaultAliyunPreset(),
    group: presetGroupFilter.value && presetGroupFilter.value !== '__ungrouped__' ? presetGroupFilter.value : '',
  }
  markPresetBaseline()
}

async function handleDeletePreset() {
  if (!selectedPresetId.value) return
  try {
    await ElMessageBox.confirm(`确定删除预设 "${aliyunPreset.value?.name || '未命名'}"？`, '确认', { type: 'warning' })
  } catch {
    return
  }
  removePreset(selectedPresetId.value)
  selectedPresetId.value = ''
  aliyunPreset.value = defaultAliyunPreset()
  markPresetBaseline()
  ElMessage.success('已删除')
}

async function handleRenamePresetGroup(group: string) {
  const { value } = await ElMessageBox.prompt('重命名预设分组', '重命名', {
    inputValue: group,
    inputPattern: /\S+/,
    inputErrorMessage: '名称不能为空',
  }).catch(() => ({ value: undefined }))
  if (value && value !== group) {
    renamePresetGroup(group, value)
    if (presetGroupFilter.value === group) presetGroupFilter.value = value
  }
}

async function handleDeletePresetGroup(group: string) {
  const count = presetGroupCounts.value[group] || 0
  try {
    await ElMessageBox.confirm(`删除分组 "${group}"？组内 ${count} 个预设将变为未分组`, '确认', { type: 'warning' })
  } catch {
    return
  }
  deletePresetGroup(group)
  if (presetGroupFilter.value === group) presetGroupFilter.value = null
}

// ─── 连接监听 ──────────────────────────────────────────────────────────────

watch(
  () => props.connection,
  (conn) => {
    if (conn) {
      form.value = { ...conn }
      if (conn.host.includes('.mqtt.aliyuncs.com') && conn.username.startsWith('Signature|')) {
        cloudPreset.value = 'aliyun'
        parseAliyunFromConnection(conn)
      } else {
        cloudPreset.value = ''
        aliyunPreset.value = null
        selectedPresetId.value = ''
      }
    } else {
      form.value = defaultConnection()
      cloudPreset.value = ''
      aliyunPreset.value = null
      selectedPresetId.value = ''
    }
  },
  { immediate: true },
)

function parseAliyunFromConnection(conn: MqttConnection) {
  const hostMatch = conn.host.match(/^(.+)\.mqtt\.aliyuncs\.com$/)
  const userMatch = conn.username.match(/^Signature\|(.+)\|(.+)$/)
  const clientMatch = conn.clientId.match(/^(.+)@@@(.+)$/)

  // 尝试匹配已有预设
  const match = presets.value.find((p) =>
    p.instanceId === (hostMatch?.[1] ?? '') &&
    p.accessKeyId === (userMatch?.[1] ?? '') &&
    p.groupId === (clientMatch?.[1] ?? ''),
  )

  if (match) {
    selectedPresetId.value = match.id
    aliyunPreset.value = { ...match }
  } else {
    aliyunPreset.value = {
      ...defaultAliyunPreset(),
      name: '',
      instanceId: hostMatch?.[1] ?? '',
      accessKeyId: userMatch?.[1] ?? '',
      groupId: clientMatch?.[1] ?? '',
      deviceId: clientMatch?.[2] ?? '',
    }
  }
  markPresetBaseline()
}

async function handlePresetChange(val: CloudPreset | undefined) {
  if (val === 'aliyun') {
    if (!aliyunPreset.value) {
      aliyunPreset.value = defaultAliyunPreset()
      markPresetBaseline()
    }
    return
  }
  // 退出阿里云模式：已生成过连接参数时先确认，保留名称和分组
  if (form.value.host.endsWith('.mqtt.aliyuncs.com')) {
    try {
      await ElMessageBox.confirm('退出阿里云模式将清空已生成的连接参数（保留名称和分组）', '提示', { type: 'warning' })
    } catch {
      cloudPreset.value = 'aliyun'
      return
    }
  }
  const { name, group } = form.value
  form.value = { ...defaultConnection(), id: form.value.id, name, group }
  aliyunPreset.value = null
  selectedPresetId.value = ''
}

async function applyAliyunPreset() {
  const p = aliyunPreset.value
  if (!p || !p.instanceId || !p.accessKeyId || !p.accessKeySecret || !p.groupId) return

  const generated = await generateAliyunConnection(p)

  // 用户手动改过连接名时不覆盖（自动名以“阿里云 ”开头）
  const autoName = p.name || `阿里云 ${p.groupId}`
  const keepName = form.value.name !== '' && form.value.name !== autoName && !form.value.name.startsWith('阿里云 ')

  form.value = {
    ...form.value,
    ...generated,
    port: form.value.ssl ? 8883 : 1883,
    name: keepName ? form.value.name : autoName,
  }
}

function persistPresetIfChanged() {
  const p = aliyunPreset.value
  if (!p || cloudPreset.value !== 'aliyun') return
  if (presetSignature(p) === presetBaseline) return
  presetBaseline = presetSignature(p)
  if (selectedPresetId.value) {
    updatePreset(selectedPresetId.value, p)
  } else if (p.name || p.instanceId || p.groupId || p.accessKeyId) {
    // 空白预设填入有效内容后才入库
    const added = addPreset({ ...p })
    selectedPresetId.value = added.id
  }
}

// 预设内容变化：去抖后自动入库 + 重新生成连接参数
let persistTimer: ReturnType<typeof setTimeout> | undefined
watch(aliyunPreset, () => {
  if (cloudPreset.value !== 'aliyun' || !aliyunPreset.value) return
  if (persistTimer) clearTimeout(persistTimer)
  persistTimer = setTimeout(() => {
    persistTimer = undefined
    persistPresetIfChanged()
    applyAliyunPreset()
  }, 500)
}, { deep: true })

async function flushPendingPreset() {
  if (!persistTimer) return
  clearTimeout(persistTimer)
  persistTimer = undefined
  persistPresetIfChanged()
  await applyAliyunPreset()
}

// 阿里云模式下切换 TLS 时端口跟随（1883 / 8883）
watch(() => form.value.ssl, (ssl) => {
  if (cloudPreset.value === 'aliyun' && form.value.host.endsWith('.mqtt.aliyuncs.com')) {
    form.value.port = ssl ? 8883 : 1883
  }
})

// ─── 预设导入导出 ──────────────────────────────────────────────────────────

function handleExportPresets() {
  const json = exportPresets()
  const blob = new Blob([json], { type: 'application/json' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = `mqttx_presets_${new Date().toISOString().slice(0, 10)}.json`
  a.click()
  URL.revokeObjectURL(url)
  ElMessage.success(`已导出 ${presets.value.length} 个预设（含明文密钥，请妥善保管）`)
}

function handleImportPresetsClick() {
  importInput.value?.click()
}

async function handleImportPresetsFile(event: Event) {
  const file = (event.target as HTMLInputElement).files?.[0]
  if (!file) return
  try {
    const text = await file.text()
    const { imported, skipped } = importPresets(text)
    if (imported === 0 && skipped > 0) {
      ElMessage.warning(`全部 ${skipped} 个预设与现有内容重复，未导入`)
    } else {
      ElMessage.success(skipped > 0 ? `导入 ${imported} 个预设，跳过 ${skipped} 个重复` : `成功导入 ${imported} 个预设`)
    }
  } catch (e) {
    ElMessage.error(`导入失败: ${e}`)
  } finally {
    if (importInput.value) importInput.value.value = ''
  }
}

// ─── 其他 ──────────────────────────────────────────────────────────────────

function generateClientId() {
  form.value.clientId = `mqttx_${Math.random().toString(36).slice(2, 10)}`
}

async function handleSave() {
  await flushPendingPreset()
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
  await flushPendingPreset()
  if (!form.value.host.trim()) {
    ElMessage.warning('请输入主机地址')
    return
  }
  testing.value = true
  try {
    const msg = await mqttTestConnection(toConnectionDto(form.value))
    ElMessage.success(msg)
  } catch (e: unknown) {
    ElMessage.error(e instanceof Error ? e.message : String(e))
  } finally {
    testing.value = false
  }
}

async function handleViewDebugLog() {
  try {
    const log = await mqttReadDebugLog()
    ElMessageBox.alert(
      log || '暂无调试日志',
      'MQTT 调试日志',
      { type: 'info', customStyle: { whiteSpace: 'pre-wrap', maxWidth: '800px' }, confirmButtonText: '关闭' },
    )
  } catch (e: unknown) {
    ElMessage.error('读取日志失败: ' + (e instanceof Error ? e.message : String(e)))
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
      <!-- 云厂商预设 -->
      <el-form-item label="云厂商预设">
        <el-select
          v-model="cloudPreset"
          placeholder="无"
          clearable
          style="width: 100%"
          @change="handlePresetChange"
        >
          <el-option value="aliyun" label="阿里云 MQTT" />
        </el-select>
      </el-form-item>

      <!-- 阿里云预设配置 -->
      <template v-if="cloudPreset === 'aliyun'">
        <div class="section-title">
          <span>阿里云 MQTT 配置</span>
          <div class="section-actions">
            <span class="autosave-hint">修改自动保存</span>
            <el-button size="small" @click="handleImportPresetsClick">导入预设</el-button>
            <el-button size="small" :disabled="presets.length === 0" @click="handleExportPresets">导出预设</el-button>
          </div>
          <input ref="importInput" type="file" accept=".json" style="display: none" @change="handleImportPresetsFile" />
        </div>

        <!-- 预设分组筛选 -->
        <div v-if="presetGroups.length > 0" class="preset-group-tabs">
          <span
            :class="['preset-tab', presetGroupFilter === null && 'active']"
            @click="presetGroupFilter = null"
          >全部</span>
          <span
            :class="['preset-tab', presetGroupFilter === '__ungrouped__' && 'active']"
            @click="presetGroupFilter = '__ungrouped__'"
          >未分组</span>
          <span
            v-for="g in presetGroups"
            :key="g"
            :class="['preset-tab', presetGroupFilter === g && 'active']"
            @click="presetGroupFilter = g"
          >
            {{ g }}
            <el-dropdown trigger="click" @command="(cmd: string) => cmd === 'rename' ? handleRenamePresetGroup(g) : handleDeletePresetGroup(g)">
              <span class="preset-tab-menu" @click.stop>&#8230;</span>
              <template #dropdown>
                <el-dropdown-menu>
                  <el-dropdown-item command="rename">重命名</el-dropdown-item>
                  <el-dropdown-item command="delete" divided>删除</el-dropdown-item>
                </el-dropdown-menu>
              </template>
            </el-dropdown>
          </span>
        </div>

        <!-- 预设选择 -->
        <el-row :gutter="8">
          <el-col :span="16">
            <el-form-item label="选择预设">
              <el-select
                :model-value="selectedPresetId"
                placeholder="选择已保存的预设"
                clearable
                style="width: 100%"
                @change="(val: string) => handleSelectPreset(val)"
              >
                <el-option
                  v-for="p in filteredPresets"
                  :key="p.id"
                  :value="p.id"
                  :label="p.name || `${p.instanceId} / ${p.groupId}`"
                />
              </el-select>
            </el-form-item>
          </el-col>
          <el-col :span="8" style="display: flex; align-items: flex-end; gap: 4px; padding-bottom: 18px;">
            <el-button size="small" @click="handleNewPreset">新建</el-button>
            <el-button size="small" type="danger" :disabled="!selectedPresetId" @click="handleDeletePreset">删除</el-button>
          </el-col>
        </el-row>

        <el-form-item v-if="aliyunPreset" label="预设名称（可选）">
          <el-input v-model="aliyunPreset.name" placeholder="如：生产环境-门禁设备" />
        </el-form-item>

        <template v-if="aliyunPreset">
          <el-form-item v-if="presetGroups.length > 0 || aliyunPreset.group" label="预设分组">
            <el-select v-model="aliyunPreset.group" placeholder="未分组" clearable style="width: 100%">
              <el-option v-for="g in presetGroups" :key="g" :label="g" :value="g" />
            </el-select>
          </el-form-item>

          <el-row :gutter="12">
            <el-col :span="12">
              <el-form-item label="实例 ID">
                <el-input v-model="aliyunPreset.instanceId" placeholder="post-cn-xxx" />
              </el-form-item>
            </el-col>
            <el-col :span="12">
              <el-form-item label="Group ID">
                <el-input v-model="aliyunPreset.groupId" placeholder="GID-prod" />
              </el-form-item>
            </el-col>
          </el-row>
          <el-row :gutter="12">
            <el-col :span="12">
              <el-form-item label="Access Key ID">
                <el-input v-model="aliyunPreset.accessKeyId" placeholder="LTAI4xxx" />
              </el-form-item>
            </el-col>
            <el-col :span="12">
              <el-form-item label="Access Key Secret">
                <el-input
                  v-model="aliyunPreset.accessKeySecret"
                  type="password"
                  show-password
                  placeholder="K24iRxxx"
                />
              </el-form-item>
            </el-col>
          </el-row>
          <el-form-item label="设备 ID">
            <el-input v-model="aliyunPreset.deviceId" placeholder="device_xxx">
              <template #append>
                <el-button @click="aliyunPreset!.deviceId = `device_${Math.random().toString(36).slice(2, 10)}`">
                  重新生成
                </el-button>
              </template>
            </el-input>
          </el-form-item>
        </template>

        <el-divider border-style="dashed" />
      </template>

      <el-form-item label="名称">
        <el-input v-model="form.name" placeholder="给连接起个名字" />
      </el-form-item>
      <el-form-item label="分组">
        <el-select v-model="form.group" placeholder="未分组" clearable style="width: 100%">
          <el-option v-for="opt in connectionGroupOptions" :key="opt.value" :label="opt.label" :value="opt.value" />
        </el-select>
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
            <el-input-number v-model="form.port" :min="1" :max="65535" controls-position="right" style="width: 100%" />
          </el-form-item>
        </el-col>
      </el-row>
      <el-row :gutter="12">
        <el-col :span="12">
          <el-form-item label="用户名">
            <el-input v-model="form.username" placeholder="可选" />
          </el-form-item>
        </el-col>
        <el-col :span="12">
          <el-form-item label="密码">
            <el-input v-model="form.password" type="password" show-password placeholder="可选" />
          </el-form-item>
        </el-col>
      </el-row>
      <el-row :gutter="12">
        <el-col :span="16">
          <el-form-item label="Client ID">
            <el-input v-model="form.clientId" placeholder="客户端 ID">
              <template #append>
                <el-button @click="generateClientId">随机生成</el-button>
              </template>
            </el-input>
          </el-form-item>
        </el-col>
        <el-col :span="8">
          <el-form-item label="协议版本">
            <el-select v-model="form.protocolVersion" style="width: 100%">
              <el-option value="5.0" label="MQTT 5.0" />
              <el-option value="3.1.1" label="MQTT 3.1.1" />
            </el-select>
          </el-form-item>
        </el-col>
      </el-row>

      <!-- MQTT 选项 -->
      <div class="section-title">
        {{ form.protocolVersion === '5.0' ? 'MQTT v5 选项' : 'MQTT 选项' }}
      </div>
      <el-row :gutter="12">
        <el-col :span="8">
          <el-form-item :label="form.protocolVersion === '5.0' ? 'Clean Start' : 'Clean Session'">
            <el-switch v-model="form.cleanStart" />
          </el-form-item>
        </el-col>
        <el-col v-if="form.protocolVersion === '5.0'" :span="8">
          <el-form-item label="Session Expiry">
            <el-input-number v-model="form.sessionExpiryInterval" :min="0" :value-on-clear="null" style="width: 100%" />
          </el-form-item>
        </el-col>
        <el-col :span="8">
          <el-form-item label="Keep Alive">
            <el-input-number v-model="form.keepAlive" :min="0" :max="65535" :value-on-clear="null" style="width: 100%" />
          </el-form-item>
        </el-col>
      </el-row>
      <el-row v-if="form.protocolVersion === '5.0'" :gutter="12">
        <el-col :span="8">
          <el-form-item label="Receive Maximum">
            <el-input-number v-model="form.receiveMaximum" :min="0" :value-on-clear="null" style="width: 100%" />
          </el-form-item>
        </el-col>
        <el-col :span="8">
          <el-form-item label="Max Packet Size">
            <el-input-number v-model="form.maximumPacketSize" :min="0" :value-on-clear="null" style="width: 100%" />
          </el-form-item>
        </el-col>
        <el-col :span="8">
          <el-form-item label="Topic Alias Max">
            <el-input-number v-model="form.topicAliasMaximum" :min="0" :value-on-clear="null" style="width: 100%" />
          </el-form-item>
        </el-col>
      </el-row>
    </el-form>
    <template #footer>
      <div style="display: flex; justify-content: space-between; width: 100%;">
        <el-button type="info" link @click="handleViewDebugLog">查看调试日志</el-button>
        <div>
          <el-button @click="handleCancel">取消</el-button>
          <el-button :loading="testing" :disabled="testing" @click="handleTestConnection">测试连接</el-button>
          <el-button type="primary" @click="handleSave">保存</el-button>
        </div>
      </div>
    </template>
  </el-dialog>
</template>

<style scoped>
.section-title {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-weight: 600;
  font-size: 13px;
  color: var(--el-text-color-primary);
  margin-bottom: 10px;
  padding-top: 6px;
  border-top: 1px solid var(--el-border-color-lighter);
}
.section-actions { display: flex; align-items: center; gap: 4px; font-weight: normal; }
.autosave-hint { font-size: 12px; color: var(--el-text-color-secondary); margin-right: 4px; }

.preset-group-tabs {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  margin-bottom: 8px;
}
.preset-tab {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding: 3px 8px;
  border-radius: 4px;
  cursor: pointer;
  font-size: 12px;
  background: var(--el-fill-color-light);
}
.preset-tab:hover { background: var(--el-fill-color); }
.preset-tab.active { background: var(--el-color-primary-light-9); color: var(--el-color-primary); font-weight: 500; }
.preset-tab-menu { opacity: 0.5; font-size: 14px; }
.preset-tab-menu:hover { opacity: 1; }
</style>
