import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { onUnmounted } from 'vue'
import type {
  MqttConnectionDto,
  PublishDto,
  SubscribeDto,
  MqttMessageEvent,
  MqttStatusEvent,
  MqttLogEvent,
  ConnectionStatus,
} from '../types/mqtt'
import type { MqttMessage } from '../types/mqtt'
import { useConnections } from './useConnections'
import { useMessages } from './useMessages'
import { useLogs } from './useLogs'
import { useSubscriptions } from './useSubscriptions'
import type { LogEntry } from './useLogs'

/**
 * Initialize MQTT event listeners.
 * Call this once in App.vue setup.
 */
export function useMqttBridge() {
  const { updateStatus } = useConnections()
  const { push: pushMessage } = useMessages()
  const { push: pushLog } = useLogs()

  let unlistenMessage: (() => void) | null = null
  let unlistenStatus: (() => void) | null = null
  let unlistenLog: (() => void) | null = null

  async function init() {
    // Listen for incoming messages
    unlistenMessage = await listen<MqttMessageEvent>('mqtt:message', (event) => {
      const e = event.payload
      const msg: MqttMessage = {
        id: crypto.randomUUID(),
        connectionId: e.connection_id,
        topic: e.topic,
        payload: e.payload,
        qos: e.qos,
        retain: e.retain,
        timestamp: e.timestamp,
        direction: 'in',
        reasonCode: e.reason_code ?? undefined,
        userProperties: e.user_properties.map(([k, v]) => ({ key: k, value: v })),
        contentType: e.content_type ?? undefined,
        contentEncoding: e.content_encoding ?? undefined,
        responseTopic: e.response_topic ?? undefined,
        correlationData: e.correlation_data ?? undefined,
        messageExpiryInterval: e.message_expiry_interval ?? undefined,
        subscriptionIdentifier: e.subscription_identifier ?? undefined,
      }
      pushMessage(msg)
    })

    // Listen for connection status changes
    unlistenStatus = await listen<MqttStatusEvent>('mqtt:status', (event) => {
      const e = event.payload
      switch (e.status) {
        case 'connecting':
          updateStatus(e.connection_id, 'connecting')
          break
        case 'connected': {
          updateStatus(e.connection_id, 'connected')
          // Auto-resubscribe all persisted subscriptions for this connection
          const { getSubscriptionsByConnection } = useSubscriptions()
          const subs = getSubscriptionsByConnection(e.connection_id)
          for (const sub of subs) {
            mqttSubscribe({ connection_id: e.connection_id, topic: sub.topic, qos: sub.qos }).catch(() => {
              // Silently ignore resubscribe errors — the user will see them in the log
            })
          }
          break
        }
        case 'disconnected':
          updateStatus(e.connection_id, 'disconnected')
          break
        case 'error':
          updateStatus(e.connection_id, 'error', e.error ?? 'Unknown error')
          break
      }
    })

    // Listen for log events
    unlistenLog = await listen<MqttLogEvent>('mqtt:log', (event) => {
      const e = event.payload
      const log: LogEntry = {
        id: crypto.randomUUID(),
        timestamp: e.timestamp,
        connectionId: e.connection_id,
        level: e.level as 'info' | 'warn' | 'error',
        event: e.event,
        message: e.message,
        details: e.details ?? undefined,
      }
      pushLog(log)
    })
  }

  onUnmounted(() => {
    unlistenMessage?.()
    unlistenStatus?.()
    unlistenLog?.()
  })

  return { init }
}

// ─── Tauri invoke wrappers ─────────────────────────────────────────────────────

export async function mqttConnect(conn: MqttConnectionDto): Promise<void> {
  await invoke('mqtt_connect', { conn })
}

export async function mqttDisconnect(connectionId: string): Promise<void> {
  await invoke('mqtt_disconnect', { connectionId })
}

export async function mqttPublish(publish: PublishDto): Promise<void> {
  await invoke('mqtt_publish', { publish })
}

export async function mqttSubscribe(subscribe: SubscribeDto): Promise<void> {
  await invoke('mqtt_subscribe', { subscribe })
}

export async function mqttUnsubscribe(
  connectionId: string,
  topic: string,
): Promise<void> {
  await invoke('mqtt_unsubscribe', { connectionId, topic })
}

export async function mqttGetConnections(): Promise<ConnectionStatus[]> {
  return await invoke<ConnectionStatus[]>('mqtt_get_connections')
}

export async function mqttGetConnectionStatus(
  connectionId: string,
): Promise<ConnectionStatus> {
  return await invoke<ConnectionStatus>('mqtt_get_connection_status', {
    connectionId,
  })
}

export async function mqttTestConnection(conn: MqttConnectionDto): Promise<string> {
  return await invoke<string>('mqtt_test_connection', { conn })
}

export async function mqttReadDebugLog(): Promise<string> {
  return await invoke<string>('mqtt_read_debug_log')
}

export async function mqttGetLogDir(): Promise<string> {
  return await invoke<string>('mqtt_get_log_dir')
}