<script setup>
import { Check, ChevronDown } from '@lucide/vue'
import { SelectContent, SelectItem, SelectItemIndicator, SelectItemText, SelectPortal, SelectRoot, SelectTrigger, SelectValue, SelectViewport } from 'reka-ui'

const model = defineModel({ type: String })
defineProps({ options: Array, label: String, theme: String })
</script>

<template>
  <SelectRoot v-model="model">
    <SelectTrigger class="picker" :aria-label="label">
      <SelectValue />
      <ChevronDown :size="13" aria-hidden="true" />
    </SelectTrigger>
    <SelectPortal>
      <SelectContent as-child position="popper" :side-offset="5">
        <div class="picker-menu" :data-theme="theme">
          <SelectViewport class="picker-viewport">
            <SelectItem v-for="option in options" :key="option.id" :value="option.id" class="picker-option" :title="option.name || '未命名上游'">
              <SelectItemText>{{ option.name || '未命名上游' }}</SelectItemText>
              <SelectItemIndicator><Check :size="13" aria-hidden="true" /></SelectItemIndicator>
            </SelectItem>
          </SelectViewport>
        </div>
      </SelectContent>
    </SelectPortal>
  </SelectRoot>
</template>

<style scoped>
.picker { display: inline-flex; align-items: center; justify-content: space-between; gap: 6px; box-sizing: border-box; min-width: 0; max-width: 100%; height: 24px; padding: 0 7px; border: 0; border-radius: 6px; background: var(--hover); color: inherit; font: inherit; font-size: 11px; line-height: 1; transition: background-color .15s ease; }
.picker > :first-child { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; text-align: left; }
.picker svg { flex: none; }
.picker:hover { background: color-mix(in srgb, var(--foreground) 10%, transparent); }
.picker:focus, .picker:focus-visible, .picker[data-state="open"] { outline: none; background: color-mix(in srgb, var(--accent) 16%, var(--hover)); }
.picker-menu { z-index: 30; box-sizing: border-box; min-width: var(--reka-select-trigger-width); max-width: min(260px, calc(100vw - 24px)); border: 1px solid var(--edge); border-radius: 10px; background: var(--surface); color: var(--foreground); backdrop-filter: blur(24px) saturate(180%); box-shadow: 0 8px 24px #0004; font-size: 12px; }
.picker-viewport { max-height: min(240px, var(--reka-select-content-available-height, 240px)); padding: 4px; }
.picker-option { display: flex; align-items: center; justify-content: space-between; gap: 10px; padding: 6px 8px; border-radius: 6px; outline: none; cursor: pointer; }
.picker-option > :first-child { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.picker-option svg { flex: none; }
.picker-option[data-highlighted] { background: var(--hover); }
</style>
