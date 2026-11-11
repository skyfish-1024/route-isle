<script setup>
import { nextTick, reactive, ref, watch } from 'vue'
import { ChevronDown, Route } from '@lucide/vue'
import { invoke, isTauri } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import RulePanel from './RulePanel.vue'
import { createTraffic, formatBytes, simulateTraffic, subscribeMihomoTraffic } from './traffic'

const desktop = isTauri()
const isPanelWindow = desktop && new URLSearchParams(location.search).get('window') === 'panel'
const isCapsuleWindow = desktop && !isPanelWindow
document.documentElement.classList.toggle('desktop-preview', desktop)

const expanded = ref(false)
const popUp = ref(false)
const togglingPanel = ref(false)
const dragging = ref(false)
let dragOrigin
let suppressClick = false

function measureCapsuleInsets(el) {
  if (!el) return [12, 9, 12, 9]
  const bounds = el.getBoundingClientRect()
  return [
    Math.max(0, bounds.left),
    Math.max(0, bounds.top),
    Math.max(0, innerWidth - bounds.right),
    Math.max(0, innerHeight - bounds.bottom),
  ]
}

function prepareDrag(event) {
  if (dragging.value) return
  suppressClick = false
  if (!desktop || event.button !== 0 || event.pointerType !== 'mouse') return
  dragOrigin = { x: event.clientX, y: event.clientY }
  event.currentTarget.setPointerCapture(event.pointerId)
}

function startDrag(event) {
  if (!dragOrigin || !(event.buttons & 1) ||
    Math.hypot(event.clientX - dragOrigin.x, event.clientY - dragOrigin.y) < 9) return
  dragOrigin = undefined
  suppressClick = true
  dragging.value = true
  if (event.currentTarget.hasPointerCapture(event.pointerId)) {
    event.currentTarget.releasePointerCapture(event.pointerId)
  }
  const insets = measureCapsuleInsets(event.currentTarget)
  invoke('drag_island', { insets })
    .catch(error => console.error('Unable to move the desktop island', error))
    .finally(() => { dragging.value = false })
}

function endDrag() {
  dragOrigin = undefined
}

async function toggleIsland(event) {
  if (event.detail && suppressClick) {
    suppressClick = false
    return
  }
  if (!desktop) {
    expanded.value = !expanded.value
    return
  }
  if (togglingPanel.value) return
  togglingPanel.value = true
  try {
    const insets = measureCapsuleInsets(event.currentTarget)
    await invoke('toggle_panel', { insets })
  } catch (error) {
    console.error('Unable to toggle panel window', error)
  } finally {
    togglingPanel.value = false
  }
}

function onPanelHidden() {
  if (isPanelWindow && !expanded.value) {
    invoke('hide_panel').catch(error => {
      console.error('Unable to hide panel window', error)
    })
  }
}

const running = ref(false)
const traffic = ref(createTraffic())
const historyKey = 'routeisle-preview-traffic-history'
function loadHistory() {
  try {
    const saved = JSON.parse(localStorage.getItem(historyKey) || '[]')
    return Array.isArray(saved) ? saved : []
  } catch {
    return []
  }
}
const sessions = ref(loadHistory())
let startedAt
const tunEnabled = ref(false)
const theme = ref('dark')
let themeTransition

function setTheme(value) {
  if (value === theme.value) return
  themeTransition?.skipTransition()
  const apply = () => {
    theme.value = value
    return nextTick()
  }
  if (!document.startViewTransition || matchMedia('(prefers-reduced-motion: reduce)').matches) {
    apply()
    return
  }
  const transition = document.startViewTransition(apply)
  themeTransition = transition
  const clear = () => {
    if (themeTransition === transition) themeTransition = undefined
  }
  transition.ready.catch(() => {})
  transition.finished.then(clear, clear)
}

const rules = reactive({
  groups: [
    { id: 'system', name: '内网服务', domains: 'intranet.example.com', target: 'system', enabled: true },
    { id: 'direct', name: 'WLAN 直连', domains: 'direct.example.com\nippure.com\nipv6.icanhazip.com\napi.ipify.org', target: 'wlan', enabled: true },
  ],
  upstreams: [{ id: 'flyingbird', name: '飞鸟', protocol: 'http', address: '127.0.0.1:7890' }],
  fallback: 'flyingbird',
})

let controllerInfo = { host: '127.0.0.1', port: 17891, secret: 'routeisle-local-secret' }
let rulesLoaded = !desktop

if (desktop) {
  listen('panel-visibility', ({ payload }) => {
    expanded.value = Boolean(payload?.visible)
    popUp.value = Boolean(payload?.popUp)
  }).catch(error => console.error('Unable to listen panel visibility', error))

  invoke('get_window_state').then(([capsuleState, visibility]) => {
    if (visibility) {
      expanded.value = Boolean(visibility.visible)
      popUp.value = Boolean(visibility.popUp)
    }
    if (isCapsuleWindow && capsuleState) {
      running.value = Boolean(capsuleState.running)
      traffic.value.uploadRate = Number(capsuleState.uploadRate) || 0
      traffic.value.downloadRate = Number(capsuleState.downloadRate) || 0
      if (capsuleState.theme) theme.value = capsuleState.theme
    }
  }).catch(() => {})
}

if (isCapsuleWindow) {
  listen('capsule-state', ({ payload }) => {
    if (!payload) return
    running.value = Boolean(payload.running)
    traffic.value.uploadRate = Number(payload.uploadRate) || 0
    traffic.value.downloadRate = Number(payload.downloadRate) || 0
    if (payload.theme) theme.value = payload.theme
  }).catch(error => console.error('Unable to listen capsule state', error))
}

if (isPanelWindow) {
  invoke('load_rules').then(saved => {
    if (saved) {
      if (Array.isArray(saved.groups)) rules.groups = saved.groups
      if (Array.isArray(saved.upstreams)) rules.upstreams = saved.upstreams
      if (saved.fallback) rules.fallback = saved.fallback
      if (typeof saved.tunEnabled === 'boolean') tunEnabled.value = saved.tunEnabled
      if (saved.controller) controllerInfo = saved.controller
      if (typeof saved.running === 'boolean') running.value = saved.running
    }
  }).catch(error => {
    console.error('Unable to load proxy rules', error)
  }).finally(() => {
    rulesLoaded = true
  })

  watch([running, () => traffic.value.uploadRate, () => traffic.value.downloadRate, theme],
    ([isRunning, uploadRate, downloadRate, currentTheme]) => {
      invoke('sync_capsule_state', {
        running: isRunning,
        uploadRate: Number(uploadRate) || 0,
        downloadRate: Number(downloadRate) || 0,
        theme: currentTheme,
      }).catch(() => {})
    },
    { immediate: true },
  )
}

function serializeRules() {
  return {
    groups: rules.groups.map(group => ({
      id: group.id,
      name: group.name,
      domains: group.domains,
      target: group.target,
      enabled: group.enabled !== false,
    })),
    upstreams: rules.upstreams.map(upstream => ({ ...upstream })),
    fallback: rules.fallback,
    tunEnabled: tunEnabled.value,
  }
}

watch([rules, tunEnabled], () => {
  if (!isPanelWindow || !rulesLoaded) return
  invoke('save_rules', { rules: serializeRules() }).catch(error => {
    console.error('Unable to save proxy rules', error)
    if (tunEnabled.value) {
      tunEnabled.value = false
      running.value = false
    }
    alert(typeof error === 'string' ? error : '保存配置或启用 TUN 失败')
  })
}, { deep: true })

const toggling = ref(false)
async function toggleRunning() {
  if (toggling.value) return
  if (!desktop) {
    running.value = !running.value
    return
  }
  toggling.value = true
  try {
    if (running.value) {
      await invoke('stop_proxy')
      running.value = false
    } else {
      await invoke('start_proxy', { rules: serializeRules() })
      running.value = true
    }
  } catch (error) {
    console.error('Unable to toggle proxy', error)
    alert(typeof error === 'string' ? error : '代理切换失败')
  } finally {
    toggling.value = false
  }
}

watch(running, (active, _, onCleanup) => {
  if (isCapsuleWindow) return
  if (!active) {
    traffic.value.uploadRate = 0
    traffic.value.downloadRate = 0
    if (traffic.value.logs.length) {
      sessions.value = [{
        startedAt, endedAt: Date.now(),
        upload: traffic.value.upload, download: traffic.value.download,
        logs: [...traffic.value.logs],
        byExit: traffic.value.byExit, byRule: traffic.value.byRule,
      }, ...sessions.value].slice(0, 30)
      try {
        localStorage.setItem(historyKey, JSON.stringify(sessions.value))
      } catch {
        // History remains available in memory when storage is unavailable.
      }
    }
    return
  }
  startedAt = Date.now()
  traffic.value = createTraffic()
  if (!desktop) {
    const timer = setInterval(() => simulateTraffic(traffic.value, rules), 1000)
    onCleanup(() => clearInterval(timer))
    return
  }
  const unsubscribe = subscribeMihomoTraffic({
    trafficRef: traffic,
    rules,
    controller: controllerInfo,
    onDisconnect: () => {
      running.value = false
    },
  })
  onCleanup(unsubscribe)
})
</script>

<template>
  <main v-if="isCapsuleWindow" class="capsule-stage" :class="{ expanded, 'pop-up': popUp }" :data-theme="theme">
    <button class="island" :class="{ 'is-dragging': dragging }"
      @pointerdown="prepareDrag" @pointermove="startDrag" @pointerup="endDrag" @pointercancel="endDrag"
      :aria-expanded="expanded" @click="toggleIsland">
      <span class="capsule-brand">
        <span class="mark" aria-hidden="true">
          <Route :size="17" :stroke-width="1.8" />
        </span>
        <strong>分流岛</strong>
      </span>
      <span class="state" :class="{ running }" aria-hidden="true"><i></i></span>
      <span class="capsule-rate">
        <template v-if="running">
          <span>↑ {{ formatBytes(traffic.uploadRate) }}/s</span>
          <span>↓ {{ formatBytes(traffic.downloadRate) }}/s</span>
        </template>
        <span v-else class="capsule-idle">未启用</span>
      </span>
      <ChevronDown class="chevron" :size="13" aria-hidden="true" />
    </button>
  </main>

  <main v-else-if="isPanelWindow" class="panel-stage" :class="{ 'pop-up': popUp }" :data-theme="theme">
    <Transition @after-leave="onPanelHidden">
      <RulePanel v-if="expanded" v-model:tun-enabled="tunEnabled" :theme="theme" :rules="rules" :running="running" :traffic="traffic" :sessions="sessions"
        @update:theme="setTheme" @toggle="toggleRunning" />
    </Transition>
  </main>

  <main v-else class="shell mx-auto px-3 pt-3" :class="{ expanded }" :data-theme="theme">
    <button class="island" :class="{ 'is-dragging': dragging }"
      @pointerdown="prepareDrag" @pointermove="startDrag" @pointerup="endDrag" @pointercancel="endDrag"
      :aria-expanded="expanded" @click="toggleIsland">
      <span class="capsule-brand">
        <span class="mark" aria-hidden="true">
          <Route :size="17" :stroke-width="1.8" />
        </span>
        <strong>分流岛</strong>
      </span>
      <span class="state" :class="{ running }" aria-hidden="true"><i></i></span>
      <span class="capsule-rate">
        <template v-if="running">
          <span>↑ {{ formatBytes(traffic.uploadRate) }}/s</span>
          <span>↓ {{ formatBytes(traffic.downloadRate) }}/s</span>
        </template>
        <span v-else class="capsule-idle">未启用</span>
      </span>
      <ChevronDown class="chevron" :size="13" aria-hidden="true" />
    </button>
    <Transition>
      <RulePanel v-if="expanded" v-model:tun-enabled="tunEnabled" :theme="theme" :rules="rules" :running="running" :traffic="traffic" :sessions="sessions"
        @update:theme="setTheme" @toggle="toggleRunning" />
    </Transition>
  </main>
</template>

<style>
html, body { margin: 0; overflow: hidden; background: transparent; font: 13px "Inter", "Segoe UI", "Microsoft YaHei UI", sans-serif; color-scheme: dark; }
button, textarea, input { font: inherit; }
button { cursor: pointer; }
[data-theme] { --capsule-surface: rgba(24,24,27,.85); --surface: var(--capsule-surface); --field: rgba(255,255,255,.04); --edge: rgba(255,255,255,.08); --muted: #a1a1aa; --accent: #34d399; --ink: #052e24; --foreground: #f4f4f5; --cell: rgba(255,255,255,.035); --hover: rgba(255,255,255,.07); --selected: rgba(255,255,255,.09); color: var(--foreground); }
[data-theme="light"] { color-scheme: light; --capsule-surface: rgba(255,255,255,.78); --field: rgba(0,0,0,.025); --edge: rgba(0,0,0,.06); --muted: #71717a; --accent: #059669; --ink: #fff; --foreground: #18181b; --cell: rgba(0,0,0,.025); --hover: rgba(0,0,0,.045); --selected: #fff; }
.shell { max-width: 460px; box-sizing: border-box; }
.capsule-stage { display: grid; place-items: center; width: 100vw; height: 100vh; box-sizing: border-box; }
.panel-stage { width: 100vw; height: 100vh; padding: 8px; box-sizing: border-box; }
.panel-stage > .panel { margin-top: 0; }
.desktop-preview [data-theme] { --surface: #242427; }
.desktop-preview [data-theme="light"] { --surface: #fafafa; }
.island { display: flex; align-items: center; gap: 8px; box-sizing: border-box; height: 30px; margin: 0 auto; padding: 0 10px; border: 1px solid var(--edge); border-radius: 999px; color: inherit; background: var(--capsule-surface); backdrop-filter: blur(24px) saturate(180%); box-shadow: 0 4px 10px #0001, 0 12px 24px #0001, inset 0 1px #ffffff0a; transition: padding .2s, transform .2s; }
.island:hover { padding-inline: 12px; }
.island { user-select: none; }
.island.is-dragging { cursor: grabbing; }
.island strong { font-size: 13px; font-weight: 500; letter-spacing: -.01em; }
.mark { display: grid; place-items: center; color: var(--muted); }
.mark svg { width: 14px; height: 14px; }
.state { display: flex; align-items: center; gap: 6px; margin-left: 8px; color: var(--muted); font-size: 11px; }
.state i { width: 6px; height: 6px; border-radius: 50%; background: #a1a1aa; }
.state.running i { background: #34d399; box-shadow: 0 0 7px #34d39966; }
.capsule-brand, .capsule-rate { flex: 0 0 62px; }
.capsule-brand { display: flex; align-items: center; gap: 8px; white-space: nowrap; }
.capsule-rate { display: grid; font-size: 10px; line-height: 11px; color: var(--muted); font-variant-numeric: tabular-nums; text-align: right; white-space: nowrap; }
.capsule-idle { line-height: 22px; text-align: center; }
.chevron { color: var(--muted); transition: transform .2s; }
.expanded .chevron { transform: rotate(180deg); }
.pop-up.expanded .chevron { transform: rotate(0deg); }
.v-enter-active, .v-leave-active { transition: opacity .16s ease, transform .16s ease; }
.v-enter-from, .v-leave-to { opacity: 0; transform: translateY(-6px) scale(.985); }
.pop-up .v-enter-from, .pop-up .v-leave-to { opacity: 0; transform: translateY(6px) scale(.985); }
@media (prefers-reduced-motion: reduce) { .v-enter-active, .v-leave-active, .island { transition: none; } }
</style>
