import { ref, computed } from 'vue'
import type { MqttMessage } from '../types/mqtt'

const MAX_MESSAGES = 500

const messages = ref<MqttMessage[]>([])

export function useMessages() {
  function push(msg: MqttMessage) {
    messages.value.push(msg)
    if (messages.value.length > MAX_MESSAGES) {
      messages.value.splice(0, messages.value.length - MAX_MESSAGES)
    }
  }

  function clear() {
    messages.value = []
  }

  function filterByTopic(topicFilter: string): MqttMessage[] {
    if (!topicFilter.trim()) return messages.value
    const filter = topicFilter.trim().toLowerCase()
    return messages.value.filter((m) => m.topic.toLowerCase().includes(filter))
  }

  function filterByConnection(connectionId: string): MqttMessage[] {
    if (!connectionId) return messages.value
    return messages.value.filter((m) => m.connectionId === connectionId)
  }

  const messageCount = computed(() => messages.value.length)

  return {
    messages,
    push,
    clear,
    filterByTopic,
    filterByConnection,
    messageCount,
  }
}