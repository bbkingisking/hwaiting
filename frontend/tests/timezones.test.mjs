import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import ts from 'typescript'

const source = readFileSync(new URL('../src/lib/timezones.ts', import.meta.url), 'utf8')
const { outputText } = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2020 },
})
const { timezoneOptions } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`)

test('sorts by numeric UTC offset across date boundaries, including fractional hours', () => {
  const zones = timezoneOptions(['Asia/Kathmandu', 'Pacific/Kiritimati', 'UTC', 'America/St_Johns', 'Pacific/Honolulu', 'UTC'], new Date('2026-01-01T23:45:00Z'))
  assert.deepEqual(zones.map(({ minutes }) => minutes), [-600, -210, 0, 345, 840])
  assert.equal(zones[3].label, '(UTC+05:45) Asia/Kathmandu')
  assert.equal(zones[1].label, '(UTC−03:30) America/St Johns')
})

test('offsets follow daylight saving time at the supplied instant', () => {
  const winter = timezoneOptions(['Europe/London'], new Date('2026-01-01T00:00:00Z'))
  const summer = timezoneOptions(['Europe/London'], new Date('2026-07-01T00:00:00Z'))
  assert.equal(winter[0].minutes, 0)
  assert.equal(summer[0].minutes, 60)
})
