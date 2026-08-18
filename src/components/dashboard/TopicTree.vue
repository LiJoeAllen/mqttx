<script setup lang="ts">
import { ref, computed } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import type { MqttMessage } from '../../types/mqtt'
import type { Subscription } from '../../types/mqtt'
import { useSubscriptions } from '../../stores/useSubscriptions'
import { mqttSubscribe, mqttUnsubscribe } from '../../stores/useMqttBridge'

interface TopicNode {
  id: string
  label: string
  fullPath: string
  count: number
  isSubscribed: boolean
  subscriptionQos: number
  children: TopicNode[]
}

const props = defineProps<{
  messages: MqttMessage[]
  selectedTopic: string
  connectionId: string
  connected: boolean
}>()

const emit = defineEmits<{
  'update:selectedTopic': [value: string]
  'select': [topic: string]
}>()

const { addSubscription, removeSubscription, getSubscriptionsByConnection } = useSubscriptions()

const subscribeInput = ref('')
const subscribeQos = ref<0 | 1 | 2>(0)
const subscribing = ref(false)

// Build tree from both subscriptions and messages
const treeData = computed(() => {
  const root: TopicNode[] = []
  const nodeMap = new Map<string, TopicNode>()

  function getOrCreateNode(path: string, label: string): TopicNode {
    let node = nodeMap.get(path)
    if (!node) {
      node = {
        id: path,
        label,
        fullPath: path,
        count: 0,
        isSubscribed: false,
        subscriptionQos: 0,
        children: [],
      }
      nodeMap.set(path, node)
    }
    return node
  }

  function addToTree(fullPath: string) {
    const segments = fullPath.split('/')
    let currentPath = ''
    let parentList = root
    for (let i = 0; i < segments.length; i++) {
      const seg = segments[i]
      currentPath = currentPath ? `${currentPath}/${seg}` : seg
      let node = parentList.find((n) => n.id === currentPath)
      if (!node) {
        node = getOrCreateNode(currentPath, seg)
        parentList.push(node)
      }
      parentList = node.children
    }
  }

  const subs = getSubscriptionsByConnection(props.connectionId)
  for (const sub of subs) {
    addToTree(sub.topic)
    const node = nodeMap.get(sub.topic)
    if (node) { node.isSubscribed = true; node.subscriptionQos = sub.qos }
  }
  for (const msg of props.messages) {
    addToTree(msg.topic)
  }
  for (const msg of props.messages) {
    const node = nodeMap.get(msg.topic)
    if (node) node.count++
  }

  function sortTree(nodes: TopicNode[]) {
    nodes.sort((a, b) => {
      const aIsFolder = a.children.length > 0
      const bIsFolder = b.children.length > 0
      if (aIsFolder !== bIsFolder) return aIsFolder ? -1 : 1
      return a.label.localeCompare(b.label)
    })
    for (const n of nodes) sortTree(n.children)
  }
  sortTree(root)
  return root
})

function handleNodeClick(node: TopicNode) {
  if (props.selectedTopic === node.fullPath) {
    emit('update:selectedTopic', '')
    emit('select', '')
  } else {
    emit('update:selectedTopic', node.fullPath)
    emit('select', node.fullPath)
  }
}

async function handleSubscribe() {
  const topic = subscribeInput.value.trim()
  if (!topic) { ElMessage.warning('请输入订阅主题'); return }
  if (!props.connectionId) { ElMessage.warning('请先选择一个连接'); return }
  subscribing.value = true
  try {
    await mqttSubscribe({ connection_id: props.connectionId, topic, qos: subscribeQos.value })
    addSubscription(props.connectionId, topic, subscribeQos.value)
    ElMessage.success(`已订阅: ${topic}`)
    subscribeInput.value = ''
  } catch (e: unknown) {
    ElMessage.error(`订阅失败: ${e instanceof Error ? e.message : String(e)}`)
  } finally {
    subscribing.value = false
  }
}

async function handleUnsubscribe(topic: string) {
  if (!props.connectionId) return
  try {
    await ElMessageBox.confirm(`确定取消订阅 "${topic}"？`, '取消订阅', {
      type: 'warning', confirmButtonText: '确定', cancelButtonText: '取消',
    })
    await mqttUnsubscribe(props.connectionId, topic)
    removeSubscription(props.connectionId, topic)
    ElMessage.success(`已取消订阅: ${topic}`)
  } catch { /* cancelled */ }
}

function getNodeIcon(node: TopicNode): string {
  if (node.isSubscribed) return '✅'
  if (node.children.length > 0) return '📁'
  return '📄'
}

function getNodeClass(node: TopicNode): string {
  if (node.fullPath === props.selectedTopic) return 'selected'
  if (node.isSubscribed) return 'subscribed'
  return ''
}
</script>

<template>
  <div class="topic-tree">
    <div class="tree-header">
      <span class="tree-title">主题树</span>
      <span class="tree-count">{{ treeData.length }}</span>
    </div>

    <div class="subscribe-row">
      <el-input
        v-model="subscribeInput"
        size="small"
        :placeholder="connectionId ? (connected ? '输入主题...' : '连接未就绪') : '先选择连接'"
        :disabled="!connectionId || !connected"
        @keyup.enter="handleSubscribe"
      >
        <template #prefix>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="color: var(--el-text-color-placeholder)"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>
        </template>
        <template #append>
          <el-select v-model="subscribeQos" size="small" style="width: 62px" :disabled="!connectionId || !connected">
            <el-option :value="0" label="Q0" />
            <el-option :value="1" label="Q1" />
            <el-option :value="2" label="Q2" />
          </el-select>
        </template>
      </el-input>
      <el-button
        size="small"
        type="primary"
        :loading="subscribing"
        :disabled="!connectionId || !connected || !subscribeInput.trim()"
        @click="handleSubscribe"
        class="sub-btn"
      >+</el-button>
    </div>

    <div v-if="treeData.length === 0" class="empty-state">
      <div class="empty-text">暂无主题</div>
      <div class="empty-sub">输入上方搜索框订阅</div>
    </div>

    <div class="tree-list">
      <template v-for="node in treeData" :key="node.id">
        <div :class="['tree-node', getNodeClass(node)]" @click="handleNodeClick(node)">
          <span class="node-icon">{{ getNodeIcon(node) }}</span>
          <span class="node-label">{{ node.label }}</span>
          <span class="node-count">{{ node.count }}</span>
          <span v-if="node.isSubscribed" class="node-unsub" title="取消订阅" @click.stop="handleUnsubscribe(node.fullPath)">✕</span>
        </div>
        <div v-if="node.children.length > 0" class="children-group">
          <template v-for="child in node.children" :key="child.id">
            <div :class="['tree-node child', getNodeClass(child)]" @click="handleNodeClick(child)">
              <span class="node-icon">{{ getNodeIcon(child) }}</span>
              <span class="node-label">{{ child.label }}</span>
              <span class="node-count">{{ child.count }}</span>
              <span v-if="child.isSubscribed" class="node-unsub" @click.stop="handleUnsubscribe(child.fullPath)">✕</span>
            </div>
            <div v-if="child.children.length > 0" class="children-group">
              <div
                v-for="gc in child.children"
                :key="gc.id"
                :class="['tree-node child', { selected: gc.fullPath === selectedTopic, subscribed: gc.isSubscribed }]"
                @click="handleNodeClick(gc)"
              >
                <span class="node-icon">{{ getNodeIcon(gc) }}</span>
                <span class="node-label">{{ gc.label }}</span>
                <span class="node-count">{{ gc.count }}</span>
                <span v-if="gc.isSubscribed" class="node-unsub" @click.stop="handleUnsubscribe(gc.fullPath)">✕</span>
              </div>
            </div>
          </template>
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped>
.topic-tree {
  width: 240px;
  min-width: 200px;
  border: 1px solid var(--el-border-color);
  border-radius: 8px;
  background: var(--el-bg-color);
  display: flex;
  flex-direction: column;
  font-size: 13px;
  overflow: hidden;
}
.tree-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 10px;
  border-bottom: 1px solid var(--el-border-color-light);
  flex-shrink: 0;
}
.tree-title {
  font-size: 13px;
  font-weight: 600;
}
.tree-count {
  font-size: 10px;
  color: var(--el-text-color-secondary);
  background: var(--el-fill-color);
  padding: 0 6px;
  border-radius: 6px;
  line-height: 18px;
}
.subscribe-row {
  display: flex;
  gap: 4px;
  padding: 6px 8px;
  border-bottom: 1px solid var(--el-border-color-light);
  flex-shrink: 0;
}
.sub-btn {
  flex-shrink: 0;
  width: 28px;
  padding: 0 !important;
  font-size: 16px;
  font-weight: 700;
  border-radius: 6px;
}
.empty-state {
  padding: 24px 16px;
  text-align: center;
  color: var(--el-text-color-placeholder);
}
.empty-text { font-size: 13px; font-weight: 500; }
.empty-sub { font-size: 11px; margin-top: 2px; opacity: 0.7; }
.tree-list {
  flex: 1;
  overflow-y: auto;
  padding: 2px 0;
}
.tree-node {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 3px 8px;
  cursor: pointer;
  border-radius: 0;
  transition: background 0.1s;
  user-select: none;
  margin: 0 2px;
  border-radius: 4px;
}
.tree-node:hover { background: var(--el-fill-color-light); }
.tree-node.selected { background: var(--el-color-primary-light-9) !important; color: var(--el-color-primary); font-weight: 600; }
.tree-node.subscribed { color: var(--el-color-success); }
.tree-node.child { padding-left: 20px; }
.node-icon { font-size: 12px; flex-shrink: 0; }
.node-label { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.node-count { font-size: 11px; color: var(--el-text-color-secondary); background: var(--el-fill-color); padding: 0 5px; border-radius: 6px; min-width: 16px; text-align: center; flex-shrink: 0; line-height: 16px; }
.node-unsub { font-size: 10px; color: var(--el-color-danger); cursor: pointer; padding: 0 2px; opacity: 0; transition: opacity 0.12s; flex-shrink: 0; }
.tree-node:hover .node-unsub { opacity: 1; }
.children-group { margin-left: 8px; border-left: 1px solid var(--el-border-color-extra-light); }
</style>