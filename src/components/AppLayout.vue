<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useMqttBridge } from '../stores/useMqttBridge'
import { useUpdater } from '../composables/useUpdater'
import ConnectionList from './connections/ConnectionList.vue'
import PresetList from './presets/PresetList.vue'
import DashboardView from './dashboard/DashboardView.vue'

const { init } = useMqttBridge()
const {
  state: updateState,
  checking: updateChecking,
  checkForUpdates,
  downloadAndInstall,
  silentCheck,
} = useUpdater()

const activeTab = ref<'connections' | 'presets' | 'dashboard'>('dashboard')
const isDark = ref(false)

function applyTheme(dark: boolean) {
  isDark.value = dark
  document.documentElement.classList.toggle('dark', dark)
}

function toggleTheme() {
  applyTheme(!isDark.value)
  try {
    localStorage.setItem('mqttx_theme', isDark.value ? 'dark' : 'light')
  } catch { /* ignore */ }
}

function getSystemTheme(): boolean {
  return window.matchMedia('(prefers-color-scheme: dark)').matches
}

onMounted(() => {
  init()

  // 静默检查更新（应用启动时）
  silentCheck()

  // Listen for system theme changes
  window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', (e) => {
    const saved = localStorage.getItem('mqttx_theme')
    // Only follow system if user hasn't explicitly set a preference
    if (!saved) {
      applyTheme(e.matches)
    }
  })

  // Restore theme preference, fallback to system
  try {
    const saved = localStorage.getItem('mqttx_theme')
    if (saved === 'dark') {
      applyTheme(true)
    } else if (saved === 'light') {
      applyTheme(false)
    } else {
      // No saved preference → follow system
      applyTheme(getSystemTheme())
    }
  } catch { /* ignore */ }
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
      <div class="header-right">
        <!-- 更新按钮 -->
        <button
          v-if="updateState.available && !updateState.downloading && !updateState.installing"
          class="update-btn"
          @click="downloadAndInstall"
          title="有新版本可用"
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>
            <polyline points="7 10 12 15 17 10"/>
            <line x1="12" y1="15" x2="12" y2="3"/>
          </svg>
          <span class="update-badge">v{{ updateState.version }}</span>
        </button>

        <!-- 下载进度 -->
        <div
          v-else-if="updateState.downloading"
          class="update-progress"
          title="正在下载更新..."
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>
            <polyline points="7 10 12 15 17 10"/>
            <line x1="12" y1="15" x2="12" y2="3"/>
          </svg>
          <span class="progress-text">{{ updateState.downloadProgress }}%</span>
        </div>

        <!-- 安装中 -->
        <div
          v-else-if="updateState.installing"
          class="update-installing"
          title="正在安装更新..."
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="spin">
            <circle cx="12" cy="12" r="10" stroke-dasharray="32" stroke-dashoffset="32"/>
          </svg>
          <span class="progress-text">安装中...</span>
        </div>

        <!-- 手动检查更新按钮（无更新时） -->
        <button
          v-else
          class="check-update-btn"
          :disabled="updateChecking"
          @click="checkForUpdates"
          :title="updateChecking ? '检查中...' : '检查更新'"
        >
          <svg
            width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
            :class="{ spin: updateChecking }"
          >
            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>
            <polyline points="7 10 12 15 17 10"/>
            <line x1="12" y1="15" x2="12" y2="3"/>
          </svg>
        </button>

        <button class="theme-btn" @click="toggleTheme" :title="isDark ? '切换到亮色模式' : '切换到暗色模式'">
          <svg v-if="isDark" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="5"/><line x1="12" y1="1" x2="12" y2="3"/><line x1="12" y1="21" x2="12" y2="23"/><line x1="4.22" y1="4.22" x2="5.64" y2="5.64"/><line x1="18.36" y1="18.36" x2="19.78" y2="19.78"/><line x1="1" y1="12" x2="3" y2="12"/><line x1="21" y1="12" x2="23" y2="12"/><line x1="4.22" y1="19.78" x2="5.64" y2="18.36"/><line x1="18.36" y1="5.64" x2="19.78" y2="4.22"/></svg>
          <svg v-else width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"/></svg>
        </button>
      </div>
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
  background: var(--comfort-bg);
}

/* ─── Header ─────────────────────────────────────────────────── */
.app-header {
  display: flex;
  align-items: center;
  padding: 0 16px;
  height: 48px;
  background: var(--comfort-bg-card);
  border-bottom: 1px solid var(--comfort-border);
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
  color: var(--comfort-primary);
  letter-spacing: 0.5px;
}
.brand-version {
  font-size: 11px;
  color: var(--comfort-text-muted);
  background: var(--comfort-fill-color, var(--el-fill-color));
  padding: 0 5px;
  border-radius: 4px;
  line-height: 18px;
}

/* ─── Tabs ───────────────────────────────────────────────────── */
.header-tabs {
  display: flex;
  gap: 2px;
  background: var(--comfort-fill-color, var(--el-fill-color));
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
  color: var(--comfort-text-secondary);
  cursor: pointer;
  transition: all 0.15s;
  font-family: inherit;
  white-space: nowrap;
}
.tab-btn:hover {
  color: var(--comfort-text);
}
.tab-btn.active {
  background: var(--comfort-bg-card);
  color: var(--comfort-primary);
  font-weight: 500;
  box-shadow: var(--comfort-shadow);
}
.tab-btn svg {
  flex-shrink: 0;
}
.header-right {
  flex: 1;
  display: flex;
  justify-content: flex-end;
}
.theme-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  border: 1px solid var(--comfort-border);
  border-radius: 6px;
  background: var(--comfort-bg-card);
  color: var(--comfort-text-secondary);
  cursor: pointer;
  transition: all 0.2s;
}
.theme-btn:hover {
  color: var(--comfort-primary);
  border-color: var(--comfort-primary-light);
  background: var(--comfort-primary-bg);
}
.theme-btn svg {
  flex-shrink: 0;
}

/* ─── Update Button ──────────────────────────────────────────── */
.update-btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 0 10px;
  height: 30px;
  border: 1px solid var(--el-color-primary, #409eff);
  border-radius: 6px;
  background: var(--el-color-primary-light-9, #ecf5ff);
  color: var(--el-color-primary, #409eff);
  cursor: pointer;
  transition: all 0.2s;
  font-family: inherit;
  font-size: 13px;
  white-space: nowrap;
}
.update-btn:hover {
  background: var(--el-color-primary, #409eff);
  color: #fff;
}
.update-badge {
  font-weight: 600;
  font-size: 11px;
}

/* ─── Update Progress ────────────────────────────────────────── */
.update-progress,
.update-installing {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 0 10px;
  height: 30px;
  border-radius: 6px;
  background: var(--comfort-fill-color, var(--el-fill-color));
  color: var(--comfort-text-secondary);
  font-size: 12px;
}
.progress-text {
  font-variant-numeric: tabular-nums;
  font-weight: 500;
}

/* ─── Check Update Button ────────────────────────────────────── */
.check-update-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  border: 1px solid var(--comfort-border);
  border-radius: 6px;
  background: var(--comfort-bg-card);
  color: var(--comfort-text-secondary);
  cursor: pointer;
  transition: all 0.2s;
}
.check-update-btn:hover:not(:disabled) {
  color: var(--comfort-primary);
  border-color: var(--comfort-primary-light);
  background: var(--comfort-primary-bg);
}
.check-update-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* ─── Spin animation ─────────────────────────────────────────── */
.spin {
  animation: spin 1s linear infinite;
}
@keyframes spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}

/* ─── Main ───────────────────────────────────────────────────── */
.app-main {
  flex: 1;
  padding: 14px;
  overflow: auto;
}
</style>