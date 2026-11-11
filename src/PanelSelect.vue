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
            <SelectItem v-for="option in options" :key="option.id" :value="option.id" class="picker-option">
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
.picker { display: inline-flex; align-items: center; justify-content: space-between; gap: 8px; min-width: 0; padding: 5px 8px; border: 1px solid var(--edge); border-radius: 8px; background: var(--field); color: inherit; font: inherit; font-size: 12px; }
.picker:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
.picker-menu { z-index: 20; box-sizing: border-box; min-width: var(--reka-select-trigger-width); border: 1px solid #344442; border-radius: 10px; background: #161f21; color: #eaf4f1; box-shadow: 0 8px 24px #0004; font-size: 12px; }
.picker-viewport { max-height: min(240px, var(--reka-select-content-available-height, 240px)); padding: 4px; }
.picker-menu[data-theme="light"] { background: #f6faf8; color: #18322b; border-color: #cadbd5; }
.picker-option { display: flex; align-items: center; justify-content: space-between; gap: 10px; padding: 6px 8px; border-radius: 6px; outline: none; cursor: pointer; }
.picker-option[data-highlighted] { background: #84edc72b; }
.picker { border-color: var(--edge); background: var(--field); color: inherit; font-size: 11px; border-radius: 6px; }
.picker-menu, .picker-menu[data-theme="light"] { background: var(--surface); color: var(--foreground); border-color: var(--edge); backdrop-filter: blur(24px) saturate(180%); }
.picker-option[data-highlighted] { background: var(--hover); }
</style>
