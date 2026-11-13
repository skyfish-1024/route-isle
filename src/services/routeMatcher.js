import { domainsOf } from '../utils/format'

export function exitNameFor(target, upstreams = []) {
  if (target === 'system') return 'Windows 选路'
  if (target === 'wlan') return 'WLAN 直连'
  if (target === 'reject') return '拒绝连接'
  const upstream = upstreams.find(item => item.id === target)
  return upstream ? (upstream.name?.trim() || '未命名上游') : '未命名出口'
}

function compilePattern(pattern) {
  if (pattern.startsWith('*.')) {
    const suffix = pattern.slice(2)
    if (suffix && !suffix.includes('*') && !suffix.includes('?')) {
      return { kind: 'suffix', value: suffix }
    }
  }
  if (pattern.includes('*') || pattern.includes('?')) {
    const escaped = pattern
      .replace(/[.+^${}()|[\]\\]/g, '\\$&')
      .replace(/\*/g, '.*')
      .replace(/\?/g, '.')
    return { kind: 'wildcard', regex: new RegExp(`^${escaped}$`) }
  }
  return { kind: 'exact', value: pattern }
}

function matchesRule(clean, matcher) {
  if (matcher.kind === 'exact') return clean === matcher.value
  if (matcher.kind === 'suffix') return clean === matcher.value || clean.endsWith(`.${matcher.value}`)
  return matcher.regex.test(clean)
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

const isNonDomainRule = (pattern) =>
  pattern.startsWith('proc:') ||
  pattern.startsWith('process:') ||
  pattern.startsWith('port:') ||
  /^\d{1,3}(\.\d{1,3}){3}(\/\d{1,2})?$/.test(pattern) ||
  (pattern.includes(':') && /^[0-9a-f:]+(\/\d{1,3})?$/i.test(pattern))

    for (const raw of domainsOf(group.domains)) {
      const pattern = raw.replace(/^\.+/, '').toLowerCase()
      if (!pattern || seen.has(pattern) || isNonDomainRule(pattern)) continue
      seen.add(pattern)
      domains.push({
        pattern,
        matcher: compilePattern(pattern),
        ruleId: group.id,
        ruleName,
        exitId: group.target,
        exitName,
      })
    }
  }

  const cnFallbackExitId = rules.cnFallback || 'wlan'
  const fallbackExitId = rules.fallback || 'system'
  const fallbackExitName = exitNameFor(fallbackExitId, rules.upstreams)
  if (cnFallbackExitId !== fallbackExitId) {
    activeRules.push(['geoip-cn', 'GeoIP 国内直连'])
    activeExits.set(cnFallbackExitId, exitNameFor(cnFallbackExitId, rules.upstreams))
  }
  activeRules.push(['unmatched', '未匹配'])
  activeExits.set(fallbackExitId, fallbackExitName)

  function resolve(hostOrPayload = '', outboundHint = '', ruleType = '') {
    const rawStr = String(hostOrPayload).trim()
    if (ruleType === 'GeoIP' || /^GeoIP[(/]/.test(rawStr)) {
      const exit = (outboundHint && outboundToExit.get(outboundHint)) || {
        exitId: cnFallbackExitId,
        exitName: exitNameFor(cnFallbackExitId, rules.upstreams),
      }
      return {
        ruleId: 'geoip-cn',
        ruleName: 'GeoIP 国内直连',
        exitId: exit.exitId,
        exitName: exit.exitName,
      }
    }

    const clean = rawStr
      .replace(/^(DomainSuffix|DomainWildcard|Domain)\(/, '')
      .replace(/^(DomainSuffix|DomainWildcard|Domain)\//, '')
      .replace(/\)$/, '')
      .split(':')[0]
      .toLowerCase()

    if (clean) {
      for (const meta of domains) {
        if (matchesRule(clean, meta.matcher)) {
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
