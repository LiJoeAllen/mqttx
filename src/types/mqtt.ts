// ─── Subscription ──────────────────────────────────────────────────────────────

export interface Subscription {
  id: string
  connectionId: string
  topic: string
  qos: number
}

// ─── Aliyun Preset ─────────────────────────────────────────────────────────────

/** 阿里云鉴权模式 */
export type AliyunAuthMode = 'token' | 'device_credential'

export interface AliyunPreset {
  /** 唯一 ID */
  id: string
  /** 预设名称 */
  name: string
  /** 分组 */
  group: string
  /** 实例 ID，如 post-cn-4591dq2ra1i */
  instanceId: string
  /** 鉴权模式 */
  authMode: AliyunAuthMode
  /** Access Key ID（Token 模式必填） */
  accessKeyId: string
  /** Access Key Secret（Token 模式必填） */
  accessKeySecret: string
  /** Group ID，如 GID-prod */
  groupId: string
  /** 设备 ID，用于拼接 ClientId */
  deviceId: string
  /** 设备 Access Key ID（一机一密模式必填） */
  deviceAccessKeyId: string
  /** 设备 Access Key Secret（一机一密模式必填） */
  deviceAccessKeySecret: string
}

export function defaultAliyunPreset(): AliyunPreset {
  return {
    id: crypto.randomUUID(),
    name: '',
    group: '',
    instanceId: '',
    authMode: 'token',
    accessKeyId: '',
    accessKeySecret: '',
    groupId: '',
    deviceId: `device_${Math.random().toString(36).slice(2, 10)}`,
    deviceAccessKeyId: '',
    deviceAccessKeySecret: '',
  }
}

/** 计算阿里云 MQTT 签名: HMAC-SHA1(Secret, ClientId) -> Base64 (使用 Web Crypto API) */
export async function calculateAliyunSignature(clientId: string, secret: string): Promise<string> {
  const encoder = new TextEncoder()
  const key = await crypto.subtle.importKey(
    'raw',
    encoder.encode(secret),
    { name: 'HMAC', hash: 'SHA-1' },
    false,
    ['sign'],
  )
  const signature = await crypto.subtle.sign('HMAC', key, encoder.encode(clientId))
  return bytesToBase64(new Uint8Array(signature))
}

/** 根据阿里云预设生成连接参数 */
export async function generateAliyunConnection(preset: AliyunPreset): Promise<Pick<MqttConnection, 'host' | 'port' | 'username' | 'password' | 'clientId' | 'protocolVersion' | 'cleanStart'>> {
  const clientId = `${preset.groupId}@@@${preset.deviceId}`
  const username = `Signature|${preset.accessKeyId}|${preset.instanceId}`
  const password = await calculateAliyunSignature(clientId, preset.accessKeySecret)

  return {
    host: `${preset.instanceId}.mqtt.aliyuncs.com`,
    port: 1883,
    username,
    password,
    clientId,
    protocolVersion: '3.1.1',
    cleanStart: true,
  }
}

function bytesToBase64(bytes: Uint8Array): string {
  const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'
  let result = ''
  for (let i = 0; i < bytes.length; i += 3) {
    const b0 = bytes[i]
    const b1 = i + 1 < bytes.length ? bytes[i + 1] : 0
    const b2 = i + 2 < bytes.length ? bytes[i + 2] : 0
    result += chars[(b0 >> 2) & 0x3f]
    result += chars[((b0 << 4) | (b1 >> 4)) & 0x3f]
    result += i + 1 < bytes.length ? chars[((b1 << 2) | (b2 >> 6)) & 0x3f] : '='
    result += i + 2 < bytes.length ? chars[b2 & 0x3f] : '='
  }
  return result
}

// ─── Connection ────────────────────────────────────────────────────────────────

export interface MqttConnection {
  id: string
  name: string
  host: string
  port: number
  username: string
  password: string
  clientId: string
  /** MQTT 协议版本：3.1.1 / 5.0 */
  protocolVersion: '3.1.1' | '5.0'
  cleanStart: boolean
  sessionExpiryInterval: number
  keepAlive: number
  ssl: boolean
  /** 分组名称，空字符串表示未分组 */
  group: string
  receiveMaximum: number | null
  maximumPacketSize: number | null
  topicAliasMaximum: number | null
  // runtime state (not persisted)
  status: 'disconnected' | 'connecting' | 'connected' | 'error'
  lastError: string
}

export function defaultConnection(): MqttConnection {
  return {
    id: crypto.randomUUID(),
    name: '',
    host: 'localhost',
    port: 1883,
    username: '',
    password: '',
    clientId: `mqttx_${Math.random().toString(36).slice(2, 10)}`,
    protocolVersion: '5.0',
    cleanStart: true,
    sessionExpiryInterval: 0,
    keepAlive: 60,
    ssl: false,
    group: '',
    receiveMaximum: null,
    maximumPacketSize: null,
    topicAliasMaximum: null,
    status: 'disconnected',
    lastError: '',
  }
}

// ─── Preset ────────────────────────────────────────────────────────────────────

export interface Preset {
  id: string
  name: string
  topic: string
  payloadTemplate: string
  qos: 0 | 1 | 2
  retain: boolean
  userProperties: { key: string; value: string }[]
}

export function defaultPreset(): Preset {
  return {
    id: crypto.randomUUID(),
    name: '',
    topic: '',
    payloadTemplate: '',
    qos: 0,
    retain: false,
    userProperties: [],
  }
}

// ─── Message ───────────────────────────────────────────────────────────────────

export interface MqttMessage {
  id: string
  connectionId: string
  topic: string
  payload: string
  qos: number
  retain: boolean
  timestamp: number
  direction: 'in' | 'out'
  // v5 properties
  reasonCode?: number
  userProperties?: { key: string; value: string }[]
  contentType?: string
  contentEncoding?: string
  responseTopic?: string
  correlationData?: string
  messageExpiryInterval?: number
  subscriptionIdentifier?: number
}

// ─── Variable Bindings ─────────────────────────────────────────────────────────

export interface VariableBindings {
  [key: string]: string
}

// ─── Tauri IPC DTOs ────────────────────────────────────────────────────────────

export interface MqttConnectionDto {
  id: string
  name: string
  host: string
  port: number
  username: string
  password: string
  client_id: string
  protocol_version: string
  clean_start: boolean
  session_expiry_interval: number
  keep_alive: number
  ssl: boolean
  receive_maximum: number | null
  maximum_packet_size: number | null
  topic_alias_maximum: number | null
}

export function toConnectionDto(conn: MqttConnection): MqttConnectionDto {
  return {
    id: conn.id,
    name: conn.name,
    host: conn.host,
    port: conn.port,
    username: conn.username,
    password: conn.password,
    client_id: conn.clientId,
    protocol_version: conn.protocolVersion,
    clean_start: conn.cleanStart,
    session_expiry_interval: conn.sessionExpiryInterval,
    keep_alive: conn.keepAlive,
    ssl: conn.ssl,
    receive_maximum: conn.receiveMaximum,
    maximum_packet_size: conn.maximumPacketSize,
    topic_alias_maximum: conn.topicAliasMaximum,
  }
}

export interface PublishDto {
  connection_id: string
  topic: string
  payload: string
  qos: number
  retain: boolean
  user_properties: { key: string; value: string }[]
  content_type: string | null
  message_expiry_interval: number | null
  response_topic: string | null
  correlation_data: string | null
}

export interface SubscribeDto {
  connection_id: string
  topic: string
  qos: number
}

export interface MqttMessageEvent {
  connection_id: string
  topic: string
  payload: string
  qos: number
  retain: boolean
  timestamp: number
  reason_code: number | null
  user_properties: [string, string][]
  content_type: string | null
  content_encoding: string | null
  response_topic: string | null
  correlation_data: string | null
  message_expiry_interval: number | null
  subscription_identifier: number | null
}

export interface MqttStatusEvent {
  connection_id: string
  status: string
  error: string | null
  session_present: boolean | null
  reason_code: number | null
}

export interface ConnectionStatus {
  id: string
  connected: boolean
  error: string | null
}

export interface MqttLogEvent {
  timestamp: number
  connection_id: string
  level: string
  event: string
  message: string
  details: string | null
}
