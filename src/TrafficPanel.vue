<script setup>
import { computed, ref } from 'vue'
import { formatBytes } from './traffic'
import { exitNameFor } from './services/routeMatcher'
import PanelDatePicker from './PanelDatePicker.vue'
import PanelSelect from './PanelSelect.vue'

const props = defineProps({
  traffic: { type: Object, required: true },
  sessions: { type: Array, required: true },
  rules: { type: Object, default: () => ({ groups: [], upstreams: [] }) },
  theme: { type: String, required: true },
})
const view = ref('live')
const logSession = ref(null)
const logQuery = ref('')
const logExit = ref('all')
const logResult = ref('all')
const logDate = ref('')
const historyDate = ref('')
const historyExit = ref('all')
const historyRule = ref('all')

function isOnDate(timestamp, date) {
  if (!date) return true
  const start = new Date(date + 'T00:00:00')
  const end = new Date(start)
  end.setDate(end.getDate() + 1)
  return timestamp >= start.getTime() && timestamp < end.getTime()
}

const datedSessions = computed(() => props.sessions.filter(session => isOnDate(session.startedAt, historyDate.value)))
function collectGroupNames(sessions, key, initialNames = []) {
  const names = new Set(initialNames)
  for (const session of sessions) {
    for (const group of Object.values(session[key] || {})) {
      names.add(group?.name || '未命名')
    }
  }
  return Array.from(names)
}
const historyExitOptions = computed(() => {
  const configured = [
    exitNameFor('system'),
    exitNameFor('wlan'),
    ...(props.rules?.upstreams || []).map(upstream => exitNameFor(upstream.id, props.rules.upstreams)),
    exitNameFor('reject'),
  ]
  return [
    { id: 'all', name: '出口' },
    ...collectGroupNames(props.sessions, 'byExit', configured).map(name => ({ id: 'exit:' + name, name })),
  ]
})
const historyRuleOptions = computed(() => {
  const configured = [
    ...(props.rules?.groups || []).map(group => group.name?.trim() || '未命名规则组'),
    '未匹配',
  ]
  return [
    { id: 'all', name: '规则' },
    ...collectGroupNames(props.sessions, 'byRule', configured).map(name => ({ id: 'rule:' + name, name })),
  ]
})
function sumMatchingBuckets(buckets, selectedName) {
  let upload = 0
  let download = 0
  let matched = false
  for (const group of Object.values(buckets || {})) {
    if ((group?.name || '未命名') !== selectedName) continue
    upload += group.upload || 0
    download += group.download || 0
    if ((group.upload || 0) > 0 || (group.download || 0) > 0) matched = true
  }
  return { upload, download, matched }
}

function sessionTrafficForFilter(session) {
  const exitName = historyExit.value === 'all' ? null : historyExit.value.slice('exit:'.length)
  const ruleName = historyRule.value === 'all' ? null : historyRule.value.slice('rule:'.length)

  if (!exitName && !ruleName) {
    return { upload: session.upload || 0, download: session.download || 0, matched: true }
  }

  if (exitName && ruleName) {
    if (session.byPair) {
      let upload = 0
      let download = 0
      let matched = false
      for (const pair of Object.values(session.byPair)) {
        if ((pair?.exit || '未命名') !== exitName || (pair?.rule || '未命名') !== ruleName) continue
        upload += pair.upload || 0
        download += pair.download || 0
        if ((pair.upload || 0) > 0 || (pair.download || 0) > 0) matched = true
      }
      return { upload, download, matched }
    }
    // Fallback for sessions saved before byPair tracking: check logs for an unambiguous mapping.
    const ruleTotals = sumMatchingBuckets(session.byRule, ruleName)
    const exitTotals = sumMatchingBuckets(session.byExit, exitName)
    if (!ruleTotals.matched || !exitTotals.matched) return { upload: 0, download: 0, matched: false }
    const logs = session.logs || []
    const ruleExits = new Set(logs.filter(l => (l.rule || '未命名') === ruleName).map(l => l.exit || '未命名'))
    if (ruleExits.size === 1 && ruleExits.has(exitName)) return ruleTotals
    const exitRules = new Set(logs.filter(l => (l.exit || '未命名') === exitName).map(l => l.rule || '未命名'))
    if (exitRules.size === 1 && exitRules.has(ruleName)) return exitTotals
    return { upload: 0, download: 0, matched: false }
  }

  return exitName
    ? sumMatchingBuckets(session.byExit, exitName)
    : sumMatchingBuckets(session.byRule, ruleName)
}

const filteredSessions = computed(() => datedSessions.value.flatMap(session => {
  const { upload, download, matched } = sessionTrafficForFilter(session)
  return matched ? [{ session, upload, download }] : []
}))
const logs = computed(() => logSession.value ? logSession.value.logs || [] : props.traffic.logs)
const exitOptions = computed(() => [
  { id: 'all', name: '出口' },
  ...Array.from(new Set(logs.value.map(log => log.exit)), name => ({ id: 'exit:' + name, name })),
])
const resultOptions = [
  { id: 'all', name: '结果' },
  { id: 'success', name: '成功' },
  { id: 'failed', name: '失败' },
]
const filteredLogs = computed(() => {
  const query = logQuery.value.trim().toLowerCase()
  return logs.value.filter(log =>
    isOnDate(log.timestamp, logDate.value)
    && log.host.toLowerCase().includes(query)
    && (logExit.value === 'all' || 'exit:' + log.exit === logExit.value)
    && (logResult.value === 'all' || log.failed === (logResult.value === 'failed')),
  ).reverse()
})
function openLogs(session = null) {
  logSession.value = session
  logQuery.value = ''
  logExit.value = 'all'
  logResult.value = 'all'
  hoverIndex.value = null
  view.value = 'logs'
}
const dimension = ref('all')
const dimensions = [
  { value: 'all', label: '全部' },
  { value: 'exit', label: '出口' },
  { value: 'rule', label: '规则' },
]
const hoverIndex = ref(null)
const hoveredSample = computed(() => props.traffic.history[hoverIndex.value])
const sampleX = (index) => 3 + index * 6
const sampleY = (value) => 46 - value / Math.max(1, peak.value) * 42

function inspectSample(event) {
  const bounds = event.currentTarget.getBoundingClientRect()
  const index = Math.floor((event.clientX - bounds.left) / bounds.width * 60)
  hoverIndex.value = index >= 0 && index < props.traffic.history.length ? index : null
}
const seriesColors = ['#34d399', '#818cf8', '#e5a963', '#63bcd5', '#d994bd', '#a7b966']
const grouped = computed(() => dimension.value !== 'all')
const groupKey = computed(() => dimension.value === 'exit' ? 'byExit' : 'byRule')
const series = computed(() => {
  if (!grouped.value) return [
    { id: 'upload', name: '上传', color: '#818cf8' },
    { id: 'download', name: '下载', color: 'var(--accent)' },
  ]
  const names = new Map()
  for (const sample of props.traffic.history) {
    for (const [id, row] of Object.entries(sample[groupKey.value] || {})) names.set(id, row.name)
  }
  return Array.from(names, ([id, name], index) => ({ id, name, color: seriesColors[index % seriesColors.length] }))
})
function sampleRate(sample, id) {
  if (!grouped.value) return sample[id]
  const row = sample[groupKey.value]?.[id]
  return row ? row.upload + row.download : 0
}
const peak = computed(() => Math.max(0, ...props.traffic.history.flatMap(sample => series.value.map(item => sampleRate(sample, item.id)))))
const hoveredRows = computed(() => series.value.flatMap(item => {
  const row = hoveredSample.value?.[groupKey.value]?.[item.id]
  return row ? [{ ...item, ...row }] : []
}))
const curves = computed(() => {
  const samples = props.traffic.history
  return series.value.map(item => {
    const points = samples.map((sample, index) => [
      sampleX(index),
      sampleY(sampleRate(sample, item.id)),
    ])
    if (points.length === 1) {
      const [x, y] = points[0]
      return { ...item, path: `M ${x - 2} ${y} H ${x + 2}` }
    }
    const path = points.map(([x, y], index) => {
      if (index === 0) return `M ${x} ${y}`
      const [previousX, previousY] = points[index - 1]
      const midX = (previousX + x) / 2
      return `C ${midX} ${previousY}, ${midX} ${y}, ${x} ${y}`
    }).join(' ')
    return { ...item, path }
  })
})
const dateFormat = new Intl.DateTimeFormat('zh-CN', { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' })
const sampleTimeFormat = new Intl.DateTimeFormat('zh-CN', {
  year: 'numeric', month: '2-digit', day: '2-digit',
  hour: '2-digit', minute: '2-digit', second: '2-digit', hourCycle: 'h23',
})
</script>

<template>
  <section class="traffic-details" aria-label="流量统计">
    <header class="traffic-heading traffic-nav">
      <div class="dimensions" role="group" aria-label="统计页面">
        <button type="button" :aria-pressed="view === 'live'" @click="view = 'live'">当前</button>
        <button type="button" :aria-pressed="view === 'history'" @click="view = 'history'; hoverIndex = null">历史</button>
        <button type="button" :aria-pressed="view === 'logs'" @click="openLogs()">日志</button>
      </div>
      <div v-if="view === 'live'" class="stats-filters" role="group" aria-label="统计筛选">
        <div class="dimensions">
          <button v-for="option in dimensions" :key="option.value" type="button"
            :aria-pressed="dimension === option.value"
            @click="dimension = option.value">{{ option.label }}</button>
        </div>
      </div>
      <div v-else-if="view === 'history'" class="history-filters" role="group" aria-label="历史筛选">
        <PanelDatePicker v-model="historyDate" :theme="theme" label="历史日期" />
        <PanelSelect v-model="historyExit" :options="historyExitOptions" :theme="theme" label="筛选出口" />
        <PanelSelect v-model="historyRule" :options="historyRuleOptions" :theme="theme" label="筛选规则" />
      </div>
      <div v-else-if="view === 'logs'" class="log-filters" role="group" aria-label="筛选日志">
        <input v-model="logQuery" type="search" placeholder="搜索域名" aria-label="搜索域名">
        <PanelDatePicker v-model="logDate" :theme="theme" label="日志日期" />
        <PanelSelect v-model="logExit" :options="exitOptions" :theme="theme" label="筛选出口" />
        <PanelSelect v-model="logResult" :options="resultOptions" :theme="theme" label="筛选结果" />
      </div>
    </header>
    <template v-if="view === 'live'">
    <div class="totals">
      <div><span>上传</span><strong>{{ formatBytes(traffic.upload) }}</strong></div>
      <div><span>下载</span><strong>{{ formatBytes(traffic.download) }}</strong></div>
    </div>
    <div class="traffic-heading trend-heading">
      <strong>最近 60 秒</strong>
      <span v-if="grouped">{{ dimension === 'exit' ? '各出口' : '各规则组' }} · 上传 + 下载</span>
      <span v-else><span class="upload-key">●</span> 上传 · <span class="download-key">●</span> 下载</span>
    </div>
    <div class="trend-area" @pointermove="inspectSample" @pointerleave="hoverIndex = null" @pointercancel="hoverIndex = null">
      <svg class="trend" viewBox="0 0 360 50" preserveAspectRatio="none" role="img"
        :aria-label="`最近 60 秒流量趋势，峰值 ${formatBytes(peak)}/s`">
        <path class="grid-line" d="M 0 24 H 360 M 0 46 H 360" vector-effect="non-scaling-stroke" />
        <path v-for="curve in curves" :key="curve.id" :stroke="curve.color" :d="curve.path" vector-effect="non-scaling-stroke" />
        <line v-if="hoveredSample" class="trend-guide" :x1="sampleX(hoverIndex)" :x2="sampleX(hoverIndex)" y1="2" y2="48" vector-effect="non-scaling-stroke" />
      </svg>
      <Transition name="sample-tooltip">
        <div v-if="hoveredSample" class="trend-tooltip"
          :style="{ '--sample-position': sampleX(hoverIndex) / 3.6 + '%' }" @pointermove.stop>
          <header class="sample-time">{{ hoveredSample.timestamp ? sampleTimeFormat.format(hoveredSample.timestamp) : '采样时间未知' }}</header>
          <div class="sample-scroll">
            <template v-if="grouped">
              <div v-for="row in hoveredRows" :key="row.id" class="sample-detail">
                <span class="sample-name" :title="row.name"><i :style="{ color: row.color }">●</i> {{ row.name }}</span>
                <span class="sample-rates"><span>↑ {{ formatBytes(row.upload) }}/s</span><span>↓ {{ formatBytes(row.download) }}/s</span></span>
              </div>
            </template>
            <template v-else>
              <div class="sample-detail"><span><i class="upload-key">●</i> 上传</span><span class="sample-rates">{{ formatBytes(hoveredSample.upload) }}/s</span></div>
              <div class="sample-detail"><span><i class="download-key">●</i> 下载</span><span class="sample-rates">{{ formatBytes(hoveredSample.download) }}/s</span></div>
            </template>
          </div>
        </div>
      </Transition>
    </div>
    </template>
    <div v-else-if="view === 'history'" class="traffic-scroll history-list" role="region" aria-label="运行历史" tabindex="0">
      <div v-for="{ session, upload, download } in filteredSessions" :key="session.startedAt" class="history-row">
        <div class="traffic-row history-summary">
          <span>{{ dateFormat.format(session.startedAt) }}</span>
          <span>↑ {{ formatBytes(upload) }} · ↓ {{ formatBytes(download) }}</span>
          <button type="button" class="log-link" @click="openLogs(session)">查看日志</button>
        </div>
      </div>
      <p v-if="!filteredSessions.length" class="traffic-empty">
        {{ !sessions.length ? '停止一次有连接记录的运行后，历史会显示在这里。' : (!datedSessions.length ? '该日期没有运行记录。' : '没有符合筛选条件的运行记录。') }}
      </p>
    </div>
    <template v-else>
      <div class="log-heading">
        <span>{{ logSession ? dateFormat.format(logSession.startedAt) + ' 的日志' : '当前运行日志' }}</span>
        <span>{{ filteredLogs.length }} / {{ logs.length }} 条</span>
      </div>
      <div class="traffic-scroll history-list" role="region" aria-label="代理日志" tabindex="0">
        <article v-for="(log, index) in filteredLogs" :key="log.timestamp + '-' + index" class="log-entry">
          <div><strong :title="log.host">{{ log.host }}</strong><span :class="{ failed: log.failed }">{{ log.failed ? log.reason : '成功' }}</span></div>
          <div><span>{{ log.rule }} → {{ log.exit }}</span><time>{{ sampleTimeFormat.format(log.timestamp) }}</time></div>
        </article>
        <p v-if="!filteredLogs.length" class="traffic-empty">{{ logs.length ? '没有符合筛选条件的日志。' : '没有连接日志。' }}</p>
      </div>
    </template>
  </section>
</template>

<style scoped>
.traffic-details { display: flex; flex-direction: column; box-sizing: border-box; height: 100%; padding: 2px; }
.traffic-scroll { overflow: auto; scrollbar-width: thin; scrollbar-color: color-mix(in srgb, var(--muted) 45%, transparent) transparent; }
.history-list { display: flex; flex-direction: column; flex: 1; min-height: 0; }
@supports selector(::-webkit-scrollbar) {
  .traffic-scroll { scrollbar-width: auto; scrollbar-color: auto; }
  .traffic-scroll::-webkit-scrollbar { width: 4px; }
  .traffic-scroll::-webkit-scrollbar-track { background: transparent; }
  .traffic-scroll::-webkit-scrollbar-thumb { border-radius: 999px; background: color-mix(in srgb, var(--muted) 30%, transparent); }
  .traffic-scroll:hover::-webkit-scrollbar-thumb { background: color-mix(in srgb, var(--muted) 55%, transparent); }
  .traffic-scroll::-webkit-scrollbar-button { display: none; }
}
.traffic-heading, .traffic-row { display: flex; justify-content: space-between; align-items: center; gap: 8px; }
.traffic-heading strong { font-weight: 500; }
.traffic-heading > span, .traffic-empty { font-size: 11px; color: var(--muted); }
.totals { display: grid; grid-template-columns: 1fr 1fr; gap: 6px; margin-top: 6px; }
.totals div { display: grid; gap: 4px; padding: 7px 9px; border-radius: 8px; background: var(--cell); }
.totals span { font-size: 11px; color: var(--muted); }
.totals strong { font-size: 16px; font-weight: 500; font-variant-numeric: tabular-nums; }
.trend-heading { margin-top: 10px; }
.traffic-nav { padding: 2px 0 6px; }
.traffic-nav, .totals, .trend-heading { flex: none; }
.trend-area { position: relative; flex: 1; min-height: 48px; margin-top: 5px; }
.trend { position: absolute; inset: 0; display: block; width: 100%; height: 100%; border-radius: 7px; background: var(--field); }
.trend-guide { stroke: var(--muted); stroke-dasharray: 2 2; }
.trend-tooltip { --tooltip-half: min(140px, 50%); position: absolute; z-index: 2; bottom: 100%; left: clamp(var(--tooltip-half), var(--sample-position), calc(100% - var(--tooltip-half))); transform: translateX(-50%); box-sizing: border-box; width: min(280px, 100%); border: 1px solid var(--edge); border-radius: 8px; background: var(--surface); backdrop-filter: blur(18px); box-shadow: 0 4px 14px #0002; font-size: 11px; font-variant-numeric: tabular-nums; }
.sample-time { padding: 6px 8px; color: var(--muted); border-bottom: 1px solid var(--edge); white-space: nowrap; }
.sample-scroll { max-height: 104px; overflow-y: auto; scrollbar-width: thin; padding: 2px 8px; }
.trend-tooltip i { font-style: normal; }
.sample-detail { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 4px 0; }
.sample-detail + .sample-detail { border-top: 1px solid var(--edge); }
.sample-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.sample-rates { display: flex; flex: none; gap: 8px; justify-content: flex-end; white-space: nowrap; color: var(--muted); }
@media (prefers-reduced-motion: no-preference) {
  .sample-tooltip-enter-active, .sample-tooltip-leave-active { transition: opacity 140ms ease, translate 140ms ease; }
  .sample-tooltip-enter-from, .sample-tooltip-leave-to { opacity: 0; translate: 0 4px; }
}
.sample-tooltip-leave-active { pointer-events: none; }
.trend path { fill: none; stroke-width: 1.5; stroke-linecap: round; }
.grid-line { stroke: var(--edge); }
.upload-key { color: #818cf8; }
.download-key { color: var(--accent); }
.dimensions { display: flex; gap: 2px; }
.dimensions button { padding: 4px 6px; border: 0; border-radius: 6px; background: transparent; color: var(--muted); font-size: 11px; }
.dimensions button[aria-pressed="true"] { color: var(--foreground); background: var(--selected); }
.traffic-row { min-height: 28px; padding: 0 6px; font-size: 11px; }
.traffic-row > span:first-child { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.traffic-row > span:last-child { flex: none; color: var(--muted); font-variant-numeric: tabular-nums; }
.traffic-empty { margin: auto; padding: 12px; text-align: center; }
.history-row { flex: none; margin-top: 4px; border-radius: 6px; background: var(--cell); }
.history-summary > span:nth-child(2) { margin-left: auto; color: var(--muted); white-space: nowrap; }
.log-link { flex: none; border: 0; background: transparent; color: var(--accent); font: inherit; }
.log-heading { display: flex; justify-content: space-between; gap: 6px; margin: 4px 0; color: var(--muted); font-size: 11px; }
.traffic-nav { flex-wrap: wrap; gap: 6px 12px; }
.traffic-nav .dimensions { flex: none; }
.stats-filters { display: flex; align-items: center; gap: 5px; margin-left: auto; }
.history-filters { display: grid; grid-template-columns: 94px minmax(62px, 1fr) minmax(62px, 1fr); gap: 4px; flex: 0 1 236px; margin-left: auto; align-items: center; }
.log-filters { display: grid; grid-template-columns: minmax(68px, 1fr) 94px 54px 54px; gap: 4px; flex: 1 1 280px; margin-left: auto; align-items: center; }
.log-filters input { box-sizing: border-box; min-width: 0; height: 24px; padding: 0 7px; border: 0; border-radius: 6px; background: var(--hover); color: var(--foreground); font: inherit; font-size: 11px; line-height: 24px; transition: background-color .15s ease; }
.log-filters input:hover { background: color-mix(in srgb, var(--foreground) 10%, transparent); }
.log-filters input:focus, .log-filters input:focus-visible { outline: none; background: color-mix(in srgb, var(--accent) 16%, var(--hover)); }
.history-filters :deep(.picker), .log-filters :deep(.picker) { gap: 3px; height: 24px; padding: 0 6px; }
@media (max-width: 365px) {
  .history-filters { grid-template-columns: 1fr 1fr; }
  .history-filters > :first-child { grid-column: 1 / -1; }
  .log-filters { grid-template-columns: minmax(68px, 1fr) 94px; }
}
.history-filters :deep(.picker > span:first-child), .log-filters :deep(.picker > span:first-child) { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.history-filters :deep(.picker svg), .log-filters :deep(.picker svg) { flex: none; }
.log-entry { flex: none; padding: 5px 6px; margin-bottom: 4px; border-radius: 6px; background: var(--cell); font-size: 11px; }
.log-entry > div { display: flex; align-items: baseline; justify-content: space-between; gap: 6px; }
.log-entry > div + div { margin-top: 2px; color: var(--muted); font-size: 10px; }
.log-entry strong, .log-entry > div + div > span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.log-entry time, .log-entry > div:first-child > span { flex: none; }
.log-entry > div:first-child > span { color: var(--accent); }
.log-entry > div:first-child > .failed { color: #e8657b; }
</style>
