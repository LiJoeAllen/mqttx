import { ref, onMounted } from 'vue'
import { check } from '@tauri-apps/plugin-updater'
import { relaunch } from '@tauri-apps/plugin-process'

/**
 * 更新状态
 */
export interface UpdateState {
  /** 是否有可用更新 */
  available: boolean
  /** 当前版本 */
  currentVersion: string
  /** 新版本号 */
  version?: string
  /** 更新日期 */
  date?: string
  /** 更新说明 */
  body?: string
  /** 下载进度 (0-100) */
  downloadProgress: number
  /** 是否正在下载 */
  downloading: boolean
  /** 是否正在安装 */
  installing: boolean
  /** 错误信息 */
  error: string | null
}

/**
 * Tauri 自动更新 composable
 *
 * 用法：
 * ```ts
 * const updater = useUpdater()
 * await updater.checkForUpdates()
 * ```
 */
export function useUpdater() {
  const state = ref<UpdateState>({
    available: false,
    currentVersion: '',
    downloadProgress: 0,
    downloading: false,
    installing: false,
    error: null,
  })

  const checking = ref(false)

  /**
   * 检查更新
   */
  async function checkForUpdates(): Promise<void> {
    checking.value = true
    state.value.error = null

    try {
      const update = await check()

      if (update) {
        state.value.available = true
        state.value.version = update.version
        state.value.date = update.date ?? undefined
        state.value.body = update.body ?? undefined
        state.value.currentVersion = update.currentVersion ?? ''
      } else {
        state.value.available = false
      }
    } catch (err: unknown) {
      const message = err instanceof Error ? err.message : String(err)
      // 忽略网络错误（用户可能离线）, 只记录不显示错误
      console.debug('[updater] 检查更新失败:', message)
      state.value.error = null // 不显示错误给用户，静默失败
    } finally {
      checking.value = false
    }
  }

  /**
   * 下载并安装更新，然后重启应用
   */
  async function downloadAndInstall(): Promise<void> {
    if (!state.value.available) return

    state.value.downloading = true
    state.value.error = null

    try {
      const update = await check()
      if (!update) {
        state.value.available = false
        return
      }

      // 下载更新（带进度）
      let downloaded = 0
      let total = 0

      await update.download((event) => {
        switch (event.event) {
          case 'Started':
            total = event.data.contentLength ?? 0
            state.value.downloadProgress = 0
            break
          case 'Progress':
            downloaded += event.data.chunkLength
            if (total > 0) {
              state.value.downloadProgress = Math.min(
                100,
                Math.round((downloaded / total) * 100),
              )
            }
            break
          case 'Finished':
            state.value.downloadProgress = 100
            break
        }
      })

      state.value.downloading = false
      state.value.installing = true

      // 安装并重启
      await update.install()
      await relaunch()
    } catch (err: unknown) {
      const message = err instanceof Error ? err.message : String(err)
      state.value.error = message
      state.value.downloading = false
      state.value.installing = false
    }
  }

  /**
   * 静默检查更新（应用启动时调用，仅当有更新时设置状态）
   */
  async function silentCheck(): Promise<void> {
    try {
      const update = await check()
      if (update) {
        state.value.available = true
        state.value.version = update.version
        state.value.date = update.date ?? undefined
        state.value.body = update.body ?? undefined
        state.value.currentVersion = update.currentVersion ?? ''
      }
    } catch {
      // 静默失败，不显示任何错误
    }
  }

  return {
    state,
    checking,
    checkForUpdates,
    downloadAndInstall,
    silentCheck,
  }
}