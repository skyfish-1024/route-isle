import { domainsOf } from '../utils/format'

export function exitNameFor(target, upstreams = []) {
  if (target === 'system') return 'Windows 选路'
  if (target === 'wlan') return 'WLAN 直连'
  if (target === 'reject') return '拒绝连接'
  const upstream = upstreams.find(item => item.id === target)
  return upstream ? (upstream.name?.trim() || '未命名上游') : '未命名出口'
}

export function buildRouteLookup(rules) {
  const outboundToExit = new Map([
    ['DIRECT', { exitId: 'system', exitName: 'Windows 选路' }],
    ['PhysicalDirect', { exitId: 'wlan', exitName: 'WLAN 直连' }],
    ['REJECT', { exitId: 'reject', exitName: '拒绝连接' }],
  ])
  for (const upstream of rules.upstreams || []) {
    outboundToExit.set(`upstream_${upstream.id}`, {
      exitId: upstream.id,
      exitName: exitNameFor(upstream.id, rules.upstreams),
    })
  }

  const domains = []
  const seen = new Set()
  const activeRules = []
  const activeExits = new Map()

  for (const group of rules.groups || []) {
    if (group.enabled === false) continue
    const ruleName = group.name?.trim() || '未命名规则组'
    activeRules.push([group.id, ruleName])
    const exitName = exitNameFor(group.target, rules.upstreams)
    activeExits.set(group.target, exitName)

    for (const raw of domainsOf(group.domains)) {
      const suffix = raw.replace(/^\.+/, '').toLowerCase()
      if (!suffix || seen.has(suffix)) continue
      seen.add(suffix)
      domains.push({
        suffix,
        ruleId: group.id,
        ruleName,
        exitId: group.target,
        exitName,
      })
    }
  }

  domains.sort((a, b) => b.suffix.length - a.suffix.length)

  const fallbackExitId = rules.fallback || 'system'
  const fallbackExitName = exitNameFor(fallbackExitId, rules.upstreams)
  activeRules.push(['unmatched', '未匹配'])
  activeExits.set(fallbackExitId, fallbackExitName)

  function resolve(hostOrPayload = '', outboundHint = '') {
    const clean = String(hostOrPayload)
      .trim()
      .replace(/^DomainSuffix\(/, '')
      .replace(/^DomainSuffix\//, '')
      .replace(/\)$/, '')
      .split(':')[0]
      .toLowerCase()

    if (clean) {
      for (const meta of domains) {
        if (clean === meta.suffix || clean.endsWith(`.${meta.suffix}`)) {
          return {
            ruleId: meta.ruleId,
            ruleName: meta.ruleName,
            exitId: meta.exitId,
            exitName: meta.exitName,
          }
        }
      }
    }

    if (outboundHint && outboundToExit.has(outboundHint)) {
      const { exitId, exitName } = outboundToExit.get(outboundHint)
      return {
        ruleId: 'unmatched',
        ruleName: '未匹配',
        exitId,
        exitName,
      }
    }

    return {
      ruleId: 'unmatched',
      ruleName: '未匹配',
      exitId: fallbackExitId,
      exitName: fallbackExitName,
    }
  }

  return {
    activeRules,
    activeExits: Array.from(activeExits.entries()),
    fallbackExitId,
    fallbackExitName,
    resolve,
  }
}

export function parseMihomoLogPayload(msg = '', lookup) {
  const trimmed = msg.trim()
  if (!trimmed.startsWith('[TCP]') && !trimmed.startsWith('[UDP]')) return null
  if (trimmed.includes('apex -->') || trimmed.includes('ProcessName')) return null

  const dialPrefix = trimmed.startsWith('[TCP] dial ')
    ? '[TCP] dial '
    : trimmed.startsWith('[UDP] dial ')
      ? '[UDP] dial '
      : null

  if (dialPrefix) {
    const dialRest = trimmed.slice(dialPrefix.length)
    const outbound = dialRest.split(/\s+/)[0] || ''
    const arrowPos = dialRest.indexOf('--> ')
    if (arrowPos < 0) return null
    const afterArrow = dialRest.slice(arrowPos + 4)
    const [targetAddr, errRaw = '连接失败'] = afterArrow.split(' error: ')
    const lastColon = targetAddr.lastIndexOf(':')
    const host = (lastColon > 0 ? targetAddr.slice(0, lastColon) : targetAddr).trim()

    let matchPayload = host
    const matchIdx = dialRest.indexOf('(match ')
    if (matchIdx >= 0) {
      matchPayload = dialRest.slice(matchIdx + 7).split(')')[0] || host
    }

    const { ruleId, ruleName, exitId, exitName } = lookup.resolve(
      matchPayload.startsWith('Match') ? host : matchPayload,
      outbound,
    )

    const reason = errRaw.includes('i/o timeout') || errRaw.includes('timed out')
      ? '连接超时'
      : errRaw.includes('refused') || errRaw.includes('connectex')
        ? '连接被拒绝'
        : '连接失败'

    return {
      timestamp: Date.now(),
      host,
      rule: ruleName,
      exit: exitName,
      failed: true,
      reason,
      ruleId,
      exitId,
    }
  }

  const arrowPos = trimmed.indexOf('--> ')
  if (arrowPos < 0) return null
  const afterArrow = trimmed.slice(arrowPos + 4)
  const matchSplit = afterArrow.split(' match ')
  if (matchSplit.length < 2) return null
  const [targetAddr, matchPart] = matchSplit
  const lastColon = targetAddr.lastIndexOf(':')
  const host = (lastColon > 0 ? targetAddr.slice(0, lastColon) : targetAddr).trim()
  const [ruleExpr, rawOutbound = ''] = matchPart.split(' using ')
  const outbound = rawOutbound.trim()
  if (ruleExpr.startsWith('ProcessName')) return null

  const { ruleId, ruleName, exitId, exitName } = lookup.resolve(
    ruleExpr.startsWith('Match') ? host : ruleExpr,
    outbound,
  )

  const failed = outbound === 'REJECT' || exitId === 'reject'
  const reason = failed
    ? (ruleId === 'unmatched' ? '默认规则拒绝' : '命中拒绝规则')
    : ''

  return {
    timestamp: Date.now(),
    host,
    rule: ruleName,
    exit: exitName,
    failed,
    reason,
    ruleId,
    exitId,
  }
}
