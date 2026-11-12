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

const subView = ref('trend')
const trendRange = ref('7d')
const trendRangeOptions = [
  { id: '7d', name: '近 7 天' },
  { id: '30d', name: '近 30 天' },
  { id: 'all', name: '全部' },
]

const logSession = ref(null)
const logQuery = ref('')
const logExit = ref('all')
const logResult = ref('all')
const logDate = ref('')
const historyDate = ref('')
const historyExit = ref('all')
const historyRule = ref('all')

function dateRange(date) {
  if (!date) return null
  const start = new Date(date + 'T00:00:00')
  const end = new Date(start)
  end.setDate(end.getDate() + 1)
  return [start.getTime(), end.getTime()]
}

function dayKeyOf(timestamp) {
  const d = new Date(timestamp)
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  return `${y}-${m}-${day}`
}

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
    ...(props.rules?.cnFallback && props.rules?.cnFallback !== props.rules?.fallback ? ['GeoIP 国内直连'] : []),
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

const datedSessions = computed(() => {
  const range = dateRange(historyDate.value)
  if (!range) return props.sessions
  const [startMs, endMs] = range
  return props.sessions.filter(session => session.startedAt >= startMs && session.startedAt < endMs)
})

const filteredSessions = computed(() => datedSessions.value.flatMap(session => {
  const { upload, download, matched } = sessionTrafficForFilter(session)
  return matched ? [{ session, upload, download }] : []
}))

const trendBuckets = computed(() => {
  const today = new Date()
  today.setHours(0, 0, 0, 0)
  let startMs = 0
  if (trendRange.value === '7d') {
    const start = new Date(today)
    start.setDate(start.getDate() - 6)
    startMs = start.getTime()
  } else if (trendRange.value === '30d') {
    const start = new Date(today)
    start.setDate(start.getDate() - 29)
    startMs = start.getTime()
  }

  const byDay = new Map()
  let totalUpload = 0
  let totalDownload = 0
  let sessionCount = 0
  let minDayMs = Infinity

  for (const session of props.sessions) {
    if (session.startedAt < startMs) continue
    const { upload, download, matched } = sessionTrafficForFilter(session)
    if (!matched) continue
    const key = dayKeyOf(session.startedAt)
    const d = new Date(key + 'T00:00:00')
    const ms = d.getTime()
    if (ms < minDayMs) minDayMs = ms
    const existing = byDay.get(key) || { date: key, upload: 0, download: 0, count: 0 }
    existing.upload += upload
    existing.download += download
    existing.count += 1
    byDay.set(key, existing)
    totalUpload += upload
    totalDownload += download
    sessionCount += 1
  }

  if (sessionCount === 0) {
    return { points: [], totalUpload: 0, totalDownload: 0, sessionCount: 0 }
  }

  const points = []
  const cursor = new Date(trendRange.value === 'all' ? minDayMs : startMs)
  const endCursor = today.getTime()
  while (cursor.getTime() <= endCursor) {
    const key = dayKeyOf(cursor.getTime())
    const row = byDay.get(key) || { date: key, upload: 0, download: 0, count: 0 }
    points.push(row)
    cursor.setDate(cursor.getDate() + 1)
  }

  return { points, totalUpload, totalDownload, sessionCount }
})

const hoverIndex = ref(null)
const trendPeak = computed(() => Math.max(
  1,
  ...trendBuckets.value.points.flatMap(point => [point.upload, point.download]),
))

const pointX = (index, total) => {
  if (total <= 1) return 180
  return 6 + (index / (total - 1)) * 348
}
const pointY = (value) => 46 - (value / trendPeak.value) * 40

function buildCurvePath(points, accessor) {
  if (!points.length) return ''
  const coords = points.map((p, index) => [pointX(index, points.length), pointY(accessor(p))])
  if (coords.length === 1) {
    const [x, y] = coords[0]
    return `M ${x - 6} ${y} H ${x + 6}`
  }
  return coords.map(([x, y], index) => {
    if (index === 0) return `M ${x} ${y}`
    const [px, py] = coords[index - 1]
    const midX = (px + x) / 2
    return `C ${midX} ${py}, ${midX} ${y}, ${x} ${y}`
  }).join(' ')
}

const trendCurves = computed(() => {
  const pts = trendBuckets.value.points
  if (!pts.length) return []
  return [
    { id: 'upload', color: '#818cf8', path: buildCurvePath(pts, p => p.upload) },
    { id: 'download', color: 'var(--accent)', path: buildCurvePath(pts, p => p.download) },
  ]
})

const hoveredPoint = computed(() => {
  const pts = trendBuckets.value.points
  if (hoverIndex.value === null || hoverIndex.value < 0 || hoverIndex.value >= pts.length) return null
  return pts[hoverIndex.value]
})

function inspectTrend(event) {
  const pts = trendBuckets.value.points
  if (!pts.length) {
    hoverIndex.value = null
    return
  }
  const bounds = event.currentTarget.getBoundingClientRect()
  const ratio = Math.min(1, Math.max(0, (event.clientX - bounds.left) / Math.max(1, bounds.width)))
  const index = pts.length === 1 ? 0 : Math.round(ratio * (pts.length - 1))
  hoverIndex.value = index
}

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
  const range = dateRange(logDate.value)
  return logs.value.filter(log =>
    (!range || (log.timestamp >= range[0] && log.timestamp < range[1]))
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
  subView.value = 'logs'
}

const dateFormat = new Intl.DateTimeFormat('zh-CN', { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' })
const sampleTimeFormat = new Intl.DateTimeFormat('zh-CN', {
  year: 'numeric', month: '2-digit', day: '2-digit',
  hour: '2-digit', minute: '2-digit', second: '2-digit', hourCycle: 'h23',
})
</script>

<template>
  <section class="traffic-details" aria-label="历史统计">
    <header class="traffic-heading traffic-nav">
      <div class="dimensions" role="group" aria-label="统计子页">
        <button type="button" :aria-pressed="subView === 'trend'" @click="subView = 'trend'; hoverIndex = null">趋势</button>
        <button type="button" :aria-pressed="subView === 'history'" @click="subView = 'history'; hoverIndex = null">历史</button>
        <button type="button" :aria-pressed="subView === 'logs'" @click="openLogs()">日志</button>
      </div>
      <div v-if="subView === 'trend'" class="history-filters trend-filters" role="group" aria-label="趋势筛选">
        <PanelSelect v-model="trendRange" :options="trendRangeOptions" :theme="theme" label="时间范围" />
        <PanelSelect v-model="historyExit" :options="historyExitOptions" :theme="theme" label="筛选出口" />
        <PanelSelect v-model="historyRule" :options="historyRuleOptions" :theme="theme" label="筛选规则" />
      </div>
      <div v-else-if="subView === 'history'" class="history-filters" role="group" aria-label="历史筛选">
        <PanelDatePicker v-model="historyDate" :theme="theme" label="历史日期" />
        <PanelSelect v-model="historyExit" :options="historyExitOptions" :theme="theme" label="筛选出口" />
        <PanelSelect v-model="historyRule" :options="historyRuleOptions" :theme="theme" label="筛选规则" />
      </div>
      <div v-else class="log-filters" role="group" aria-label="筛选日志">
        <input v-model="logQuery" type="search" placeholder="搜索域名" aria-label="搜索域名">
        <PanelDatePicker v-model="logDate" :theme="theme" label="日志日期" />
        <PanelSelect v-model="logExit" :options="exitOptions" :theme="theme" label="筛选出口" />
        <PanelSelect v-model="logResult" :options="resultOptions" :theme="theme" label="筛选结果" />
      </div>
    </header>

    <template v-if="subView === 'trend'">
      <div class="totals">
        <div><span>累计上传</span><strong>{{ formatBytes(trendBuckets.totalUpload) }}</strong></div>
        <div><span>累计下载</span><strong>{{ formatBytes(trendBuckets.totalDownload) }}</strong></div>
      </div>
      <div class="traffic-heading trend-heading">
        <strong>每日流量趋势 · {{ trendBuckets.sessionCount }} 次运行</strong>
        <span><span class="upload-key">●</span> 上传 · <span class="download-key">●</span> 下载</span>
      </div>
      <div v-if="trendBuckets.points.length" class="trend-area"
        @pointermove="inspectTrend" @pointerleave="hoverIndex = null" @pointercancel="hoverIndex = null">
        <svg class="trend" viewBox="0 0 360 50" preserveAspectRatio="none" role="img"
          :aria-label="`历史流量趋势，峰值 ${formatBytes(trendPeak)}`">
          <path class="grid-line" d="M 0 24 H 360 M 0 46 H 360" vector-effect="non-scaling-stroke" />
          <path v-for="curve in trendCurves" :key="curve.id" :stroke="curve.color" :d="curve.path" vector-effect="non-scaling-stroke" />
          <line v-if="hoveredPoint" class="trend-guide"
            :x1="pointX(hoverIndex, trendBuckets.points.length)"
            :x2="pointX(hoverIndex, trendBuckets.points.length)"
            y1="2" y2="48" vector-effect="non-scaling-stroke" />
        </svg>
        <Transition name="sample-tooltip">
          <div v-if="hoveredPoint" class="trend-tooltip"
            :style="{ '--sample-position': (pointX(hoverIndex, trendBuckets.points.length) / 3.6) + '%' }" @pointermove.stop>
            <header class="sample-time">{{ hoveredPoint.date.replace(/-/g, '/') }} · {{ hoveredPoint.count }} 次运行</header>
            <div class="sample-scroll">
              <div class="sample-detail"><span><i class="upload-key">●</i> 上传</span><span class="sample-rates">{{ formatBytes(hoveredPoint.upload) }}</span></div>
              <div class="sample-detail"><span><i class="download-key">●</i> 下载</span><span class="sample-rates">{{ formatBytes(hoveredPoint.download) }}</span></div>
            </div>
          </div>
        </Transition>
      </div>
      <p v-else class="traffic-empty">
        {{ !sessions.length ? '停止一次有连接记录的运行后，趋势统计会显示在这里。' : '当前筛选条件下没有历史流量记录。' }}
      </p>
    </template>

    <div v-else-if="subView === 'history'" class="traffic-scroll history-list" role="region" aria-label="运行历史" tabindex="0">
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
        <article v-for="(log, index) in filteredLogs" :key="log.id || (log.timestamp + '-' + index)" class="log-entry">
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
.traffic-nav { padding: 2px 0 6px; flex-wrap: wrap; gap: 6px 12px; }
.traffic-nav, .totals, .trend-heading { flex: none; }
.traffic-nav .dimensions { flex: none; }
.trend-area { position: relative; flex: 1; min-height: 48px; margin-top: 5px; }
.trend { position: absolute; inset: 0; display: block; width: 100%; height: 100%; border-radius: 7px; background: var(--field); }
.trend-guide { stroke: var(--muted); stroke-dasharray: 2 2; }
.trend-tooltip { --tooltip-half: min(120px, 50%); position: absolute; z-index: 2; bottom: 100%; left: clamp(var(--tooltip-half), var(--sample-position), calc(100% - var(--tooltip-half))); transform: translateX(-50%); box-sizing: border-box; width: min(240px, 100%); border: 1px solid var(--edge); border-radius: 8px; background: var(--surface); backdrop-filter: blur(18px); box-shadow: 0 4px 14px #0002; font-size: 11px; font-variant-numeric: tabular-nums; }
.sample-time { padding: 6px 8px; color: var(--muted); border-bottom: 1px solid var(--edge); white-space: nowrap; }
.sample-scroll { padding: 2px 8px; }
.trend-tooltip i { font-style: normal; }
.sample-detail { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 4px 0; }
.sample-detail + .sample-detail { border-top: 1px solid var(--edge); }
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
.history-filters { display: grid; grid-template-columns: 94px minmax(62px, 1fr) minmax(62px, 1fr); gap: 4px; flex: 0 1 236px; margin-left: auto; align-items: center; }
.trend-filters { grid-template-columns: 78px minmax(62px, 1fr) minmax(62px, 1fr); flex: 0 1 224px; }
.log-filters { display: grid; grid-template-columns: minmax(68px, 1fr) 94px 54px 54px; gap: 4px; flex: 1 1 280px; margin-left: auto; align-items: center; }
.log-filters input { box-sizing: border-box; min-width: 0; height: 24px; padding: 0 7px; border: 0; border-radius: 6px; background: var(--hover); color: var(--foreground); font: inherit; font-size: 11px; line-height: 24px; transition: background-color .15s ease; }
.log-filters input:hover { background: color-mix(in srgb, var(--foreground) 10%, transparent); }
.log-filters input:focus, .log-filters input:focus-visible { outline: none; background: color-mix(in srgb, var(--accent) 16%, var(--hover)); }
.history-filters :deep(.picker), .log-filters :deep(.picker) { gap: 3px; height: 24px; padding: 0 6px; }
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
