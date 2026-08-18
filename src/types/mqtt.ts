// ─── Subscription ──────────────────────────────────────────────────────────────

export interface Subscription {
  id: string
  connectionId: string
  topic: string
  qos: number
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
  cleanStart: boolean
  sessionExpiryInterval: number
  keepAlive: number
  ssl: boolean
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
    cleanStart: true,
    sessionExpiryInterval: 0,
    keepAlive: 60,
    ssl: false,
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