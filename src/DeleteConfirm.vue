<script setup>
import { Trash2 } from '@lucide/vue'
import { PopoverClose, PopoverContent, PopoverPortal, PopoverRoot, PopoverTrigger } from 'reka-ui'

defineProps({ label: String, description: String, theme: String })
defineEmits(['confirm'])
</script>

<template>
  <PopoverRoot>
    <PopoverTrigger as-child>
      <button type="button" class="delete-trigger" :aria-label="label" :title="label">
        <Trash2 :size="15" aria-hidden="true" />
      </button>
    </PopoverTrigger>
    <PopoverPortal>
      <PopoverContent as-child align="end" :side-offset="7" :collision-padding="10" :aria-label="label">
        <div class="delete-popover" :data-theme="theme">
          <strong>确认{{ label }}？</strong>
          <p>{{ description }}</p>
          <div class="flex justify-end gap-2">
            <PopoverClose class="cancel">取消</PopoverClose>
            <PopoverClose class="danger" @click="$emit('confirm')">删除</PopoverClose>
          </div>
        </div>
      </PopoverContent>
    </PopoverPortal>
  </PopoverRoot>
</template>

<style scoped>
.delete-trigger { display: grid; place-items: center; width: 30px; height: 30px; padding: 0; border: 1px solid transparent; border-radius: 8px; background: transparent; color: var(--muted); }
.delete-trigger:hover, .delete-trigger[data-state="open"] { color: #d36570; background: #d365701a; }
.delete-popover { z-index: 30; box-sizing: border-box; width: min(230px, calc(100vw - 20px)); padding: 12px; border: 1px solid #344442; border-radius: 12px; background: #161f21; color: #eaf4f1; box-shadow: 0 8px 24px #0005; font-size: 13px; }
.delete-popover[data-theme="light"] { background: #f6faf8; color: #18322b; border-color: #cadbd5; }
.delete-popover p { margin: 6px 0 12px; font-size: 12px; line-height: 1.5; }
.cancel, .danger { padding: 5px 10px; border: 1px solid #879a9460; border-radius: 7px; font: inherit; background: transparent; color: inherit; }
.danger { background: #a13743; color: white; border-color: transparent; }
:focus-visible { outline: 2px solid #72bda5; outline-offset: 1px; }
.delete-popover, .delete-popover[data-theme="light"] { background: var(--surface); color: var(--foreground); border-color: var(--edge); backdrop-filter: blur(24px) saturate(180%); }
.danger { background: #f43f5e1a; color: #e8657b; }
.delete-popover[data-theme="light"] .danger { color: #e11d48; }
</style>
