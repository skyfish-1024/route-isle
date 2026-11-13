<script setup>
import { computed, nextTick, reactive, ref, watch } from 'vue'
import { ChevronDown } from '@lucide/vue'
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
  if (!el) return [20, 23, 20, 23]
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

function mergeSessions(diskSessions = []) {
  const byStart = new Map()
  for (const s of sessions.value) {
    if (s?.startedAt) byStart.set(s.startedAt, s)
  }
  for (const s of diskSessions) {
    if (!s?.startedAt) continue
    const existing = byStart.get(s.startedAt)
    if (existing) {
      byStart.set(s.startedAt, { ...s, logs: existing.logs || [] })
    } else {
      byStart.set(s.startedAt, { ...s, logs: [] })
    }
  }
  sessions.value = Array.from(byStart.values()).sort((a, b) => b.startedAt - a.startedAt)
}

if (isPanelWindow) {
  invoke('load_session_logs').then(diskSessions => {
    if (Array.isArray(diskSessions) && diskSessions.length) {
      mergeSessions(diskSessions)
    }
  }).catch(() => {})
}
let startedAt
const systemProxyEnabled = ref(true)
const tunEnabled = ref(false)
const themeStorageKey = 'routeisle-theme-mode'
function loadThemeMode() {
  try {
    const saved = localStorage.getItem(themeStorageKey)
    if (saved === 'light' || saved === 'dark' || saved === 'system') return saved
  } catch {}
  return 'system'
}
const themeMode = ref(loadThemeMode())
const colorSchemeQuery = matchMedia('(prefers-color-scheme: dark)')
const systemPrefersDark = ref(colorSchemeQuery.matches)
colorSchemeQuery.addEventListener?.('change', (event) => {
  systemPrefersDark.value = event.matches
})
const theme = computed(() => (
  themeMode.value === 'system'
    ? (systemPrefersDark.value ? 'dark' : 'light')
    : themeMode.value
))
const capsuleTheme = ref(theme.value)
const activeTheme = computed(() => (isCapsuleWindow ? capsuleTheme.value : theme.value))
let themeTransition

function setThemeMode(value) {
  if (value === themeMode.value) return
  const prevResolved = theme.value
  const nextResolved = value === 'system' ? (systemPrefersDark.value ? 'dark' : 'light') : value
  const apply = () => {
    themeMode.value = value
    try { localStorage.setItem(themeStorageKey, value) } catch {}
    return nextTick()
  }
  if (prevResolved === nextResolved || !document.startViewTransition || matchMedia('(prefers-reduced-motion: reduce)').matches) {
    apply()
    return
  }
  themeTransition?.skipTransition()
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
    { id: 'system', name: '内网服务', domains: 'intranet.example.com\nlocalhost\n*.localhost\nlocalhost.*', target: 'system', enabled: true },
    { id: 'direct', name: 'WLAN 直连', domains: '*.direct.example.com\n*.ippure.com\nipv6.icanhazip.com\napi.ipify.org', target: 'wlan', enabled: true },
    {
      id: 'video',
      name: '国内视频',
      domains: '*.bilibili.com\n*.bilivideo.com\n*.bilivideo.cn\n*.hdslb.com\n*.biliapi.net\n*.biliapi.com\n*.acgvideo.com\n*.szbdyd.com\n*.iqiyi.com\n*.iqiyipic.com\n*.qy.net\n*.qiyipic.com\n*.71.am\nv.qq.com\n*.video.qq.com\n*.gtimg.com\n*.gtimg.cn\n*.qpic.cn\n*.smtcdns.com\n*.smtcdns.net\n*.youku.com\n*.ykimg.com\n*.tudou.com\n*.cibntv.net\n*.douyin.com\n*.douyinvod.com\n*.douyinpic.com\n*.ixigua.com\n*.bytevcloudcdn.com\n*.pstatp.com\n*.kuaishou.com\n*.kwai.com\n*.yximgs.com\n*.mgtv.com\n*.hunantv.com\n*.hitv.com\n*.yangshipin.cn\n*.cctv.com\n*.cctvpic.com\n*.miguvideo.com\n*.tv.sohu.com\n*.douyu.com\n*.douyucdn.cn\n*.huya.com\n*.msstatic.com',
      target: 'wlan',
      enabled: true,
    },
  ],
  upstreams: [{ id: 'local-proxy', name: '本地代理', protocol: 'http', address: '127.0.0.1:7890' }],
  fallback: 'local-proxy',
  cnFallback: 'wlan',
  bypassDomains: 'localhost\n*.localhost\nlocalhost.*\nlocalhost.v.qq.com\nlocalhost.weixin.qq.com\n*.local\n*.lan',
  cnDns: '223.5.5.5\n119.29.29.29',
  overseasDns: 'https://dns.google/dns-query',
})

let controllerInfo = { host: '127.0.0.1', port: 17891, secret: 'routeisle-local-secret' }
let rulesLoaded = !desktop
let suppressSave = false

if (!desktop) {
  watch(expanded, (open, _, onCleanup) => {
    if (!open) return
    const onPointerDown = (event) => {
      if (event.target?.closest?.('.shell, .picker-menu, .date-popover, .delete-popover, .editor, .editor-overlay')) return
      expanded.value = false
    }
    document.addEventListener('pointerdown', onPointerDown)
    onCleanup(() => document.removeEventListener('pointerdown', onPointerDown))
  })
}

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
      if (capsuleState.theme) capsuleTheme.value = capsuleState.theme
    }
  }).catch(() => {})
}

if (isCapsuleWindow) {
  listen('capsule-state', ({ payload }) => {
    if (!payload) return
    running.value = Boolean(payload.running)
    traffic.value.uploadRate = Number(payload.uploadRate) || 0
    traffic.value.downloadRate = Number(payload.downloadRate) || 0
    if (payload.theme) capsuleTheme.value = payload.theme
  }).catch(error => console.error('Unable to listen capsule state', error))
}

if (isPanelWindow) {
  invoke('load_rules').then(async saved => {
    let shouldAutoStart = false
    if (saved) {
      if (Array.isArray(saved.groups)) rules.groups = saved.groups
      if (Array.isArray(saved.upstreams)) rules.upstreams = saved.upstreams
      if (saved.fallback) rules.fallback = saved.fallback
      if (saved.cnFallback) rules.cnFallback = saved.cnFallback
      if (typeof saved.bypassDomains === 'string') rules.bypassDomains = saved.bypassDomains
      if (typeof saved.cnDns === 'string') rules.cnDns = saved.cnDns
      if (typeof saved.overseasDns === 'string') rules.overseasDns = saved.overseasDns
      if (typeof saved.systemProxyEnabled === 'boolean') systemProxyEnabled.value = saved.systemProxyEnabled
      if (typeof saved.tunEnabled === 'boolean') tunEnabled.value = saved.tunEnabled
      if (saved.controller) controllerInfo = saved.controller
      if (typeof saved.running === 'boolean') running.value = saved.running
      shouldAutoStart = Boolean(saved.autoStart) && !running.value
    }
    await nextTick()
    rulesLoaded = true
    if (shouldAutoStart) {
      try {
        await invoke('start_proxy', { rules: serializeRules() })
        running.value = true
      } catch (error) {
        console.error('Unable to resume proxy after elevation', error)
      }
    }
  }).catch(error => {
    console.error('Unable to load proxy rules', error)
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
    cnFallback: rules.cnFallback,
    bypassDomains: rules.bypassDomains,
    cnDns: rules.cnDns,
    overseasDns: rules.overseasDns,
    systemProxyEnabled: systemProxyEnabled.value,
    tunEnabled: tunEnabled.value,
  }
}

let saveRulesTimer = null
function persistRulesNow(tunEnabling = false) {
  if (!isPanelWindow || !rulesLoaded || suppressSave) return
  if (saveRulesTimer) {
    clearTimeout(saveRulesTimer)
    saveRulesTimer = null
  }
  invoke('save_rules', { rules: serializeRules(), wasRunning: running.value }).catch(async error => {
    console.error('Unable to save proxy rules', error)
    if (tunEnabling && tunEnabled.value) {
      suppressSave = true
      tunEnabled.value = false
      await nextTick()
      suppressSave = false
    }
    alert(typeof error === 'string' ? error : (tunEnabling ? '启用 TUN 失败' : '保存配置失败'))
  })
}

watch(systemProxyEnabled, () => {
  persistRulesNow(false)
})

watch(tunEnabled, (next, prev) => {
  persistRulesNow(Boolean(next && !prev))
})

watch(rules, () => {
  if (!isPanelWindow || !rulesLoaded || suppressSave) return
  if (saveRulesTimer) clearTimeout(saveRulesTimer)
  saveRulesTimer = setTimeout(() => {
    saveRulesTimer = null
    persistRulesNow(false)
  }, 350)
}, { deep: true })

const toggling = ref(false)
async function toggleRunning() {
  if (toggling.value) return
  if (!desktop) {
    running.value = !running.value
    return
  }
  if (saveRulesTimer) {
    clearTimeout(saveRulesTimer)
    saveRulesTimer = null
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
      const endedAt = Date.now()
      const sessionEntry = {
        startedAt, endedAt,
        upload: traffic.value.upload, download: traffic.value.download,
        byExit: traffic.value.byExit, byRule: traffic.value.byRule, byPair: traffic.value.byPair,
      }
      sessions.value = [{
        ...sessionEntry,
        logs: [...traffic.value.logs],
      }, ...sessions.value].slice(0, 30)
      try {
        localStorage.setItem(historyKey, JSON.stringify(sessions.value))
      } catch {
        // History remains available in memory when storage is unavailable.
      }
      if (desktop) {
        invoke('append_session_log', { session: sessionEntry }).catch(() => {})
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
    onLogsBatch: (entries) => {
      invoke('append_connection_logs', { entries }).catch(() => {})
    },
    onDisconnect: () => {
      running.value = false
    },
  })
  onCleanup(unsubscribe)
})
function onLogsCleared() {
  traffic.value.logs = []
  sessions.value = []
}
</script>

<template>
  <main v-if="isCapsuleWindow" class="capsule-stage" :class="{ expanded, 'pop-up': popUp }" :data-theme="activeTheme">
    <button class="island" :class="{ 'is-dragging': dragging }"
      @pointerdown="prepareDrag" @pointermove="startDrag" @pointerup="endDrag" @pointercancel="endDrag"
      :aria-expanded="expanded" @click="toggleIsland">
      <span class="capsule-brand">
        <span class="mark" :class="{ running }" aria-hidden="true">
          <svg viewBox="0 0 24 24" fill="none" stroke-linecap="round" stroke-linejoin="round">
            <path class="branch-low" d="M10.2 12C13.6 12 14.2 17.2 18.8 17.2" stroke-width="2.3" />
            <path class="branch-high" d="M10.2 12C13.6 12 14.2 6.8 18.8 6.8" stroke-width="2.4" />
            <path class="trunk" d="M3.8 12H10.2" stroke-width="2.4" />
            <circle class="node-high" cx="18.8" cy="6.8" r="2.3" />
            <circle class="node-low" cx="18.8" cy="17.2" r="2.0" />
            <circle class="node-root" cx="10.2" cy="12" r="2.2" />
          </svg>
        </span>
        <strong>分流岛</strong>
      </span>
      <span class="state" :class="{ running }" aria-hidden="true"><i></i></span>
      <span class="capsule-right">
        <span class="capsule-rate">
          <template v-if="running">
            <span>↑ {{ formatBytes(traffic.uploadRate) }}/s</span>
            <span>↓ {{ formatBytes(traffic.downloadRate) }}/s</span>
          </template>
          <span v-else class="capsule-idle">未启用</span>
        </span>
        <ChevronDown class="chevron" :size="13" aria-hidden="true" />
      </span>
    </button>
  </main>

  <main v-else-if="isPanelWindow" class="panel-stage" :class="{ 'pop-up': popUp }" :data-theme="theme">
    <Transition @after-leave="onPanelHidden">
      <RulePanel v-if="expanded" v-model:system-proxy-enabled="systemProxyEnabled" v-model:tun-enabled="tunEnabled" :theme="theme" :theme-mode="themeMode" :rules="rules" :running="running" :toggling="toggling" :traffic="traffic" :sessions="sessions"
        @update:theme-mode="setThemeMode" @toggle="toggleRunning" @logs-cleared="onLogsCleared" />
    </Transition>
  </main>

  <main v-else class="shell mx-auto px-3 pt-3" :class="{ expanded }" :data-theme="theme">
    <button class="island" :class="{ 'is-dragging': dragging }"
      @pointerdown="prepareDrag" @pointermove="startDrag" @pointerup="endDrag" @pointercancel="endDrag"
      :aria-expanded="expanded" @click="toggleIsland">
      <span class="capsule-brand">
        <span class="mark" :class="{ running }" aria-hidden="true">
          <svg viewBox="0 0 24 24" fill="none" stroke-linecap="round" stroke-linejoin="round">
            <path class="branch-low" d="M10.2 12C13.6 12 14.2 17.2 18.8 17.2" stroke-width="2.3" />
            <path class="branch-high" d="M10.2 12C13.6 12 14.2 6.8 18.8 6.8" stroke-width="2.4" />
            <path class="trunk" d="M3.8 12H10.2" stroke-width="2.4" />
            <circle class="node-high" cx="18.8" cy="6.8" r="2.3" />
            <circle class="node-low" cx="18.8" cy="17.2" r="2.0" />
            <circle class="node-root" cx="10.2" cy="12" r="2.2" />
          </svg>
        </span>
        <strong>分流岛</strong>
      </span>
      <span class="state" :class="{ running }" aria-hidden="true"><i></i></span>
      <span class="capsule-right">
        <span class="capsule-rate">
          <template v-if="running">
            <span>↑ {{ formatBytes(traffic.uploadRate) }}/s</span>
            <span>↓ {{ formatBytes(traffic.downloadRate) }}/s</span>
          </template>
          <span v-else class="capsule-idle">未启用</span>
        </span>
        <ChevronDown class="chevron" :size="13" aria-hidden="true" />
      </span>
    </button>
    <Transition>
      <RulePanel v-if="expanded" v-model:system-proxy-enabled="systemProxyEnabled" v-model:tun-enabled="tunEnabled" :theme="theme" :theme-mode="themeMode" :rules="rules" :running="running" :toggling="toggling" :traffic="traffic" :sessions="sessions"
        @update:theme-mode="setThemeMode" @toggle="toggleRunning" @logs-cleared="onLogsCleared" />
    </Transition>
  </main>
</template>

<style>
html, body { margin: 0; overflow: hidden; background: transparent; font: 13px "Inter", "Segoe UI", "Microsoft YaHei UI", sans-serif; color-scheme: dark; }
@supports not selector(::-webkit-scrollbar) {
  * { scrollbar-width: thin; scrollbar-color: color-mix(in srgb, var(--muted) 35%, transparent) transparent; }
}
::-webkit-scrollbar { width: 5px; height: 5px; }
::-webkit-scrollbar-track { background: transparent; }
::-webkit-scrollbar-thumb { border-radius: 999px; background: color-mix(in srgb, var(--muted, #a1a1aa) 28%, transparent); }
::-webkit-scrollbar-thumb:hover { background: color-mix(in srgb, var(--muted, #a1a1aa) 52%, transparent); }
::-webkit-scrollbar-button { display: none; width: 0; height: 0; }
::-webkit-scrollbar-corner { background: transparent; }
button, textarea, input { font: inherit; }
button { cursor: pointer; }
[data-theme] { --capsule-surface: rgba(24,24,27,.85); --capsule-active-surface: rgba(16,16,19,.95); --island-shadow: 0 4px 10px rgba(0, 0, 0, .16), 0 8px 16px rgba(0, 0, 0, .12), inset 0 1px #ffffff0a; --surface: var(--capsule-surface); --field: rgba(255,255,255,.04); --edge: rgba(255,255,255,.08); --muted: #a1a1aa; --accent: #34d399; --ink: #052e24; --foreground: #f4f4f5; --cell: rgba(255,255,255,.035); --hover: rgba(255,255,255,.07); --selected: rgba(255,255,255,.09); color: var(--foreground); }
[data-theme="light"] { color-scheme: light; --capsule-surface: rgba(255,255,255,.82); --capsule-active-surface: rgba(238,238,242,.95); --island-shadow: 0 2px 6px rgba(0, 0, 0, .06), 0 6px 14px rgba(0, 0, 0, .05), inset 0 1px rgba(255, 255, 255, .7); --field: rgba(0,0,0,.025); --edge: rgba(0,0,0,.06); --muted: #71717a; --accent: #059669; --ink: #fff; --foreground: #18181b; --cell: rgba(0,0,0,.025); --hover: rgba(0,0,0,.045); --selected: #fff; }
.shell { max-width: 460px; box-sizing: border-box; }
.capsule-stage { display: grid; place-items: center; width: 100vw; height: 100vh; box-sizing: border-box; }
.panel-stage { width: 100vw; height: 100vh; padding: 14px; box-sizing: border-box; }
.panel-stage > .panel { margin-top: 0; }
.desktop-preview [data-theme] { --surface: #242427; }
.desktop-preview [data-theme="light"] { --surface: #fafafa; }
.island { display: grid; grid-template-columns: 1fr auto 1fr; align-items: center; gap: 10px; box-sizing: border-box; min-width: 214px; height: 30px; margin: 0 auto; padding: 0 11px; border: 1px solid var(--edge); border-radius: 999px; color: inherit; background: var(--capsule-surface); backdrop-filter: blur(24px) saturate(180%); box-shadow: var(--island-shadow); transition: padding .2s, transform .2s, background-color .18s ease, box-shadow .18s ease; }
.island:hover, .expanded .island { background: var(--capsule-active-surface); }
.island:hover { padding-inline: 13px; }
.island { user-select: none; }
.island.is-dragging { cursor: grabbing; }
.island strong { font-size: 13px; font-weight: 500; letter-spacing: -.01em; }
.mark { display: grid; place-items: center; color: var(--muted); }
.mark svg { width: 16px; height: 16px; }
.mark .trunk { stroke: var(--foreground); opacity: .94; }
.mark .branch-high { stroke: var(--accent); opacity: .92; transition: opacity .2s ease; }
.mark .branch-low { stroke: var(--foreground); opacity: .72; }
.mark .node-root { fill: var(--foreground); opacity: .96; }
.mark .node-high { fill: var(--accent); opacity: .95; }
.mark .node-low { fill: var(--foreground); opacity: .78; }
.mark.running .branch-high, .mark.running .node-high { opacity: 1; }
.state { display: flex; align-items: center; justify-content: center; color: var(--muted); font-size: 11px; }
.state i { width: 6px; height: 6px; border-radius: 50%; background: #a1a1aa; }
.state.running i { background: #34d399; box-shadow: 0 0 7px #34d39966; }
.capsule-brand { display: flex; align-items: center; justify-self: start; gap: 7px; white-space: nowrap; }
.capsule-right { display: flex; align-items: center; justify-self: end; gap: 6px; white-space: nowrap; }
.capsule-rate { display: grid; min-width: 58px; font-size: 10px; line-height: 11px; color: var(--muted); font-variant-numeric: tabular-nums; text-align: right; white-space: nowrap; }
.capsule-idle { line-height: 22px; text-align: right; }
.chevron { color: var(--muted); transition: transform .2s; }
.expanded .chevron { transform: rotate(180deg); }
.pop-up.expanded .chevron { transform: rotate(0deg); }
.v-enter-active, .v-leave-active { transition: opacity .16s ease, transform .16s ease; }
.v-enter-from, .v-leave-to { opacity: 0; transform: translateY(-6px) scale(.985); }
.pop-up .v-enter-from, .pop-up .v-leave-to { opacity: 0; transform: translateY(6px) scale(.985); }
@media (prefers-reduced-motion: reduce) { .v-enter-active, .v-leave-active, .island { transition: none; } }
</style>
