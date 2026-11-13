<script setup>
import { invoke } from '@tauri-apps/api/core'
import { ArrowDown, ArrowUp, Monitor, Moon, Pencil, Plus, RefreshCw, Sun, Trash2 } from '@lucide/vue'
import { computed, ref, watch } from 'vue'
import { DialogClose, DialogContent, DialogDescription, DialogOverlay, DialogPortal, DialogRoot, DialogTitle, SwitchRoot, SwitchThumb, TabsContent, TabsList, TabsRoot, TabsTrigger } from 'reka-ui'
import DeleteConfirm from './DeleteConfirm.vue'
import HistoryPanel from './HistoryPanel.vue'
import PanelSelect from './PanelSelect.vue'
import TrafficPanel from './TrafficPanel.vue'
import { domainsOf, formatBytes } from './utils/format'

const props = defineProps({ rules: { type: Object, required: true }, theme: { type: String, required: true }, running: Boolean, toggling: Boolean, traffic: { type: Object, required: true }, sessions: { type: Array, required: true } })
const emit = defineEmits(['toggle', 'logs-cleared'])
const desktop = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
const themeMode = defineModel('themeMode', { type: String, default: 'system' })
const systemProxyEnabled = defineModel('systemProxyEnabled', { type: Boolean, default: true })
const tunEnabled = defineModel('tunEnabled', { type: Boolean })
const protocols = [
  { id: 'http', name: 'HTTP' },
  { id: 'socks5', name: 'SOCKS5' },
  { id: 'wireguard', name: 'WireGuard' },
  { id: 'interface', name: '指定网卡' },
]
const networkInterfaces = ref([])
const detectedProxies = ref([])
const scanBusy = ref(false)
const wgFileInput = ref(null)
const editing = ref(false)
const tab = ref('traffic')
const addLabel = computed(() => tab.value === 'rules' ? '添加规则组' : '添加上游')
const draft = ref(null)
const upstreamDraft = ref(null)
const editingUpstream = ref(false)
const editingBypass = ref(false)
const bypassDraft = ref('')
const editingDns = ref(false)
const dnsDraft = ref({ cnDns: '', overseasDns: '' })
const DEFAULT_CN_DNS = '223.5.5.5\n119.29.29.29'
const DEFAULT_OVERSEAS_DNS = 'https://dns.google/dns-query'
const geoipStatus = ref({ exists: false, size: 0, updatedAt: 0 })
const geoipBusy = ref(false)
const geoipNote = ref('')
const logStats = ref({ fileCount: 0, totalBytes: 0 })
const logsBusy = ref(false)
const logsNote = ref('')
const coreBusy = ref(false)
const coreNote = ref('')
const targets = computed(() => [
  { id: 'system', name: '直连 · Windows 选路' },
  { id: 'wlan', name: '直连 · WLAN' },
  ...props.rules.upstreams,
  { id: 'reject', name: '拒绝连接' },
])
const targetNameMap = computed(() => new Map(targets.value.map(item => [item.id, item.name])))
const targetName = (id) => targetNameMap.value.get(id) || '未命名出口'
const fallbackSummary = computed(() => {
  const cn = targetName(props.rules.cnFallback || 'wlan')
  const overseas = targetName(props.rules.fallback || 'system')
  return cn === overseas ? `默认 ${overseas}` : `国内 ${cn.replace('直连 · ', '')} / 国外 ${overseas.replace('直连 · ', '')}`
})
async function loadSettingsInfo() {
  if (!desktop) return
  try {
    const [geo, logs] = await Promise.all([invoke('get_geoip_status'), invoke('get_log_stats')])
    if (geo) geoipStatus.value = geo
    if (logs) logStats.value = logs
  } catch {}
}
watch(tab, (next) => {
  if (next === 'settings') {
    geoipNote.value = ''
    logsNote.value = ''
    coreNote.value = ''
    loadSettingsInfo()
  }
})
async function restartCore() {
  if (!desktop || !props.running || coreBusy.value) return
  coreBusy.value = true
  coreNote.value = '正在重启内核...'
  try {
    await invoke('restart_proxy')
    coreNote.value = '内核已重启'
  } catch (err) {
    coreNote.value = String(err || '重启失败')
  } finally {
    coreBusy.value = false
  }
}
async function refreshGeoip() {
  if (!desktop || geoipBusy.value) return
  geoipBusy.value = true
  geoipNote.value = '正在下载最新数据库...'
  try {
    const res = await invoke('refresh_geoip_db')
    geoipStatus.value = res
    geoipNote.value = res.restarted ? '已更新并重载内核' : '数据库已更新'
  } catch (err) {
    geoipNote.value = String(err || '刷新失败')
  } finally {
    geoipBusy.value = false
  }
}
async function clearLogsNow() {
  if (!desktop || logsBusy.value) return
  logsBusy.value = true
  logsNote.value = ''
  try {
    const res = await invoke('clear_logs')
    logStats.value = res
    emit('logs-cleared')
    logsNote.value = '日志已清空'
  } catch (err) {
    logsNote.value = String(err || '清理失败')
  } finally {
    logsBusy.value = false
  }
}
function editBypass() {
  bypassDraft.value = props.rules.bypassDomains ?? ''
  editingBypass.value = true
}
function saveBypass() {
  props.rules.bypassDomains = [...new Set(domainsOf(bypassDraft.value))].join('\n')
  editingBypass.value = false
}
function dnsLinesOf(raw) {
  return [...new Set(String(raw || '').split(/[\s,;]+/).map(s => s.trim()).filter(Boolean))]
}
function editDns() {
  dnsDraft.value = {
    cnDns: props.rules.cnDns || DEFAULT_CN_DNS,
    overseasDns: props.rules.overseasDns || DEFAULT_OVERSEAS_DNS,
  }
  editingDns.value = true
}
function resetDnsDraft() {
  dnsDraft.value = { cnDns: DEFAULT_CN_DNS, overseasDns: DEFAULT_OVERSEAS_DNS }
}
function saveDns() {
  const cn = dnsLinesOf(dnsDraft.value.cnDns)
  const overseas = dnsLinesOf(dnsDraft.value.overseasDns)
  props.rules.cnDns = cn.length ? cn.join('\n') : DEFAULT_CN_DNS
  props.rules.overseasDns = overseas.length ? overseas.join('\n') : DEFAULT_OVERSEAS_DNS
  editingDns.value = false
}
function formatGeoipMeta() {
  if (!geoipStatus.value.exists) return '未检测到数据库'
  const sizeText = formatBytes(geoipStatus.value.size || 0)
  if (!geoipStatus.value.updatedAt) return sizeText
  const d = new Date(geoipStatus.value.updatedAt)
  const pad = (n) => String(n).padStart(2, '0')
  return `${sizeText} · ${d.getMonth() + 1}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}
const editingRuleItem = ref(false)
const ruleItemDraft = ref({ index: -1, kind: 'suffix', value: '' })
const ruleKindOptions = [
  { id: 'suffix', label: '*.后缀', placeholder: '例如 google.com 或 https://mail.google.com' },
  { id: 'wildcard', label: '*泛域名*', placeholder: '例如 openai 或 cdn-*.example.com' },
  { id: 'prefix', label: '前缀.*', placeholder: '例如 api.internal' },
  { id: 'exact', label: '精确', placeholder: '例如 exact.example.com' },
  { id: 'cidr', label: 'IP/CIDR', placeholder: '例如 10.10.0.0/16 或 10.10.1.5' },
  { id: 'port', label: 'port:端口', placeholder: '例如 22 或 8000-9000' },
  { id: 'proc', label: 'proc:进程', placeholder: '例如 ssh.exe（支持粘贴完整路径）' },
]
const draftRules = computed(() => (draft.value ? domainsOf(draft.value.domains) : []))
function detectRuleKind(rule) {
  const s = String(rule || '').trim()
  if (/^(?:proc|process):/i.test(s)) return { kind: 'proc', badge: '进程', value: s.replace(/^(?:proc|process):/i, '') }
  if (/^port:/i.test(s)) return { kind: 'port', badge: '端口', value: s.replace(/^port:/i, '') }
  if (/^\d{1,3}(\.\d{1,3}){3}(\/\d{1,2})?$/.test(s) || (s.includes(':') && /^[0-9a-f:]+(\/\d{1,3})?$/i.test(s))) {
    return { kind: 'cidr', badge: 'IP', value: s }
  }
  if (s.startsWith('*.') && !s.slice(2).includes('*')) return { kind: 'suffix', badge: '后缀', value: s.slice(2) }
  if (s.startsWith('*') && s.endsWith('*') && s.length > 2) return { kind: 'wildcard', badge: '泛域', value: s.slice(1, -1) }
  if (s.endsWith('.*') && !s.slice(0, -2).includes('*')) return { kind: 'prefix', badge: '前缀', value: s.slice(0, -2) }
  if (s.includes('*') || s.includes('?')) return { kind: 'wildcard', badge: '通配', value: s }
  return { kind: 'exact', badge: '精确', value: s }
}
function editGroup(group = null) {
  draft.value = group ? { ...group } : { id: crypto.randomUUID(), name: '', domains: '', target: 'system' }
  editingRuleItem.value = false
  editing.value = true
}
function openRuleItemEditor(index = -1) {
  if (index >= 0) {
    const existing = draftRules.value[index] || ''
    const parsed = detectRuleKind(existing)
    ruleItemDraft.value = { index, kind: parsed.kind, value: parsed.value }
  } else {
    ruleItemDraft.value = { index: -1, kind: 'suffix', value: '' }
  }
  editingRuleItem.value = true
}
function removeRuleItem(index) {
  if (!draft.value) return
  const list = [...draftRules.value]
  list.splice(index, 1)
  draft.value.domains = list.join('\n')
}
function extractHostToken(raw) {
  let s = String(raw || '').trim()
  if (!s) return ''
  if (/^[a-z][a-z0-9+.-]*:\/\//i.test(s)) {
    try {
      s = new URL(s).hostname
    } catch {
      s = s.replace(/^[a-z][a-z0-9+.-]*:\/\//i, '').split(/[/?#]/)[0]
    }
  } else {
    s = s.split(/[/?#]/)[0]
  }
  if (!s.includes(':') || s.split(':').length === 2) {
    s = s.replace(/:\d+$/, '')
  }
  return s.trim().toLowerCase()
}
function buildRuleToken(raw, kind) {
  const s = String(raw || '').trim()
  if (!s) return ''
  if (kind === 'proc') {
    const cleaned = s.replace(/^(?:proc|process):/i, '').replace(/^["']|["']$/g, '').trim()
    const exe = cleaned.split(/[\\/]/).pop()?.trim()
    return exe ? `proc:${exe}` : ''
  }
  if (kind === 'port') {
    const expr = s.replace(/^port:/i, '').trim()
    const range = expr.match(/^(\d{1,5})-(\d{1,5})$/)
    if (range) {
      const a = Number(range[1])
      const b = Number(range[2])
      return a >= 1 && b <= 65535 && a <= b ? `port:${a}-${b}` : ''
    }
    if (/^\d{1,5}$/.test(expr)) {
      const p = Number(expr)
      return p >= 1 && p <= 65535 ? `port:${p}` : ''
    }
    return ''
  }
  if (kind === 'cidr') {
    return s.replace(/^\.+/, '').trim()
  }
  const host = extractHostToken(s)
  if (!host) return ''
  if (kind === 'suffix') {
    const clean = host.replace(/^\*+\.?/, '').replace(/^\.+/, '')
    return clean ? `*.${clean}` : ''
  }
  if (kind === 'wildcard') {
    const clean = host.replace(/^\.+/, '')
    if (!clean) return ''
    return /[*?]/.test(clean) ? clean : `*${clean}*`
  }
  if (kind === 'prefix') {
    const clean = host.replace(/\.?\*+$/, '').replace(/^\.+/, '')
    return clean ? `${clean}.*` : ''
  }
  return host.replace(/^\.+/, '')
}
const ruleItemPreview = computed(() => {
  const tokens = domainsOf(ruleItemDraft.value.value)
    .map(item => buildRuleToken(item, ruleItemDraft.value.kind))
    .filter(Boolean)
  return [...new Set(tokens)]
})
function saveRuleItem() {
  if (!draft.value) return
  const generated = ruleItemPreview.value
  if (!generated.length) return
  const list = [...draftRules.value]
  if (ruleItemDraft.value.index >= 0) {
    list.splice(ruleItemDraft.value.index, 1, ...generated)
  } else {
    list.push(...generated)
  }
  draft.value.domains = [...new Set(list)].join('\n')
  editingRuleItem.value = false
}
function saveGroup() {
  const group = draft.value
  const index = props.rules.groups.findIndex(item => item.id === group.id)
  const saved = { ...group, name: group.name.trim(), domains: [...new Set(domainsOf(group.domains))].join('\n') }
  if (index < 0) props.rules.groups.push(saved)
  else props.rules.groups[index] = saved
  editing.value = false
}
function moveGroup(index, delta) {
  const target = index + delta
  if (target < 0 || target >= props.rules.groups.length) return
  const list = props.rules.groups
  const [item] = list.splice(index, 1)
  list.splice(target, 0, item)
}
function extractWireGuardEndpoint(confText) {
  const match = String(confText || '').match(/^\s*Endpoint\s*=\s*([^\r\n#;]+)/im)
  return match ? match[1].trim() : ''
}
function protocolLabel(protocol) {
  const found = protocols.find(item => item.id === protocol)
  return found ? found.name : String(protocol || 'HTTP').toUpperCase()
}
async function scanSystemUpstreams() {
  if (!desktop || scanBusy.value) return
  scanBusy.value = true
  try {
    const res = await invoke('scan_system_upstreams')
    if (Array.isArray(res?.proxies)) detectedProxies.value = res.proxies
    if (Array.isArray(res?.interfaces)) networkInterfaces.value = res.interfaces
  } catch {} finally {
    scanBusy.value = false
  }
}
function applyDetectedProxy(proxy) {
  if (!upstreamDraft.value) return
  upstreamDraft.value.protocol = proxy.protocol || 'http'
  upstreamDraft.value.address = proxy.address || ''
  if (!upstreamDraft.value.name?.trim()) {
    upstreamDraft.value.name = proxy.name || ''
  }
}
function applyDetectedInterface(iface) {
  if (!upstreamDraft.value) return
  upstreamDraft.value.protocol = 'interface'
  upstreamDraft.value.address = iface
  if (!upstreamDraft.value.name?.trim()) {
    upstreamDraft.value.name = iface
  }
}
function loadNetworkInterfaces() {
  return scanSystemUpstreams()
}
const interfaceOptions = computed(() => {
  const list = [...networkInterfaces.value]
  const current = upstreamDraft.value?.address?.trim()
  if (current && !list.includes(current)) list.unshift(current)
  return list.map(name => ({ id: name, name }))
})
const upstreamCanSave = computed(() => {
  const item = upstreamDraft.value
  if (!item || !item.name?.trim()) return false
  if (item.protocol === 'wireguard') return Boolean(item.wireguardConf?.trim())
  return Boolean(item.address?.trim())
})
const importingFile = ref(false)
const hasOpenEditor = computed(() =>
  editing.value || editingRuleItem.value || editingUpstream.value || editingBypass.value || editingDns.value || importingFile.value
)
watch(hasOpenEditor, (locked) => {
  if (desktop) invoke('set_panel_blur_lock', { locked }).catch(() => {})
}, { immediate: true })
function triggerWgImport() {
  importingFile.value = true
  if (desktop) invoke('set_panel_blur_lock', { locked: true }).catch(() => {})
  const onFocusBack = () => {
    window.removeEventListener('focus', onFocusBack)
    setTimeout(() => { importingFile.value = false }, 200)
  }
  window.addEventListener('focus', onFocusBack, { once: true })
  wgFileInput.value?.click()
}
async function importWireGuardFile(event) {
  const file = event.target?.files?.[0]
  if (!file || !upstreamDraft.value) {
    importingFile.value = false
    return
  }
  try {
    const text = await file.text()
    upstreamDraft.value.wireguardConf = text
    const ep = extractWireGuardEndpoint(text)
    if (ep) upstreamDraft.value.address = ep
    if (!upstreamDraft.value.name?.trim()) {
      upstreamDraft.value.name = file.name.replace(/\.[^.]+$/, '') || 'WireGuard'
    }
  } finally {
    event.target.value = ''
    importingFile.value = false
  }
}
function editUpstream(upstream = null) {
  upstreamDraft.value = upstream
    ? { wireguardConf: '', ...upstream }
    : { id: crypto.randomUUID(), name: '', protocol: 'http', address: '', wireguardConf: '' }
  editingUpstream.value = true
  loadNetworkInterfaces()
}
function saveUpstream() {
  const upstream = upstreamDraft.value
  if (!upstreamCanSave.value) return
  const index = props.rules.upstreams.findIndex(item => item.id === upstream.id)
  const isWg = upstream.protocol === 'wireguard'
  const wgConf = isWg ? upstream.wireguardConf.trim() : undefined
  const address = isWg
    ? (extractWireGuardEndpoint(wgConf) || upstream.address.trim() || 'WireGuard')
    : upstream.address.trim()
  const saved = {
    id: upstream.id,
    name: upstream.name.trim(),
    protocol: upstream.protocol,
    address,
    ...(wgConf ? { wireguardConf: wgConf } : {}),
  }
  if (index < 0) props.rules.upstreams.push(saved)
  else props.rules.upstreams[index] = saved
  editingUpstream.value = false
}
function removeUpstream(id) {
  for (const group of props.rules.groups) {
    if (group.target === id) group.target = 'system'
  }
  if (props.rules.fallback === id) props.rules.fallback = 'system'
  if (props.rules.cnFallback === id) props.rules.cnFallback = 'wlan'
  props.rules.upstreams = props.rules.upstreams.filter(upstream => upstream.id !== id)
}
</script>

<template>
  <section class="panel">
    <header class="panel-header">
      <strong class="panel-title">域名分流</strong>
      <div class="panel-header-actions">
        <button v-if="tab === 'rules' || tab === 'upstreams'" type="button" class="add-item" :aria-label="addLabel" :title="addLabel"
          @click="tab === 'rules' ? editGroup() : editUpstream()">
          <Plus :size="16" aria-hidden="true" />
        </button>
      <div class="theme-toggle" role="group" aria-label="主题" :data-active="themeMode">
        <button type="button" aria-label="跟随系统" title="跟随系统" :aria-pressed="themeMode === 'system'" @click="themeMode = 'system'">
          <Monitor :size="15" aria-hidden="true" />
        </button>
        <button type="button" aria-label="浅色主题" title="浅色主题" :aria-pressed="themeMode === 'light'" @click="themeMode = 'light'">
          <Sun :size="15" aria-hidden="true" />
        </button>
        <button type="button" aria-label="深色主题" title="深色主题" :aria-pressed="themeMode === 'dark'" @click="themeMode = 'dark'">
          <Moon :size="15" aria-hidden="true" />
        </button>
      </div>
      </div>
    </header>
    <TabsRoot v-model="tab" class="panel-tabs">
      <div class="tabs-wrap">
        <TabsList class="tabs" :data-active="tab" aria-label="分流设置">
          <TabsTrigger value="traffic">运行详情</TabsTrigger>
          <TabsTrigger value="history">历史统计</TabsTrigger>
          <TabsTrigger value="rules">域名规则</TabsTrigger>
          <TabsTrigger value="upstreams">上游代理</TabsTrigger>
          <TabsTrigger value="settings">系统设置</TabsTrigger>
        </TabsList>
      </div>
      <TabsContent value="traffic">
        <TrafficPanel :traffic="traffic" />
      </TabsContent>
      <TabsContent value="history">
        <HistoryPanel :traffic="traffic" :sessions="sessions" :rules="rules" :theme="theme" />
      </TabsContent>
      <TabsContent value="rules">
        <TransitionGroup name="rule-list" tag="div" class="panel-scroll">
        <div v-for="(group, index) in rules.groups" :key="group.id" class="rule-grid rule-row domain-row" :class="{ 'is-disabled': group.enabled === false }">
          <div class="group-summary">
            <strong :title="group.name || '未命名规则组'">{{ group.name || '未命名规则组' }}</strong>
            <small>{{ group.enabled === false ? '已停用' : domainsOf(group.domains).length + ' 个规则' }}</small>
          </div>
          <span class="route-cell" :title="targetName(group.target)">
            {{ targetName(group.target) }}
          </span>
          <div class="rule-actions">
            <button type="button" class="edit-rule sort-rule" aria-label="上移规则组" title="上移（提高匹配优先级）" :disabled="index === 0" @click="moveGroup(index, -1)">
              <ArrowUp :size="13" aria-hidden="true" />
            </button>
            <button type="button" class="edit-rule sort-rule" aria-label="下移规则组" title="下移（降低匹配优先级）" :disabled="index === rules.groups.length - 1" @click="moveGroup(index, 1)">
              <ArrowDown :size="13" aria-hidden="true" />
            </button>
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
            <strong :title="upstream.name || '未命名上游'">{{ upstream.name || '未命名上游' }}</strong>
            <small>{{ protocolLabel(upstream.protocol) }}</small>
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
      <TabsContent value="settings">
        <div class="panel-scroll settings-list">
          <div class="rule-row setting-row">
            <div class="group-summary">
              <strong>未匹配 · 国内出口</strong>
              <small>GeoIP 命中中国大陆 IP</small>
            </div>
            <PanelSelect v-model="rules.cnFallback" :options="targets" :theme="theme" label="国内默认出口" />
          </div>
          <div class="rule-row setting-row">
            <div class="group-summary">
              <strong>未匹配 · 国外出口</strong>
              <small>非国内 IP 或未识别归属</small>
            </div>
            <PanelSelect v-model="rules.fallback" :options="targets" :theme="theme" label="国外默认出口" />
          </div>
          <div class="rule-row setting-row">
            <div class="group-summary">
              <strong>跳过代理域名</strong>
              <small>{{ domainsOf(rules.bypassDomains).length }} 条 · 跳过 Fake-IP 与系统代理</small>
            </div>
            <button type="button" class="setting-btn" @click="editBypass">
              <Pencil :size="13" aria-hidden="true" />
              <span>编辑</span>
            </button>
          </div>
          <div class="rule-row setting-row">
            <div class="group-summary">
              <strong>DNS 服务器</strong>
              <small>国内 {{ dnsLinesOf(rules.cnDns || DEFAULT_CN_DNS).length }} 个 / 国外 {{ dnsLinesOf(rules.overseasDns || DEFAULT_OVERSEAS_DNS).length }} 个</small>
            </div>
            <button type="button" class="setting-btn" @click="editDns">
              <Pencil :size="13" aria-hidden="true" />
              <span>编辑</span>
            </button>
          </div>
          <div class="rule-row setting-row">
            <div class="group-summary">
              <strong>GeoIP 数据库</strong>
              <small>{{ geoipNote || formatGeoipMeta() }}</small>
            </div>
            <button type="button" class="setting-btn" :disabled="geoipBusy || !desktop" @click="refreshGeoip">
              <RefreshCw :size="13" :class="{ 'is-spinning': geoipBusy }" aria-hidden="true" />
              <span>{{ geoipBusy ? '刷新中' : '刷新数据库' }}</span>
            </button>
          </div>
          <div class="rule-row setting-row">
            <div class="group-summary">
              <strong>代理内核</strong>
              <small>{{ coreNote || (running ? 'ApexCore 运行中' : 'ApexCore 未启动') }}</small>
            </div>
            <button type="button" class="setting-btn" :disabled="coreBusy || !desktop || !running" @click="restartCore">
              <RefreshCw :size="13" :class="{ 'is-spinning': coreBusy }" aria-hidden="true" />
              <span>{{ coreBusy ? '重启中' : '重启内核' }}</span>
            </button>
          </div>
          <div class="rule-row setting-row">
            <div class="group-summary">
              <strong>运行日志</strong>
              <small>{{ logsNote || `${logStats.fileCount} 个文件 · ${formatBytes(logStats.totalBytes)}` }}</small>
            </div>
            <button type="button" class="setting-btn danger" :disabled="logsBusy || !desktop || logStats.fileCount === 0" @click="clearLogsNow">
              <Trash2 :size="13" aria-hidden="true" />
              <span>{{ logsBusy ? '清理中' : '清理日志' }}</span>
            </button>
          </div>
        </div>
      </TabsContent>
    </TabsRoot>
    <footer class="panel-footer">
      <button type="button" class="fallback fallback-link" :title="fallbackSummary" @click="tab = 'settings'">
        <span>{{ fallbackSummary }}</span>
      </button>
      <div class="footer-switches">
        <label class="footer-switch">
          <span>系统代理</span>
          <SwitchRoot v-model="systemProxyEnabled" class="compact-switch" aria-label="系统代理">
            <SwitchThumb class="rule-switch-thumb" />
          </SwitchRoot>
        </label>
        <label class="footer-switch" :class="{ 'is-busy': toggling }">
          <span>TUN</span>
          <SwitchRoot v-model="tunEnabled" :disabled="toggling" class="compact-switch" aria-label="启用 TUN">
            <SwitchThumb class="rule-switch-thumb" />
          </SwitchRoot>
        </label>
        <label class="footer-switch" :class="{ 'is-busy': toggling }">
          <span>运行</span>
          <SwitchRoot :model-value="running" :disabled="toggling" :aria-busy="toggling" class="compact-switch" aria-label="启用分流" @update:model-value="emit('toggle')">
            <SwitchThumb class="rule-switch-thumb" :class="{ 'is-loading': toggling }" />
          </SwitchRoot>
        </label>
      </div>
    </footer>
    <DialogRoot v-model:open="editing">
      <DialogPortal>
        <DialogOverlay class="editor-overlay" />
        <DialogContent as-child>
          <form v-if="draft" class="editor" :data-theme="theme" @submit.prevent="saveGroup">
            <div class="editor-head">
              <DialogTitle class="editor-title">规则组设置</DialogTitle>
            </div>
            <div class="editor-body">
              <label>规则名称<input v-model="draft.name" required maxlength="40" placeholder="例如：AI 服务 / 公司内网" /></label>
              <div class="rule-table-section">
                <div class="label-row">
                  <span class="rule-table-label">规则列表 <small>({{ draftRules.length }})</small></span>
                  <button type="button" class="inline-action" @click="openRuleItemEditor(-1)">+ 添加规则</button>
                </div>
                <div class="rule-mini-table">
                  <div v-if="!draftRules.length" class="rule-mini-empty">暂无规则，点击右上角“+ 添加规则”</div>
                  <div v-for="(item, idx) in draftRules" :key="`${idx}-${item}`" class="rule-mini-row">
                    <span class="rule-kind-badge">{{ detectRuleKind(item).badge }}</span>
                    <span class="rule-mini-val address" :title="item">{{ item }}</span>
                    <div class="rule-mini-ops">
                      <button type="button" class="mini-op-btn" title="编辑规则" @click="openRuleItemEditor(idx)"><Pencil :size="12" /></button>
                      <button type="button" class="mini-op-btn danger" title="删除规则" @click="removeRuleItem(idx)"><Trash2 :size="12" /></button>
                    </div>
                  </div>
                </div>
              </div>
            </div>
            <div class="editor-actions">
              <PanelSelect v-model="draft.target" :options="targets" :theme="theme" label="出口选择" />
              <div class="editor-buttons">
                <DialogClose type="button">取消</DialogClose>
                <button type="submit" class="action" :disabled="!draft.name.trim() || !draftRules.length">保存</button>
              </div>
            </div>
          </form>
        </DialogContent>
      </DialogPortal>
    </DialogRoot>
    <DialogRoot v-model:open="editingRuleItem">
      <DialogPortal>
        <DialogOverlay class="editor-overlay editor-overlay-sub" />
        <DialogContent as-child>
          <form class="editor editor-sub" :data-theme="theme" @submit.prevent="saveRuleItem">
            <div class="editor-head">
              <DialogTitle class="editor-title">{{ ruleItemDraft.index >= 0 ? '编辑规则' : '添加规则' }}</DialogTitle>
            </div>
            <div class="editor-body">
              <DialogDescription class="editor-note">选择匹配模式并输入内容（支持直接粘贴 URL，多个可用空格或逗号分隔）。</DialogDescription>
              <div class="kind-chip-row">
                <button
                  v-for="opt in ruleKindOptions"
                  :key="opt.id"
                  type="button"
                  class="preset-chip"
                  :class="{ active: ruleItemDraft.kind === opt.id }"
                  @click="ruleItemDraft.kind = opt.id"
                >{{ opt.label }}</button>
              </div>
              <label>
                规则内容
                <input
                  v-model="ruleItemDraft.value"
                  required
                  autofocus
                  :placeholder="ruleKindOptions.find(o => o.id === ruleItemDraft.kind)?.placeholder"
                />
              </label>
              <p v-if="ruleItemPreview.length" class="rule-preview">将生成：<code>{{ ruleItemPreview.join(' , ') }}</code></p>
            </div>
            <div class="editor-actions">
              <span />
              <div class="editor-buttons">
                <DialogClose type="button">取消</DialogClose>
                <button type="submit" class="action" :disabled="!ruleItemPreview.length">{{ ruleItemDraft.index >= 0 ? '更新' : '添加' }}</button>
              </div>
            </div>
          </form>
        </DialogContent>
      </DialogPortal>
    </DialogRoot>
    <DialogRoot v-model:open="editingUpstream">
      <DialogPortal>
        <DialogOverlay class="editor-overlay" />
        <DialogContent as-child :aria-describedby="undefined">
          <form v-if="upstreamDraft" class="editor" :data-theme="theme" @submit.prevent="saveUpstream">
            <div class="editor-head">
              <DialogTitle class="editor-title">上游 / 出口设置</DialogTitle>
            </div>
            <div class="editor-body">
              <div class="scan-box">
                <div class="label-row">
                  <span class="scan-title">本机已开启服务 / 网卡</span>
                  <button v-if="desktop" type="button" class="inline-action" :disabled="scanBusy" @click="scanSystemUpstreams">
                    {{ scanBusy ? '扫描中...' : '重新扫描' }}
                  </button>
                </div>
                <div v-if="detectedProxies.length || networkInterfaces.length" class="scan-chips">
                  <button v-for="item in detectedProxies" :key="item.address" type="button" class="scan-chip"
                    :title="`${item.processName || '本地端口'} (${item.protocol.toUpperCase()})`"
                    @click="applyDetectedProxy(item)">
                    <span>{{ item.name }}</span>
                    <small>{{ item.address }}</small>
                  </button>
                  <button v-for="iface in networkInterfaces" :key="iface" type="button" class="scan-chip"
                    :title="`指定本机网卡出口：${iface}`"
                    @click="applyDetectedInterface(iface)">
                    <span>网卡</span>
                    <small>{{ iface }}</small>
                  </button>
                </div>
                <p v-else class="scan-empty">{{ !desktop ? '当前为浏览器预览，请在桌面端窗口中使用本机扫描' : (scanBusy ? '正在扫描本机监听端口与网卡...' : '未检测到常见本地代理端口') }}</p>
              </div>
              <label>出口名称<input v-model="upstreamDraft.name" required maxlength="40" placeholder="例如：本地代理 / 公司 WireGuard" /></label>
              <label v-if="upstreamDraft.protocol === 'wireguard'">
                <span class="label-row">
                  <span>WireGuard 配置（.conf）</span>
                  <button type="button" class="inline-action" @click="triggerWgImport">导入 .conf 文件</button>
                </span>
                <input ref="wgFileInput" type="file" accept=".conf,.txt" hidden @change="importWireGuardFile" />
                <textarea v-model="upstreamDraft.wireguardConf" rows="4" spellcheck="false" placeholder="[Interface]&#10;PrivateKey = ...&#10;Address = 10.0.0.2/32&#10;&#10;[Peer]&#10;PublicKey = ...&#10;Endpoint = wg.example.com:51820" />
              </label>
              <div v-else-if="upstreamDraft.protocol === 'interface'" class="iface-fields">
                <label>网卡名称<input v-model="upstreamDraft.address" required placeholder="wg0 / WLAN" /></label>
                <label v-if="interfaceOptions.length">
                  <span class="label-row">
                    <span>选择本机网卡</span>
                    <button type="button" class="inline-action" @click="loadNetworkInterfaces">刷新</button>
                  </span>
                  <PanelSelect v-model="upstreamDraft.address" :options="interfaceOptions" :theme="theme" label="选择网卡" />
                </label>
              </div>
              <label v-else>地址<input v-model="upstreamDraft.address" required placeholder="主机:端口（例如 127.0.0.1:7890）" /></label>
            </div>
            <div class="editor-actions">
              <PanelSelect v-model="upstreamDraft.protocol" :options="protocols" :theme="theme" label="协议" />
              <div class="editor-buttons">
                <DialogClose type="button">取消</DialogClose>
                <button type="submit" class="action" :disabled="!upstreamCanSave">保存</button>
              </div>
            </div>
          </form>
        </DialogContent>
      </DialogPortal>
    </DialogRoot>
    <DialogRoot v-model:open="editingBypass">
      <DialogPortal>
        <DialogOverlay class="editor-overlay" />
        <DialogContent as-child>
          <form class="editor" :data-theme="theme" @submit.prevent="saveBypass">
            <div class="editor-head">
              <DialogTitle class="editor-title">跳过代理域名</DialogTitle>
            </div>
            <div class="editor-body">
              <DialogDescription class="editor-note">跳过 TUN Fake-IP 并写入系统代理绕过列表，每行一条，支持通配符 *。</DialogDescription>
              <label>域名列表 <textarea v-model="bypassDraft" rows="5" placeholder="localhost&#10;*.localhost&#10;localhost.*&#10;*.local&#10;*.lan" /></label>
            </div>
            <div class="editor-actions">
              <span />
              <div class="editor-buttons">
                <DialogClose as-child><button type="button">取消</button></DialogClose>
                <button class="action" type="submit">保存</button>
              </div>
            </div>
          </form>
        </DialogContent>
      </DialogPortal>
    </DialogRoot>
    <DialogRoot v-model:open="editingDns">
      <DialogPortal>
        <DialogOverlay class="editor-overlay" />
        <DialogContent as-child>
          <form class="editor" :data-theme="theme" @submit.prevent="saveDns">
            <div class="editor-head">
              <DialogTitle class="editor-title">DNS 服务器设置</DialogTitle>
            </div>
            <div class="editor-body">
              <DialogDescription class="editor-note">支持 IP、DoH（https://...）或 DoT（tls://...），每行一条；留空保存则恢复默认。</DialogDescription>
              <label>国内 DNS（WLAN 直连 / TUN 基础解析）<textarea v-model="dnsDraft.cnDns" rows="3" spellcheck="false" placeholder="223.5.5.5&#10;119.29.29.29" /></label>
              <label>国外 DNS（经上游代理 / WireGuard 解析）<textarea v-model="dnsDraft.overseasDns" rows="2" spellcheck="false" placeholder="https://dns.google/dns-query" /></label>
            </div>
            <div class="editor-actions">
              <button type="button" @click="resetDnsDraft">恢复默认</button>
              <div class="editor-buttons">
                <DialogClose as-child><button type="button">取消</button></DialogClose>
                <button class="action" type="submit">保存</button>
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
.panel-header-actions .add-item { display: grid; place-items: center; width: 32px; height: 32px; padding: 0; border-radius: 999px; color: var(--accent); }
.panel-header-actions .add-item:hover { background: var(--hover); }
.theme-toggle { position: relative; display: flex; padding: 2px; border: 1px solid var(--edge); border-radius: 999px; background: var(--field); }
.theme-toggle::before { content: ''; position: absolute; top: 2px; left: 2px; width: 29px; height: 26px; border-radius: 999px; background: color-mix(in srgb, var(--accent) 18%, var(--surface)); transition: transform .18s ease; pointer-events: none; }
.theme-toggle[data-active="light"]::before { transform: translateX(29px); }
.theme-toggle[data-active="dark"]::before { transform: translateX(58px); }
.theme-toggle button { position: relative; display: grid; place-items: center; width: 29px; height: 26px; padding: 0; border: 0; border-radius: 999px; background: transparent; color: var(--muted); }
.theme-toggle button[aria-pressed="true"] { color: var(--accent); }
@media (prefers-reduced-motion: reduce) { .theme-toggle::before { transition: none; } }
.tabs { padding: 3px; border-radius: 10px; background: var(--field); }
.tabs button { flex: 1; background: transparent; border-color: transparent; }
.tabs button[aria-selected="true"] { border-color: var(--edge); color: var(--accent); }
.panel-tabs { display: flex; flex: 1; flex-direction: column; min-height: 0; }
.panel-tabs > :deep([role="tabpanel"]:not([hidden])) { flex: 1; min-height: 0; }
.panel-scroll { box-sizing: border-box; height: 100%; overflow-y: auto; padding: 2px; }
.rule-grid { display: grid; grid-template-columns: minmax(0, 1fr) minmax(104px, .72fr) 72px; gap: 8px; align-items: center; }
.rule-row { margin-bottom: 7px; padding: 8px; border: 1px solid var(--edge); border-radius: 10px; }
.rule-row { background: color-mix(in srgb, var(--field) 40%, var(--surface)); }
.rule-row:focus-within { border-color: color-mix(in srgb, var(--accent) 45%, var(--edge)); }
.group-summary { display: flex; align-items: baseline; gap: 6px; min-width: 0; font-size: 12px; }
.group-summary strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.group-summary small { flex: none; color: var(--muted); white-space: nowrap; }
.editor-overlay { position: fixed; inset: 0; z-index: 9; background: transparent; }
.editor { position: fixed; z-index: 10; inset: 50% auto auto 50%; transform: translate(-50%, -50%); display: flex; flex-direction: column; box-sizing: border-box; width: min(372px, calc(100vw - 36px)); max-height: min(318px, calc(100vh - 64px)); overflow: hidden; padding: 13px 15px; border: 1px solid var(--edge); border-radius: 16px; background: #161f21; color: #eaf4f1;  }
.editor[data-theme="light"] { color-scheme: light; background: #f6faf8; color: #18322b;  }
.editor-head { flex: none; padding-bottom: 6px; }
.editor-body { flex: 1; min-height: 0; overflow-y: auto; padding-right: 2px; }
.editor-title { margin: 0; font-size: 15px; }
.editor-note { margin: 0 0 9px; font-size: 11px; color: var(--muted); }
.editor label { display: grid; gap: 3px; margin-bottom: 8px; font-size: 12px; }
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
.panel { display: flex; flex-direction: column; box-sizing: border-box; height: 350px; padding: 14px; border-radius: 18px; background: var(--surface); backdrop-filter: blur(24px) saturate(180%); border: 1px solid var(--edge); box-shadow: 0 4px 12px #0000001c, inset 0 1px #ffffff0a; }
:where(.panel) { margin-top: 8px; }
.panel-header { display: flex; align-items: center; justify-content: space-between; margin-bottom: 12px; }
.tabs-wrap { margin-bottom: 12px; }
.panel > header, .panel > footer, .panel-tabs > :first-child { flex: none; }
.panel-title { font-size: 13px; font-weight: 500; letter-spacing: -.01em; }
.tabs { display: flex; position: relative; gap: 0; padding: 3px; background: var(--field); border-radius: 9px; }
.tabs::before { content: ''; position: absolute; top: 3px; bottom: 3px; left: 3px; width: calc((100% - 6px) / 5); background: var(--selected); border-radius: 6px; box-shadow: 0 1px 3px #0001; transition: transform .18s ease; }
.tabs[data-active="history"]::before { transform: translateX(100%); }
.tabs[data-active="rules"]::before { transform: translateX(200%); }
.tabs[data-active="upstreams"]::before { transform: translateX(300%); }
.tabs[data-active="settings"]::before { transform: translateX(400%); }
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
.editor { background: var(--surface); color: var(--foreground); border-color: var(--edge); backdrop-filter: blur(24px) saturate(180%); box-shadow: 0 8px 20px rgba(0, 0, 0, .24); }
.editor input, .editor textarea { border: 0; background: var(--hover); padding: 6px 8px; transition: background-color .15s ease; }
.editor input:hover, .editor textarea:hover { background: color-mix(in srgb, var(--foreground) 10%, transparent); }
.editor input:focus, .editor input:focus-visible, .editor textarea:focus, .editor textarea:focus-visible { outline: none; background: color-mix(in srgb, var(--accent) 16%, var(--hover)); }
@media (max-width: 420px) { .rule-grid { grid-template-columns: minmax(0, 1fr) minmax(0, 34%) 52px; gap: 4px; } .group-summary { gap: 3px; } }
.domain-row { grid-template-columns: minmax(0, 1fr) minmax(0, 28%) 122px; }
.rule-actions .sort-rule { width: 20px; height: 24px; }
.rule-actions .sort-rule:disabled { opacity: .28; cursor: default; pointer-events: none; }
.rule-list-move { transition: transform .18s ease; }
.is-disabled .group-summary strong, .is-disabled .route-cell { color: var(--muted); }
.rule-actions .rule-switch, .footer-switch .compact-switch { display: flex; align-items: center; width: 27px; height: 16px; padding: 2px; border-radius: 999px; background: var(--selected); }
.rule-actions .rule-switch[data-state="checked"], .footer-switch .compact-switch[data-state="checked"] { background: color-mix(in srgb, var(--accent) 35%, var(--field)); }
.panel-footer { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 8px 10px; margin-top: 12px; }
.editor-actions { flex: none; display: flex; align-items: center; justify-content: space-between; gap: 8px; padding-top: 6px; }
.editor-actions :deep(.picker) { min-width: 0; max-width: 172px; }
.editor-buttons { display: flex; gap: 8px; flex: none; }
.footer-switches { display: flex; align-items: center; gap: 10px; margin-left: auto; }
.footer-switch { display: flex; align-items: center; gap: 5px; color: var(--muted); font-size: 11px; white-space: nowrap; cursor: pointer; }
.footer-switch.is-busy, .footer-switch.is-busy .compact-switch { cursor: progress; opacity: .85; }
.panel-footer .fallback { min-width: 0; }
.panel-footer .fallback-link { border: 0; background: transparent; padding: 2px 4px; border-radius: 6px; color: var(--muted); font-size: 11px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 190px; text-align: left; }
.panel-footer .fallback-link:hover { background: var(--hover); color: var(--foreground); }
.setting-row { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
.setting-row .group-summary { display: grid; gap: 2px; }
.setting-row :deep(.picker) { width: 132px; flex: none; }
.setting-btn { display: inline-flex; align-items: center; gap: 5px; height: 26px; padding: 0 10px; border: 0; border-radius: 7px; background: var(--hover); color: var(--foreground); font-size: 11px; flex: none; }
.setting-btn:hover:not(:disabled) { background: color-mix(in srgb, var(--accent) 16%, var(--hover)); color: var(--accent); }
.setting-btn.danger:hover:not(:disabled) { background: color-mix(in srgb, #ef4444 16%, var(--hover)); color: #ef4444; }
.setting-btn:disabled { opacity: .45; cursor: default; }
.is-spinning { animation: switch-spin .8s linear infinite; }
.panel-footer :deep(.picker) { box-sizing: border-box; width: 96px; }
.panel-footer :deep(.picker > span:first-child) { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
.panel-footer :deep(.picker svg) { flex: none; }
.rule-switch-thumb { position: relative; display: block; width: 10px; height: 10px; border-radius: 50%; background: var(--muted); transition: transform .15s ease; }
.rule-switch-thumb[data-state="checked"] { transform: translateX(11px); background: var(--accent); }
.rule-switch-thumb.is-loading { background: transparent; }
.rule-switch-thumb.is-loading::after { content: ''; position: absolute; inset: 0; box-sizing: border-box; border-radius: 50%; border: 1.5px solid color-mix(in srgb, var(--accent) 28%, transparent); border-top-color: var(--accent); animation: switch-spin .7s linear infinite; }
.rule-switch-thumb[data-state="checked"].is-loading::after { border-color: color-mix(in srgb, var(--foreground) 25%, transparent); border-top-color: var(--foreground); }
@keyframes switch-spin { to { transform: rotate(360deg); } }
.label-row { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
.inline-action { border: 0; border-radius: 6px; padding: 2px 7px; background: color-mix(in srgb, var(--accent) 14%, transparent); color: var(--accent); font-size: 11px; cursor: pointer; }
.inline-action:hover { background: color-mix(in srgb, var(--accent) 24%, transparent); }
.scan-box { min-width: 0; overflow: hidden; margin-bottom: 8px; padding: 6px 8px; border-radius: 9px; background: var(--field); }
.scan-title { font-size: 11px; color: var(--muted); }
.scan-chips { display: flex; flex-wrap: wrap; gap: 5px; margin-top: 5px; max-height: 54px; overflow-y: auto; overflow-x: hidden; }
.scan-chip { display: inline-flex; align-items: center; gap: 5px; box-sizing: border-box; max-width: 30%; min-width: 0; padding: 3px 7px; border: 1px solid var(--edge); border-radius: 6px; background: var(--surface); font-size: 11px; cursor: pointer; }
.scan-chip > span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.scan-chip small { flex: 0 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--muted); font-family: "SF Mono", Consolas, monospace; font-size: 10px; }
.scan-chip:hover { border-color: var(--accent); color: var(--accent); }
.scan-empty { margin: 5px 0 0; font-size: 11px; color: var(--muted); }
.iface-fields { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; }
.iface-fields > label:only-child { grid-column: 1 / -1; }
.rule-table-section { display: grid; gap: 5px; margin-bottom: 6px; }
.rule-table-label { font-size: 12px; }
.rule-table-label small { color: var(--muted); }
.rule-mini-table { max-height: 122px; overflow-y: auto; border: 1px solid var(--edge); border-radius: 9px; background: var(--field); padding: 4px; }
.rule-mini-empty { padding: 14px 8px; text-align: center; font-size: 11px; color: var(--muted); }
.rule-mini-row { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 6px; padding: 4px 6px; border-radius: 6px; }
.rule-mini-row:hover { background: var(--hover); }
.rule-kind-badge { flex: none; padding: 1px 5px; border-radius: 4px; background: color-mix(in srgb, var(--accent) 14%, transparent); color: var(--accent); font-size: 10px; line-height: 1.4; }
.rule-mini-val { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.rule-mini-ops { display: flex; align-items: center; gap: 2px; }
.mini-op-btn { display: grid; place-items: center; width: 22px; height: 22px; padding: 0; border: 0; border-radius: 5px; background: transparent; color: var(--muted); cursor: pointer; }
.mini-op-btn:hover { background: color-mix(in srgb, var(--accent) 14%, transparent); color: var(--accent); }
.mini-op-btn.danger:hover { background: color-mix(in srgb, #ef4444 14%, transparent); color: #ef4444; }
.editor-overlay-sub { z-index: 19; background: rgba(0, 0, 0, .18); }
.editor-sub { z-index: 20; width: min(338px, calc(100vw - 48px)); }
.kind-chip-row { display: flex; flex-wrap: wrap; gap: 5px; margin-bottom: 8px; }
.preset-chip { padding: 3px 8px; border: 1px solid var(--edge); border-radius: 6px; background: var(--field); color: var(--muted); font-size: 11px; line-height: 1.4; cursor: pointer; }
.preset-chip:hover { border-color: var(--accent); color: var(--foreground); }
.preset-chip.active { border-color: var(--accent); background: color-mix(in srgb, var(--accent) 16%, var(--field)); color: var(--accent); font-weight: 500; }
.rule-preview { margin: 2px 0 4px; font-size: 11px; color: var(--muted); word-break: break-all; }
.rule-preview code { font-family: "SF Mono", Consolas, monospace; color: var(--accent); }
@media (prefers-reduced-motion: reduce) { .tabs::before, .rule-switch-thumb { transition: none; } .rule-switch-thumb.is-loading::after { animation: none; } }
</style>
