<script setup>
import { defineAsyncComponent, nextTick, reactive, ref, watch } from 'vue'
import { ChevronDown, Route } from '@lucide/vue'
import { invoke, isTauri } from '@tauri-apps/api/core'
import { createTraffic, formatBytes, simulateTraffic } from './traffic'

const RulePanel = defineAsyncComponent(() => import('./RulePanel.vue'))
const expanded = ref(false)
const resizing = ref(false)
const desktop = isTauri()
document.documentElement.classList.toggle('desktop-preview', desktop)
const dragging = ref(false)
let dragOrigin
let suppressClick = false

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
  const bounds = event.currentTarget.getBoundingClientRect()
  const insets = expanded.value ? [0, 0, 0, 0]
    : [bounds.left, bounds.top, innerWidth - bounds.right, innerHeight - bounds.bottom]
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
  if (resizing.value) return
  if (expanded.value || !isTauri()) {
    expanded.value = !expanded.value
    return
  }
  resizing.value = true
  try {
    await invoke('resize_island', { expanded: true, topInset: event.currentTarget.getBoundingClientRect().top })
    expanded.value = true
  } catch (error) {
    console.error('Unable to expand the desktop island', error)
  } finally {
    resizing.value = false
  }
}

function shrinkIsland() {
  if (isTauri() && !expanded.value) {
    invoke('resize_island', { expanded: false, topInset: 0 }).catch(error => {
      console.error('Unable to shrink the desktop island', error)
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
    { id: 'system', name: '内网服务', domains: 'intranet.example.com', target: 'system' },
    { id: 'direct', name: 'WLAN 直连', domains: 'direct.example.com\nippure.com\nipv6.icanhazip.com\napi.ipify.org', target: 'wlan' },
  ],
  upstreams: [{ id: 'flyingbird', name: '飞鸟', protocol: 'http', address: '127.0.0.1:7890' }],
  fallback: 'flyingbird',
})
watch(running, (active, _, onCleanup) => {
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
  const timer = setInterval(() => simulateTraffic(traffic.value, rules), 1000)
  onCleanup(() => clearInterval(timer))
})
</script>

<template>
  <main class="shell mx-auto px-3 pt-3" :class="{ expanded }" :data-theme="theme">
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
    <Transition @after-leave="shrinkIsland">
      <RulePanel v-if="expanded" v-model:tun-enabled="tunEnabled" :theme="theme" :rules="rules" :running="running" :traffic="traffic" :sessions="sessions"
        @update:theme="setTheme" @toggle="running = !running" />
    </Transition>
  </main>
</template>

<style>
body { margin: 0; background: transparent; font: 13px "Inter", "Segoe UI", "Microsoft YaHei UI", sans-serif; color-scheme: dark; }
button, textarea, input { font: inherit; }
button { cursor: pointer; }
[data-theme] { --capsule-surface: rgba(24,24,27,.85); --surface: var(--capsule-surface); --field: rgba(255,255,255,.04); --edge: rgba(255,255,255,.08); --muted: #a1a1aa; --accent: #34d399; --ink: #052e24; --foreground: #f4f4f5; --cell: rgba(255,255,255,.035); --hover: rgba(255,255,255,.07); --selected: rgba(255,255,255,.09); color: var(--foreground); }
[data-theme="light"] { color-scheme: light; --capsule-surface: rgba(255,255,255,.78); --field: rgba(0,0,0,.025); --edge: rgba(0,0,0,.06); --muted: #71717a; --accent: #059669; --ink: #fff; --foreground: #18181b; --cell: rgba(0,0,0,.025); --hover: rgba(0,0,0,.045); --selected: #fff; }
.shell { max-width: 460px; }
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
.v-enter-active, .v-leave-active { transition: opacity .2s, transform .2s; }
.v-enter-from, .v-leave-to { opacity: 0; transform: translateY(-6px) scale(.985); }
@media (prefers-reduced-motion: reduce) { .v-enter-active, .v-leave-active, .island { transition: none; } }
</style>
