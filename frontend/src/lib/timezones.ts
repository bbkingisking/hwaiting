// Offsets are evaluated at one instant so DST and fractional-hour zones are
// represented consistently. Store the IANA name, never the displayed offset.
export function timezoneOptions(zones: string[], now = new Date()) {
  return [...new Set(zones)].map(zone => {
    const parts = new Intl.DateTimeFormat('en-US', {
      timeZone: zone,
      year: 'numeric', month: '2-digit', day: '2-digit',
      hour: '2-digit', minute: '2-digit', second: '2-digit', hourCycle: 'h23',
    }).formatToParts(now)
    const value = (type: Intl.DateTimeFormatPartTypes) => Number(parts.find(part => part.type === type)!.value)
    const local = Date.UTC(value('year'), value('month') - 1, value('day'), value('hour'), value('minute'), value('second'))
    const minutes = Math.round((local - Math.floor(now.getTime() / 1000) * 1000) / 60000)
    const absolute = Math.abs(minutes)
    const offset = `${minutes < 0 ? '−' : '+'}${String(Math.floor(absolute / 60)).padStart(2, '0')}:${String(absolute % 60).padStart(2, '0')}`
    return { zone, minutes, label: `(UTC${offset}) ${zone.replace(/_/g, ' ')}` }
  }).sort((a, b) => a.minutes - b.minutes || a.zone.localeCompare(b.zone, 'en'))
}
