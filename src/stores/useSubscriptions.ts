import { ref, computed, watch } from 'vue'
import type { Subscription } from '../types/mqtt'

const STORAGE_KEY = 'mqttx_subscriptions'

function loadSubscriptions(): Subscription[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return []
    return JSON.parse(raw) as Subscription[]
  } catch {
    return []
  }
}

function saveSubscriptions(subs: Subscription[]) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(subs))
  } catch {
    // silently ignore
  }
}

const subscriptions = ref<Subscription[]>(loadSubscriptions())

watch(
  subscriptions,
  (val) => {
    saveSubscriptions(val)
  },
  { deep: true },
)

export function useSubscriptions() {
  function addSubscription(connId: string, topic: string, qos: number = 0) {
    const existing = subscriptions.value.find(
      (s) => s.connectionId === connId && s.topic === topic,
    )
    if (existing) {
      existing.qos = qos
      return false
    }
    subscriptions.value.push({
      id: crypto.randomUUID(),
      connectionId: connId,
      topic,
      qos,
    })
    return true
  }

  function removeSubscription(connId: string, topic: string) {
    subscriptions.value = subscriptions.value.filter(
      (s) => !(s.connectionId === connId && s.topic === topic),
    )
  }

  function removeAllByConnection(connId: string) {
    subscriptions.value = subscriptions.value.filter(
      (s) => s.connectionId !== connId,
    )
  }

  function getSubscriptionsByConnection(connId: string): Subscription[] {
    return subscriptions.value.filter((s) => s.connectionId === connId)
  }

  const allSubscriptions = computed(() => subscriptions.value)

  return {
    subscriptions,
    addSubscription,
    removeSubscription,
    removeAllByConnection,
    getSubscriptionsByConnection,
    allSubscriptions,
  }
}