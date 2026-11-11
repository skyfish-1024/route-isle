<script setup>
import { Calendar, ChevronLeft, ChevronRight } from '@lucide/vue'
import { computed, ref } from 'vue'
import { PopoverContent, PopoverPortal, PopoverRoot, PopoverTrigger } from 'reka-ui'

const model = defineModel({ type: String, default: '' })
defineProps({ label: String, theme: String })

const open = ref(false)
const weekdays = ['一', '二', '三', '四', '五', '六', '日']

function formatYmd(year, month, day) {
  return `${year}-${String(month).padStart(2, '0')}-${String(day).padStart(2, '0')}`
}

function parseYmd(value) {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value || '')
  if (!match) return null
  return { year: Number(match[1]), month: Number(match[2]), day: Number(match[3]) }
}

const now = new Date()
const todayStr = formatYmd(now.getFullYear(), now.getMonth() + 1, now.getDate())
const viewYear = ref(now.getFullYear())
const viewMonth = ref(now.getMonth() + 1)

function onOpenChange(nextOpen) {
  open.value = nextOpen
  if (nextOpen) {
    const selected = parseYmd(model.value)
    const d = selected || { year: now.getFullYear(), month: now.getMonth() + 1 }
    viewYear.value = d.year
    viewMonth.value = d.month
  }
}

function shiftMonth(delta) {
  let month = viewMonth.value + delta
  let year = viewYear.value
  if (month < 1) {
    month = 12
    year -= 1
  } else if (month > 12) {
    month = 1
    year += 1
  }
  viewYear.value = year
  viewMonth.value = month
}

const calendarDays = computed(() => {
  const firstDay = new Date(viewYear.value, viewMonth.value - 1, 1)
  const startOffset = (firstDay.getDay() + 6) % 7
  const gridStart = new Date(viewYear.value, viewMonth.value - 1, 1 - startOffset)
  return Array.from({ length: 42 }, (_, index) => {
    const cellDate = new Date(gridStart.getFullYear(), gridStart.getMonth(), gridStart.getDate() + index)
    const year = cellDate.getFullYear()
    const month = cellDate.getMonth() + 1
    const day = cellDate.getDate()
    const dateStr = formatYmd(year, month, day)
    return {
      dateStr,
      day,
      inMonth: year === viewYear.value && month === viewMonth.value,
      isToday: dateStr === todayStr,
      isSelected: dateStr === model.value,
    }
  })
})

const displayText = computed(() => (parseYmd(model.value) ? model.value.replace(/-/g, '/') : 'yyyy/mm/dd'))

function selectDate(dateStr) {
  model.value = dateStr
  open.value = false
}
</script>

<template>
  <PopoverRoot :open="open" @update:open="onOpenChange">
    <PopoverTrigger as-child>
      <button type="button" class="date-picker" :class="{ 'is-placeholder': !parseYmd(model) }"
        :data-state="open ? 'open' : 'closed'" :aria-label="label">
        <span>{{ displayText }}</span>
        <Calendar :size="12" aria-hidden="true" />
      </button>
    </PopoverTrigger>
    <PopoverPortal>
      <PopoverContent as-child align="end" :side-offset="4" :collision-padding="8">
        <div class="date-popover" :data-theme="theme">
          <div class="date-header">
            <strong>{{ viewYear }}年{{ viewMonth }}月</strong>
            <div class="date-nav">
              <button type="button" aria-label="上个月" @click="shiftMonth(-1)">
                <ChevronLeft :size="13" aria-hidden="true" />
              </button>
              <button type="button" aria-label="下个月" @click="shiftMonth(1)">
                <ChevronRight :size="13" aria-hidden="true" />
              </button>
            </div>
          </div>
          <div class="date-grid date-weekdays" aria-hidden="true">
            <span v-for="day in weekdays" :key="day">{{ day }}</span>
          </div>
          <div class="date-grid" role="grid" :aria-label="`${viewYear}年${viewMonth}月`">
            <button v-for="cell in calendarDays" :key="cell.dateStr" type="button" class="date-cell"
              :class="{ muted: !cell.inMonth, today: cell.isToday, selected: cell.isSelected }"
              :aria-pressed="cell.isSelected" @click="selectDate(cell.dateStr)">
              {{ cell.day }}
            </button>
          </div>
          <div class="date-actions">
            <button type="button" class="date-action" :disabled="!model" @click="selectDate('')">清除</button>
            <button type="button" class="date-action accent" @click="selectDate(todayStr)">今天</button>
          </div>
        </div>
      </PopoverContent>
    </PopoverPortal>
  </PopoverRoot>
</template>

<style scoped>
.date-picker { display: inline-flex; align-items: center; justify-content: space-between; gap: 4px; box-sizing: border-box; width: 100%; min-width: 0; height: 24px; padding: 0 6px; border: 0; border-radius: 6px; background: var(--hover); color: var(--foreground); font: inherit; font-size: 11px; font-variant-numeric: tabular-nums; line-height: 1; transition: background-color .15s ease; }
.date-picker.is-placeholder > span { color: var(--muted); }
.date-picker > span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.date-picker svg { flex: none; color: var(--muted); }
.date-picker:hover { background: color-mix(in srgb, var(--foreground) 10%, transparent); }
.date-picker:focus, .date-picker:focus-visible, .date-picker[data-state="open"] { outline: none; background: color-mix(in srgb, var(--accent) 16%, var(--hover)); }
.date-popover { z-index: 25; box-sizing: border-box; width: 196px; max-height: calc(100vh - 16px); overflow-y: auto; padding: 6px 8px; border: 1px solid var(--edge); border-radius: 10px; background: var(--surface); color: var(--foreground); backdrop-filter: blur(24px) saturate(180%); box-shadow: 0 8px 24px #0004; font-size: 11px; }
.date-header { display: flex; align-items: center; justify-content: space-between; margin-bottom: 4px; padding-left: 2px; }
.date-header strong { font-size: 12px; font-weight: 500; }
.date-nav { display: flex; gap: 2px; }
.date-nav button { display: grid; place-items: center; width: 20px; height: 20px; padding: 0; border: 0; border-radius: 6px; background: transparent; color: var(--muted); }
.date-nav button:hover { background: var(--hover); color: var(--foreground); }
.date-nav button:focus-visible { outline: none; background: var(--hover); color: var(--foreground); }
.date-grid { display: grid; grid-template-columns: repeat(7, 1fr); gap: 1px; }
.date-weekdays { margin-bottom: 2px; color: var(--muted); font-size: 10px; text-align: center; }
.date-weekdays span { line-height: 15px; }
.date-cell { display: grid; place-items: center; height: 19px; padding: 0; border: 0; border-radius: 5px; background: transparent; color: inherit; font: inherit; font-variant-numeric: tabular-nums; }
.date-cell.muted { color: var(--muted); opacity: .45; }
.date-cell:hover { background: var(--hover); }
.date-cell:focus-visible { outline: none; background: var(--hover); }
.date-cell.today { color: var(--accent); font-weight: 600; }
.date-cell.selected { background: color-mix(in srgb, var(--accent) 22%, var(--selected)); color: var(--accent); font-weight: 600; }
.date-actions { display: flex; align-items: center; justify-content: space-between; margin-top: 4px; padding-top: 4px; border-top: 1px solid var(--edge); }
.date-action { padding: 2px 7px; border: 0; border-radius: 5px; background: transparent; color: var(--muted); font: inherit; font-size: 11px; }
.date-action:hover:not(:disabled) { background: var(--hover); color: var(--foreground); }
.date-action:disabled { opacity: .4; cursor: default; }
.date-action.accent { color: var(--accent); }
.date-action.accent:hover { background: color-mix(in srgb, var(--accent) 14%, transparent); }
.date-action:focus-visible { outline: none; background: var(--hover); }
</style>
