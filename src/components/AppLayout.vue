<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useMqttBridge } from '../stores/useMqttBridge'
import ConnectionList from './connections/ConnectionList.vue'
import PresetList from './presets/PresetList.vue'
import DashboardView from './dashboard/DashboardView.vue'

const { init } = useMqttBridge()

const activeTab = ref<'connections' | 'presets' | 'dashboard'>('dashboard')

onMounted(() => {
  init()
})
</script>

<template>
  <div class="app-layout">
    <header class="app-header">
      <div class="header-left">
        <div class="app-brand">
          <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="brand-icon">
            <circle cx="12" cy="12" r="10"/>
            <path d="M12 6v6l4 2"/>
          </svg>
          <span class="brand-name">MQTTX</span>
          <span class="brand-version">v0.1</span>
        </div>
      </div>
      <div class="header-tabs">
        <button
          :class="['tab-btn', { active: activeTab === 'dashboard' }]"
          @click="activeTab = 'dashboard'"
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="3" width="7" height="7"/><rect x="14" y="3" width="7" height="7"/><rect x="14" y="14" width="7" height="7"/><rect x="3" y="14" width="7" height="7"/></svg>
          看板
        </button>
        <button
          :class="['tab-btn', { active: activeTab === 'connections' }]"
          @click="activeTab = 'connections'"
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M4 4h16v16H4z"/><path d="M9 9h6v6H9z"/><path d="M9 12h6"/></svg>
          连接管理
        </button>
        <button
          :class="['tab-btn', { active: activeTab === 'presets' }]"
          @click="activeTab = 'presets'"
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>
          预设
        </button>
      </div>
      <div class="header-right" />
    </header>
    <main class="app-main">
      <ConnectionList v-if="activeTab === 'connections'" />
      <PresetList v-if="activeTab === 'presets'" />
      <DashboardView v-if="activeTab === 'dashboard'" />
    </main>
  </div>
</template>

<style scoped>
.app-layout {
  display: flex;
  flex-direction: column;
  height: 100vh;
  background: var(--el-bg-color-page);
}

/* ─── Header ─────────────────────────────────────────────────── */
.app-header {
  display: flex;
  align-items: center;
  padding: 0 16px;
  height: 48px;
  background: var(--el-bg-color);
  border-bottom: 1px solid var(--el-border-color-light);
  flex-shrink: 0;
  gap: 24px;
}
.header-left {
  display: flex;
  align-items: center;
}
.app-brand {
  display: flex;
  align-items: center;
  gap: 8px;
}
.brand-icon {
  color: var(--el-color-primary);
}
.brand-name {
  font-size: 18px;
  font-weight: 700;
  color: var(--el-color-primary);
  letter-spacing: 0.5px;
}
.brand-version {
  font-size: 11px;
  color: var(--el-text-color-placeholder);
  background: var(--el-fill-color);
  padding: 0 5px;
  border-radius: 4px;
  line-height: 18px;
}

/* ─── Tabs ───────────────────────────────────────────────────── */
.header-tabs {
  display: flex;
  gap: 2px;
  background: var(--el-fill-color);
  padding: 2px;
  border-radius: 8px;
}
.tab-btn {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 0 12px;
  height: 28px;
  border: none;
  background: transparent;
  border-radius: 6px;
  font-size: 13px;
  color: var(--el-text-color-secondary);
  cursor: pointer;
  transition: all 0.15s;
  font-family: inherit;
  white-space: nowrap;
}
.tab-btn:hover {
  color: var(--el-text-color-primary);
}
.tab-btn.active {
  background: var(--el-bg-color);
  color: var(--el-color-primary);
  font-weight: 500;
  box-shadow: 0 1px 2px rgba(0,0,0,0.06);
}
.tab-btn svg {
  flex-shrink: 0;
}
.header-right {
  flex: 1;
}

/* ─── Main ───────────────────────────────────────────────────── */
.app-main {
  flex: 1;
  padding: 14px;
  overflow: auto;
}
</style>