<script setup>
import { Moon, Pencil, Plus, Sun } from '@lucide/vue'
import { computed, ref } from 'vue'
import { DialogClose, DialogContent, DialogDescription, DialogOverlay, DialogPortal, DialogRoot, DialogTitle, SwitchRoot, SwitchThumb, TabsContent, TabsList, TabsRoot, TabsTrigger } from 'reka-ui'
import DeleteConfirm from './DeleteConfirm.vue'
import PanelSelect from './PanelSelect.vue'
import TrafficPanel from './TrafficPanel.vue'

const props = defineProps({ rules: { type: Object, required: true }, running: Boolean, traffic: { type: Object, required: true }, sessions: { type: Array, required: true } })
const emit = defineEmits(['toggle'])
const theme = defineModel('theme')
const systemProxyEnabled = defineModel('systemProxyEnabled', { type: Boolean, default: true })
const tunEnabled = defineModel('tunEnabled', { type: Boolean })
const protocols = [{ id: 'http', name: 'HTTP' }, { id: 'socks5', name: 'SOCKS5' }]
const editing = ref(false)
const tab = ref('traffic')
const addLabel = computed(() => tab.value === 'rules' ? '添加规则组' : '添加上游')
const draft = ref(null)
const upstreamDraft = ref(null)
const editingUpstream = ref(false)
const domainsOf = (text) => text.split(/[\s,]+/).filter(Boolean)
const targets = computed(() => [
  { id: 'system', name: '直连 · Windows 选路' },
  { id: 'wlan', name: '直连 · WLAN' },
  ...props.rules.upstreams,
  { id: 'reject', name: '拒绝连接' },
])
function editGroup(group = null) {
  draft.value = group ? { ...group } : { id: crypto.randomUUID(), name: '', domains: '', target: 'system' }
  editing.value = true
}
function saveGroup() {
  const group = draft.value
  const index = props.rules.groups.findIndex(item => item.id === group.id)
  const saved = { ...group, name: group.name.trim(), domains: [...new Set(domainsOf(group.domains))].join('\n') }
  if (index < 0) props.rules.groups.push(saved)
  else props.rules.groups[index] = saved
  editing.value = false
}
function editUpstream(upstream = null) {
  upstreamDraft.value = upstream ? { ...upstream } : { id: crypto.randomUUID(), name: '', protocol: 'http', address: '' }
  editingUpstream.value = true
}
function saveUpstream() {
  const upstream = upstreamDraft.value
  if (!upstream.name.trim() || !upstream.address.trim()) return
  const index = props.rules.upstreams.findIndex(item => item.id === upstream.id)
  const saved = { ...upstream, name: upstream.name.trim(), address: upstream.address.trim() }
  if (index < 0) props.rules.upstreams.push(saved)
  else props.rules.upstreams[index] = saved
  editingUpstream.value = false
}
function removeUpstream(id) {
  for (const group of props.rules.groups) {
    if (group.target === id) group.target = 'system'
  }
  if (props.rules.fallback === id) props.rules.fallback = 'system'
  props.rules.upstreams = props.rules.upstreams.filter(upstream => upstream.id !== id)
}
</script>

<template>
  <section class="panel">
    <header class="panel-header">
      <strong class="panel-title">域名分流</strong>
      <div class="panel-header-actions">
        <button v-if="tab !== 'traffic'" type="button" class="add-item" :aria-label="addLabel" :title="addLabel"
          @click="tab === 'rules' ? editGroup() : editUpstream()">
          <Plus :size="16" aria-hidden="true" />
        </button>
      <div class="theme-toggle" role="group" aria-label="主题" :data-active="theme">
        <button type="button" aria-label="浅色主题" title="浅色主题" :aria-pressed="theme === 'light'" @click="theme = 'light'">
          <Sun :size="15" aria-hidden="true" />
        </button>
        <button type="button" aria-label="深色主题" title="深色主题" :aria-pressed="theme === 'dark'" @click="theme = 'dark'">
          <Moon :size="15" aria-hidden="true" />
        </button>
      </div>
      </div>
    </header>
    <TabsRoot v-model="tab" class="panel-tabs">
      <div class="tabs-wrap">
        <TabsList class="tabs" :data-active="tab" aria-label="分流设置">
          <TabsTrigger value="traffic">运行详情</TabsTrigger>
          <TabsTrigger value="rules">域名规则</TabsTrigger>
          <TabsTrigger value="upstreams">上游代理</TabsTrigger>
        </TabsList>
      </div>
      <TabsContent value="traffic">
        <TrafficPanel :traffic="traffic" :sessions="sessions" :rules="rules" :theme="theme" />
      </TabsContent>
      <TabsContent value="rules">
        <TransitionGroup name="rule-list" tag="div" class="panel-scroll">
        <div v-for="(group, index) in rules.groups" :key="group.id" class="rule-grid rule-row domain-row" :class="{ 'is-disabled': group.enabled === false }">
          <div class="group-summary">
            <strong>{{ group.name || '未命名规则组' }}</strong>
            <small>{{ group.enabled === false ? '已停用' : domainsOf(group.domains).length + ' 个域名' }}</small>
          </div>
          <span class="route-cell" :title="targets.find(target => target.id === group.target)?.name || '未命名出口'">
            {{ targets.find(target => target.id === group.target)?.name || '未命名出口' }}
          </span>
          <div class="rule-actions">
            <SwitchRoot class="rule-switch" :model-value="group.enabled !== false"
              :aria-label="'启用规则：' + (group.name || '未命名规则组')" @update:model-value="group.enabled = $event">
              <SwitchThumb class="rule-switch-thumb" />
            </SwitchRoot>
            <button type="button" class="edit-rule" aria-label="编辑规则组" title="编辑规则组" @click="editGroup(group)">
              <Pencil :size="15" aria-hidden="true" />
            </button>
            <DeleteConfirm :theme="theme" label="删除规则组" description="该组域名将按其他匹配规则或默认出口处理。" @confirm="rules.groups.splice(index, 1)" />
          </div>
        </div>
        </TransitionGroup>
      </TabsContent>
      <TabsContent value="upstreams">
        <TransitionGroup name="rule-list" tag="div" class="panel-scroll">
        <div v-for="upstream in rules.upstreams" :key="upstream.id" class="rule-grid rule-row">
          <div class="group-summary">
            <strong>{{ upstream.name || '未命名上游' }}</strong>
            <small>{{ upstream.protocol.toUpperCase() }}</small>
          </div>
          <span class="route-cell address" :title="upstream.address">{{ upstream.address }}</span>
          <div class="rule-actions">
            <button type="button" class="edit-rule" aria-label="编辑上游" title="编辑上游" @click="editUpstream(upstream)">
              <Pencil :size="15" aria-hidden="true" />
            </button>
            <DeleteConfirm :theme="theme" label="删除上游" description="引用该上游的规则和默认出口将切换为 Windows 选路。" @confirm="removeUpstream(upstream.id)" />
          </div>
        </div>
        </TransitionGroup>
      </TabsContent>
    </TabsRoot>
    <footer class="panel-footer">
      <div class="fallback">
        <span>默认出口</span>
        <PanelSelect v-model="rules.fallback" :options="targets" :theme="theme" label="默认出口" />
      </div>
      <div class="footer-switches">
        <label class="footer-switch">
          <span>系统代理</span>
          <SwitchRoot v-model="systemProxyEnabled" class="compact-switch" aria-label="系统代理">
            <SwitchThumb class="rule-switch-thumb" />
          </SwitchRoot>
        </label>
        <label class="footer-switch">
          <span>TUN</span>
          <SwitchRoot v-model="tunEnabled" class="compact-switch" aria-label="启用 TUN">
            <SwitchThumb class="rule-switch-thumb" />
          </SwitchRoot>
        </label>
        <label class="footer-switch">
          <span>运行</span>
          <SwitchRoot :model-value="running" class="compact-switch" aria-label="启用分流" @update:model-value="emit('toggle')">
            <SwitchThumb class="rule-switch-thumb" />
          </SwitchRoot>
        </label>
      </div>
    </footer>
    <DialogRoot v-model:open="editing">
      <DialogPortal>
        <DialogOverlay class="editor-overlay" />
        <DialogContent as-child>
          <form v-if="draft" class="editor" :data-theme="theme" @submit.prevent="saveGroup">
            <DialogTitle class="editor-title">规则组设置</DialogTitle>
            <DialogDescription class="editor-note">填写规则名称和域名后缀，域名可用空格、逗号或换行分隔。</DialogDescription>
            <label>规则名称<input v-model="draft.name" required maxlength="40" /></label>
            <label>域名规则<textarea v-model="draft.domains" required rows="4" spellcheck="false" /></label>
            <div class="editor-actions">
              <PanelSelect v-model="draft.target" :options="targets" :theme="theme" label="出口选择" />
              <div class="editor-buttons">
                <DialogClose type="button">取消</DialogClose>
                <button type="submit" class="action" :disabled="!draft.name.trim() || !domainsOf(draft.domains).length">保存</button>
              </div>
            </div>
          </form>
        </DialogContent>
      </DialogPortal>
    </DialogRoot>
    <DialogRoot v-model:open="editingUpstream">
      <DialogPortal>
        <DialogOverlay class="editor-overlay" />
        <DialogContent as-child>
          <form v-if="upstreamDraft" class="editor" :data-theme="theme" @submit.prevent="saveUpstream">
            <DialogTitle class="editor-title">上游代理设置</DialogTitle>
            <DialogDescription class="editor-note">设置上游名称、协议和连接地址。</DialogDescription>
            <label>上游名称<input v-model="upstreamDraft.name" required maxlength="40" /></label>
            <label>地址<input v-model="upstreamDraft.address" required placeholder="主机:端口" /></label>
            <div class="editor-actions">
              <PanelSelect v-model="upstreamDraft.protocol" :options="protocols" :theme="theme" label="协议" />
              <div class="editor-buttons">
                <DialogClose type="button">取消</DialogClose>
                <button type="submit" class="action" :disabled="!upstreamDraft.name.trim() || !upstreamDraft.address.trim()">保存</button>
              </div>
            </div>
          </form>
        </DialogContent>
      </DialogPortal>
    </DialogRoot>
  </section>
</template>

<style scoped>
.panel { background: var(--surface); border: 1px solid var(--edge); box-shadow: 0 8px 22px #0003; }
.panel-header-actions { display: flex; align-items: center; gap: 8px; }
.panel-header-actions .add-item { display: grid; place-items: center; width: 32px; height: 32px; padding: 0; border-radius: 9px; color: var(--accent); }
.panel-header-actions .add-item:hover { background: var(--hover); }
.theme-toggle { position: relative; display: flex; padding: 2px; border: 1px solid var(--edge); border-radius: 999px; background: var(--field); }
.theme-toggle::before { content: ''; position: absolute; top: 2px; left: 2px; width: 29px; height: 26px; border-radius: 999px; background: color-mix(in srgb, var(--accent) 18%, var(--surface)); transition: transform .18s ease; pointer-events: none; }
.theme-toggle[data-active="dark"]::before { transform: translateX(29px); }
.theme-toggle button { position: relative; display: grid; place-items: center; width: 29px; height: 26px; padding: 0; border: 0; border-radius: 999px; background: transparent; color: var(--muted); }
.theme-toggle button[aria-pressed="true"] { color: var(--accent); }
@media (prefers-reduced-motion: reduce) { .theme-toggle::before { transition: none; } }
.tabs { padding: 3px; border-radius: 10px; background: var(--field); }
.tabs button { flex: 1; background: transparent; border-color: transparent; }
.tabs button[aria-selected="true"] { border-color: var(--edge); color: var(--accent); }
.panel-tabs { display: flex; flex: 1; flex-direction: column; min-height: 0; }
.panel-tabs > :deep([role="tabpanel"]:not([hidden])) { flex: 1; min-height: 0; }
.panel-scroll { box-sizing: border-box; height: 100%; overflow-y: auto; scrollbar-width: thin; padding: 2px; }
.rule-grid { display: grid; grid-template-columns: minmax(0, 1fr) minmax(104px, .72fr) 72px; gap: 8px; align-items: center; }
.rule-row { margin-bottom: 7px; padding: 8px; border: 1px solid var(--edge); border-radius: 10px; }
.rule-row { background: color-mix(in srgb, var(--field) 40%, var(--surface)); }
.rule-row:focus-within { border-color: color-mix(in srgb, var(--accent) 45%, var(--edge)); }
.group-summary { display: flex; align-items: baseline; gap: 6px; min-width: 0; font-size: 12px; }
.group-summary strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.group-summary small { flex: none; color: var(--muted); white-space: nowrap; }
.editor-overlay { position: fixed; inset: 0; z-index: 9; background: #0006; }
.editor { position: fixed; z-index: 10; inset: 50% auto auto 50%; transform: translate(-50%, -50%); box-sizing: border-box; width: min(380px, calc(100vw - 24px)); max-height: calc(100vh - 24px); overflow: auto; padding: 16px; border: 1px solid var(--edge); border-radius: 16px; background: #161f21; color: #eaf4f1;  }
.editor[data-theme="light"] { color-scheme: light; background: #f6faf8; color: #18322b;  }
.editor-title { margin: 0; font-size: 16px; }
.editor-note { margin: 6px 0 12px; font-size: 12px; }
.editor label { display: grid; gap: 4px; margin-bottom: 10px; font-size: 12px; }
.editor button:disabled { opacity: .5; cursor: default; }
.route-cell { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; }
textarea { width: 100%; min-width: 0; box-sizing: border-box; }
.rule-actions { display: flex; align-items: center; justify-content: space-between; padding-left: 8px; border-left: 1px solid var(--edge); }
.rule-actions .edit-rule { display: grid; place-items: center; width: 30px; height: 30px; padding: 0; border-color: transparent; background: transparent; color: var(--muted); }
.rule-actions .edit-rule:hover { color: var(--accent); background: color-mix(in srgb, var(--accent) 10%, transparent); }
button, input, select, textarea { font: inherit; font-size: 12px; color: inherit; border: 1px solid var(--edge); border-radius: 7px; background: var(--field); padding: 5px 7px; }
textarea { resize: vertical; }
button:hover { border-color: var(--accent); }
:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
.action { background: var(--accent); color: var(--ink); border-color: transparent; border-radius: 999px; padding-inline: 12px; font-weight: 600; }
.panel { display: flex; flex-direction: column; box-sizing: border-box; height: 350px; padding: 14px; border-radius: 18px; background: var(--surface); backdrop-filter: blur(24px) saturate(180%); border: 1px solid var(--edge); box-shadow: 0 4px 6px -1px #0000000d, 0 20px 25px -5px #00000014, inset 0 1px #ffffff0a; }
:where(.panel) { margin-top: 8px; }
.panel-header { display: flex; align-items: center; justify-content: space-between; margin-bottom: 12px; }
.tabs-wrap { margin-bottom: 12px; }
.panel > header, .panel > footer, .panel-tabs > :first-child { flex: none; }
.panel-title { font-size: 13px; font-weight: 500; letter-spacing: -.01em; }
.tabs { display: flex; position: relative; gap: 0; padding: 3px; background: var(--field); border-radius: 9px; }
.tabs::before { content: ''; position: absolute; top: 3px; bottom: 3px; left: 3px; width: calc((100% - 6px) / 3); background: var(--selected); border-radius: 6px; box-shadow: 0 1px 3px #0001; transition: transform .18s ease; }
.tabs[data-active="rules"]::before { transform: translateX(100%); }
.tabs[data-active="upstreams"]::before { transform: translateX(200%); }
.tabs button { position: relative; font-size: 12px; font-weight: 500; border: 0; color: var(--muted); }
.tabs button[aria-selected="true"] { color: var(--foreground); }
.rule-grid { grid-template-columns: minmax(0, 1fr) minmax(0, 34%) 52px; gap: 8px; }
.rule-row { min-height: 44px; box-sizing: border-box; padding: 6px 8px; margin-bottom: 6px; border: 0; border-radius: 8px; background: var(--cell); }
.rule-row:hover, .rule-row:focus-within { background: var(--hover); }
.group-summary { gap: 6px; }
.group-summary strong { font-size: 13px; font-weight: 500; }
.group-summary small { font-size: 11px; font-weight: 400; }
.route-cell { justify-self: start; box-sizing: border-box; max-width: 100%; font-size: 11px; border-radius: 6px; padding: 3px 6px; background: var(--field); color: var(--muted); }
.address { font-family: "SF Mono", Consolas, monospace; font-size: 11px; }
.rule-actions { padding: 0; gap: 0; border: 0; }
.rule-actions .edit-rule, .rule-actions :deep(.delete-trigger) { width: 26px; height: 26px; }
footer { border-top: 1px solid var(--edge); padding-top: 12px; }
.fallback { display: flex; align-items: center; gap: 7px; color: var(--muted); font-size: 11px; }
.action { font-size: 11px; font-weight: 500; padding: 6px 10px; border-radius: 7px; background: #10b9811a; color: var(--accent); }
.editor { background: var(--surface); color: var(--foreground); border-color: var(--edge); backdrop-filter: blur(24px) saturate(180%); }
.editor input, .editor textarea { border: 0; background: var(--hover); padding: 6px 8px; transition: background-color .15s ease; }
.editor input:hover, .editor textarea:hover { background: color-mix(in srgb, var(--foreground) 10%, transparent); }
.editor input:focus, .editor input:focus-visible, .editor textarea:focus, .editor textarea:focus-visible { outline: none; background: color-mix(in srgb, var(--accent) 16%, var(--hover)); }
@media (max-width: 420px) { .rule-grid { grid-template-columns: minmax(0, 1fr) minmax(0, 34%) 52px; gap: 4px; } .group-summary { gap: 3px; } }
.domain-row { grid-template-columns: minmax(0, 1fr) minmax(0, 34%) 86px; }
.is-disabled .group-summary strong, .is-disabled .route-cell { color: var(--muted); }
.rule-actions .rule-switch, .footer-switch .compact-switch { display: flex; align-items: center; width: 27px; height: 16px; padding: 2px; border-radius: 999px; background: var(--selected); }
.rule-actions .rule-switch[data-state="checked"], .footer-switch .compact-switch[data-state="checked"] { background: color-mix(in srgb, var(--accent) 35%, var(--field)); }
.panel-footer { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 8px 10px; margin-top: 12px; }
.editor-actions { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
.editor-buttons { display: flex; gap: 8px; }
.footer-switches { display: flex; align-items: center; gap: 10px; margin-left: auto; }
.footer-switch { display: flex; align-items: center; gap: 5px; color: var(--muted); font-size: 11px; white-space: nowrap; cursor: pointer; }
.panel-footer .fallback { min-width: 0; }
.panel-footer .fallback > span { flex: none; }
.panel-footer :deep(.picker) { box-sizing: border-box; width: 96px; }
.panel-footer :deep(.picker > span:first-child) { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
.panel-footer :deep(.picker svg) { flex: none; }
.rule-switch-thumb { display: block; width: 10px; height: 10px; border-radius: 50%; background: var(--muted); transition: transform .15s ease; }
.rule-switch-thumb[data-state="checked"] { transform: translateX(11px); background: var(--accent); }
@media (prefers-reduced-motion: reduce) { .tabs::before, .rule-switch-thumb { transition: none; } }
</style>
