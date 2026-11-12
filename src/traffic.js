import { domainsOf, formatBytes } from './utils/format'
import { subscribeMihomoTraffic } from './services/mihomoClient'

export { formatBytes, subscribeMihomoTraffic }

export function createTraffic() {
  return {
    upload: 0, download: 0, uploadRate: 0, downloadRate: 0,
    history: [], logs: [], byExit: {}, byRule: {}, byPair: {},
  }
}

let nextSimLogId = 1

export function simulateTraffic(traffic, rules) {
  const routes = rules.groups
    .filter(group => group.enabled !== false)
    .map(group => ({ id: group.id, name: group.name, target: group.target, domains: domainsOf(group.domains) }))
  routes.push({ id: 'unmatched', name: '未匹配', target: rules.fallback, domains: ['www.example.com'] })
  const exitNames = { system: 'Windows 选路', wlan: 'WLAN 直连', reject: '拒绝连接' }
  for (const upstream of rules.upstreams) exitNames[upstream.id] = upstream.name
  traffic.uploadRate = 0
  traffic.downloadRate = 0
  const byExit = {}
  const byRule = {}
  const timestamp = Date.now()
  const loggedRoute = routes[Math.floor(Math.random() * routes.length)]
  let reason = ''
  if (loggedRoute.target === 'reject') {
    reason = loggedRoute.id === 'unmatched' ? '默认规则拒绝' : '命中拒绝规则'
  } else if (Math.random() < 0.12) {
    reason = '连接超时'
  }
  for (const route of routes) {
    if (route.target === 'reject') continue
    const connected = route !== loggedRoute || !reason
    const upload = connected ? Math.round(600 + Math.random() * 12_000) : 0
    const download = connected ? Math.round(4000 + Math.random() * 170_000) : 0
    const rule = byRule[route.id] = { name: route.name, upload: 0, download: 0 }
    const exit = byExit[route.target] ??= { name: exitNames[route.target] || '未命名出口', upload: 0, download: 0 }
    for (const bucket of [rule, exit, traffic]) {
      bucket.upload += upload
      bucket.download += download
    }
    for (const [buckets, id, name] of [
      [traffic.byRule, route.id, rule.name],
      [traffic.byExit, route.target, exit.name],
    ]) {
      buckets[id] ??= { upload: 0, download: 0 }
      buckets[id].name = name
      buckets[id].upload += upload
      buckets[id].download += download
    }
    const pairKey = `${route.target}\0${route.id}`
    const pair = traffic.byPair[pairKey] ??= { exit: exit.name, rule: rule.name, upload: 0, download: 0 }
    pair.exit = exit.name
    pair.rule = rule.name
    pair.upload += upload
    pair.download += download
    traffic.uploadRate += upload
    traffic.downloadRate += download
  }
  traffic.logs.push({
    id: nextSimLogId++,
    timestamp,
    host: loggedRoute.domains[Math.floor(Math.random() * loggedRoute.domains.length)] || 'example.invalid',
    rule: loggedRoute.name || '未命名规则组',
    exit: exitNames[loggedRoute.target] || '未命名出口',
    failed: Boolean(reason),
    reason,
  })
  if (traffic.logs.length > 120) traffic.logs.shift()
  traffic.history.push({ timestamp, upload: traffic.uploadRate, download: traffic.downloadRate, byExit, byRule })
  if (traffic.history.length > 60) traffic.history.shift()
}
