<script setup>
import { computed, ref } from 'vue'
import { formatBytes } from './traffic'

const props = defineProps({
  traffic: { type: Object, required: true },
})

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

const sampleTimeFormat = new Intl.DateTimeFormat('zh-CN', {
  year: 'numeric', month: '2-digit', day: '2-digit',
  hour: '2-digit', minute: '2-digit', second: '2-digit', hourCycle: 'h23',
})
</script>

<template>
  <section class="traffic-details" aria-label="实时流量统计">
    <header class="traffic-heading traffic-nav">
      <strong>本次运行</strong>
      <div class="stats-filters" role="group" aria-label="统计维度">
        <div class="dimensions">
          <button v-for="option in dimensions" :key="option.value" type="button"
            :aria-pressed="dimension === option.value"
            @click="dimension = option.value">{{ option.label }}</button>
        </div>
      </div>
    </header>
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
  </section>
</template>

<style scoped>
.traffic-details { display: flex; flex-direction: column; box-sizing: border-box; height: 100%; padding: 2px; }
.traffic-heading { display: flex; justify-content: space-between; align-items: center; gap: 8px; }
.traffic-heading strong { font-weight: 500; }
.traffic-heading > span { font-size: 11px; color: var(--muted); }
.traffic-nav { padding: 2px 0 6px; }
.traffic-nav > strong { font-size: 11px; color: var(--muted); font-weight: 400; }
.traffic-nav, .totals, .trend-heading { flex: none; }
.totals { display: grid; grid-template-columns: 1fr 1fr; gap: 6px; margin-top: 6px; }
.totals div { display: grid; gap: 4px; padding: 7px 9px; border-radius: 8px; background: var(--cell); }
.totals span { font-size: 11px; color: var(--muted); }
.totals strong { font-size: 16px; font-weight: 500; font-variant-numeric: tabular-nums; }
.trend-heading { margin-top: 10px; }
.trend-area { position: relative; flex: 1; min-height: 48px; margin-top: 5px; }
.trend { position: absolute; inset: 0; display: block; width: 100%; height: 100%; border-radius: 7px; background: var(--field); }
.trend-guide { stroke: var(--muted); stroke-dasharray: 2 2; }
.trend-tooltip { --tooltip-half: min(140px, 50%); position: absolute; z-index: 2; bottom: 100%; left: clamp(var(--tooltip-half), var(--sample-position), calc(100% - var(--tooltip-half))); transform: translateX(-50%); box-sizing: border-box; width: min(280px, 100%); border: 1px solid var(--edge); border-radius: 8px; background: var(--surface); backdrop-filter: blur(18px); box-shadow: 0 4px 14px #0002; font-size: 11px; font-variant-numeric: tabular-nums; }
.sample-time { padding: 6px 8px; color: var(--muted); border-bottom: 1px solid var(--edge); white-space: nowrap; }
.sample-scroll { max-height: 104px; overflow-y: auto; padding: 2px 8px; }
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
.stats-filters { display: flex; align-items: center; gap: 5px; margin-left: auto; }
.dimensions { display: flex; gap: 2px; }
.dimensions button { padding: 4px 6px; border: 0; border-radius: 6px; background: transparent; color: var(--muted); font-size: 11px; }
.dimensions button[aria-pressed="true"] { color: var(--foreground); background: var(--selected); }
</style>
