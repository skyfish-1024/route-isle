import { buildRouteLookup, parseMihomoLogPayload } from './routeMatcher'

const DEFAULT_CONTROLLER = {
  host: '127.0.0.1',
  port: 17891,
  secret: 'routeisle-local-secret',
}

export function subscribeMihomoTraffic({
  trafficRef,
  rules,
  controller = DEFAULT_CONTROLLER,
  onDisconnect,
}) {
  const {
    host = DEFAULT_CONTROLLER.host,
    port = DEFAULT_CONTROLLER.port,
    secret = DEFAULT_CONTROLLER.secret,
  } = controller || DEFAULT_CONTROLLER

  const baseWsUrl = `ws://${host}:${port}`
  const tokenParam = `token=${encodeURIComponent(secret)}`

  let stopped = false
  let logsWs = null
  let connsWs = null
  let trafficWs = null
  let reconnectTimers = []
  let consecutiveFailures = 0

  let seeded = false
  let rawPrevUploadTotal = 0
  let rawPrevDownloadTotal = 0
  let prevConnBytes = new Map()
  let lastRoute = null
  let latestWsRate = { up: 0, down: 0 }

  function ensureBuckets(lookup, sampleByExit, sampleByRule) {
    const t = trafficRef.value
    for (const [id, name] of lookup.activeExits) {
      sampleByExit[id] = { name, upload: 0, download: 0 }
      t.byExit[id] ??= { name, upload: 0, download: 0 }
      t.byExit[id].name = name
    }
    for (const [id, name] of lookup.activeRules) {
      sampleByRule[id] = { name, upload: 0, download: 0 }
      t.byRule[id] ??= { name, upload: 0, download: 0 }
      t.byRule[id].name = name
    }
  }

  function addBucketTraffic(map, id, name, up, down) {
    map[id] ??= { name, upload: 0, download: 0 }
    map[id].name = name
    map[id].upload += up
    map[id].download += down
  }

  function addPairTraffic(map, exitId, exitName, ruleId, ruleName, up, down) {
    const key = `${exitId}\0${ruleId}`
    map[key] ??= { exit: exitName, rule: ruleName, upload: 0, download: 0 }
    map[key].exit = exitName
    map[key].rule = ruleName
    map[key].upload += up
    map[key].download += down
  }

  function handleConnectionsSnapshot(parsed) {
    const t = trafficRef.value
    const lookup = buildRouteLookup(rules)
    const sampleByExit = {}
    const sampleByRule = {}
    ensureBuckets(lookup, sampleByExit, sampleByRule)

    const uploadTotal = Number(parsed.uploadTotal) || 0
    const downloadTotal = Number(parsed.downloadTotal) || 0
    const firstSeed = !seeded
    const rawDeltaUp = firstSeed ? 0 : Math.max(0, uploadTotal - rawPrevUploadTotal)
    const rawDeltaDown = firstSeed ? 0 : Math.max(0, downloadTotal - rawPrevDownloadTotal)
    rawPrevUploadTotal = uploadTotal
    rawPrevDownloadTotal = downloadTotal
    seeded = true

    let activeUserUp = 0
    let activeUserDown = 0
    let internalDeltaUp = 0
    let internalDeltaDown = 0
    const currentConnBytes = new Map()

    for (const conn of parsed.connections || []) {
      const connUp = Number(conn.upload) || 0
      const connDown = Number(conn.download) || 0
      const prev = prevConnBytes.get(conn.id) || { up: 0, down: 0 }
      const du = Math.max(0, connUp - prev.up)
      const dd = Math.max(0, connDown - prev.down)
      currentConnBytes.set(conn.id, { up: connUp, down: connDown })

      const metaHost = conn.metadata?.host || ''
      const sniffHost = conn.metadata?.sniffHost || ''
      const isInternal = conn.rule === 'ProcessName' || (!conn.rule && metaHost === 'dns.google')

      if (isInternal) {
        internalDeltaUp += du
        internalDeltaDown += dd
        continue
      }

      if (du === 0 && dd === 0) continue

      const outboundHint = Array.isArray(conn.chains) && conn.chains.length ? conn.chains[0] : ''
      const payloadStr = conn.rule === 'Match'
        ? ''
        : (conn.rulePayload || sniffHost || metaHost)
      const { ruleId, ruleName, exitId, exitName } = lookup.resolve(payloadStr, outboundHint)

      if (firstSeed) {
        t.upload += du
        t.download += dd
        addBucketTraffic(t.byRule, ruleId, ruleName, du, dd)
        addBucketTraffic(t.byExit, exitId, exitName, du, dd)
        addPairTraffic(t.byPair ??= {}, exitId, exitName, ruleId, ruleName, du, dd)
      } else {
        activeUserUp += du
        activeUserDown += dd
        addBucketTraffic(sampleByRule, ruleId, ruleName, du, dd)
        addBucketTraffic(t.byRule, ruleId, ruleName, du, dd)
        addBucketTraffic(sampleByExit, exitId, exitName, du, dd)
        addBucketTraffic(t.byExit, exitId, exitName, du, dd)
        addPairTraffic(t.byPair ??= {}, exitId, exitName, ruleId, ruleName, du, dd)
      }
    }

    prevConnBytes = currentConnBytes

    let uploadRate = 0
    let downloadRate = 0

    if (!firstSeed) {
      const remUp = Math.max(0, rawDeltaUp - (activeUserUp + internalDeltaUp))
      const remDown = Math.max(0, rawDeltaDown - (activeUserDown + internalDeltaDown))
      if (remUp > 0 || remDown > 0) {
        const route = lastRoute || {
          ruleId: 'unmatched',
          ruleName: '未匹配',
          exitId: lookup.fallbackExitId,
          exitName: lookup.fallbackExitName,
        }
        addBucketTraffic(sampleByRule, route.ruleId, route.ruleName, remUp, remDown)
        addBucketTraffic(t.byRule, route.ruleId, route.ruleName, remUp, remDown)
        addBucketTraffic(sampleByExit, route.exitId, route.exitName, remUp, remDown)
        addBucketTraffic(t.byExit, route.exitId, route.exitName, remUp, remDown)
        addPairTraffic(t.byPair ??= {}, route.exitId, route.exitName, route.ruleId, route.ruleName, remUp, remDown)
      }
      uploadRate = activeUserUp + remUp
      downloadRate = activeUserDown + remDown
      if (uploadRate === 0 && downloadRate === 0 && internalDeltaUp === 0 && internalDeltaDown === 0) {
        uploadRate = latestWsRate.up
        downloadRate = latestWsRate.down
      }
      t.upload += uploadRate
      t.download += downloadRate
    }

    t.uploadRate = uploadRate
    t.downloadRate = downloadRate
    t.history.push({
      timestamp: Date.now(),
      upload: uploadRate,
      download: downloadRate,
      byExit: sampleByExit,
      byRule: sampleByRule,
    })
    if (t.history.length > 60) t.history.shift()
  }

  function scheduleReconnect(fn, delay = 500) {
    if (stopped) return
    const timer = setTimeout(fn, delay)
    reconnectTimers.push(timer)
  }

  function connectLogs() {
    if (stopped) return
    try {
      logsWs = new WebSocket(`${baseWsUrl}/logs?level=info&${tokenParam}`)
      logsWs.onmessage = (event) => {
        try {
          const item = JSON.parse(event.data)
          if (!item?.payload) return
          const lookup = buildRouteLookup(rules)
          const entry = parseMihomoLogPayload(item.payload, lookup)
          if (!entry) return
          if (!entry.failed) {
            lastRoute = {
              ruleId: entry.ruleId,
              ruleName: entry.rule,
              exitId: entry.exitId,
              exitName: entry.exit,
            }
          }
          trafficRef.value.logs.push({
            timestamp: entry.timestamp,
            host: entry.host,
            rule: entry.rule,
            exit: entry.exit,
            failed: entry.failed,
            reason: entry.reason,
          })
          if (trafficRef.value.logs.length > 120) {
            trafficRef.value.logs.splice(0, trafficRef.value.logs.length - 120)
          }
        } catch {
          // ignore malformed log frame
        }
      }
      logsWs.onclose = () => {
        if (!stopped) scheduleReconnect(connectLogs, 600)
      }
    } catch {
      scheduleReconnect(connectLogs, 600)
    }
  }

  function connectTraffic() {
    if (stopped) return
    try {
      trafficWs = new WebSocket(`${baseWsUrl}/traffic?${tokenParam}`)
      trafficWs.onmessage = (event) => {
        try {
          const data = JSON.parse(event.data)
          latestWsRate = {
            up: Number(data.up) || 0,
            down: Number(data.down) || 0,
          }
        } catch {
          // ignore malformed frame
        }
      }
      trafficWs.onclose = () => {
        if (!stopped) scheduleReconnect(connectTraffic, 600)
      }
    } catch {
      scheduleReconnect(connectTraffic, 600)
    }
  }

  function connectConnections() {
    if (stopped) return
    try {
      connsWs = new WebSocket(`${baseWsUrl}/connections?${tokenParam}`)
      connsWs.onopen = () => {
        consecutiveFailures = 0
      }
      connsWs.onmessage = (event) => {
        consecutiveFailures = 0
        try {
          const parsed = JSON.parse(event.data)
          handleConnectionsSnapshot(parsed)
        } catch {
          // ignore malformed snapshot
        }
      }
      connsWs.onclose = () => {
        if (stopped) return
        consecutiveFailures += 1
        if (consecutiveFailures >= 6) {
          onDisconnect?.()
          return
        }
        scheduleReconnect(connectConnections, 500)
      }
    } catch {
      consecutiveFailures += 1
      if (consecutiveFailures >= 6) {
        onDisconnect?.()
        return
      }
      scheduleReconnect(connectConnections, 500)
    }
  }

  connectLogs()
  connectTraffic()
  connectConnections()

  return () => {
    stopped = true
    for (const timer of reconnectTimers) clearTimeout(timer)
    reconnectTimers = []
    for (const ws of [logsWs, trafficWs, connsWs]) {
      if (ws) {
        ws.onclose = null
        ws.onerror = null
        ws.onmessage = null
        try { ws.close() } catch {}
      }
    }
  }
}
