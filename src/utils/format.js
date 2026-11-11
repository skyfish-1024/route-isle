export function formatBytes(bytes) {
  if (bytes < 1024) return `${bytes} B`
  const units = ['KB', 'MB', 'GB', 'TB']
  let value = bytes / 1024
  let unit = 0
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024
    unit++
  }
  return `${value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`
}

export function domainsOf(text = '') {
  return text.split(/[\s,]+/).filter(Boolean)
}

export function isOnDate(timestamp, date) {
  if (!date) return true
  const start = new Date(date + 'T00:00:00')
  const end = new Date(start)
  end.setDate(end.getDate() + 1)
  return timestamp >= start.getTime() && timestamp < end.getTime()
}
