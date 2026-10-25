import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { afterEach, beforeEach, test } from 'node:test'
import ts from 'typescript'

// Keep the timer tests dependency-free apart from the project's TypeScript.
const source = readFileSync(new URL('../src/lib/review-timing.ts', import.meta.url), 'utf8')
const { outputText } = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2020 },
})
const { parseDueAt, scheduleReviewRefresh } = await import(
  `data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`
)

const start = Date.parse('2026-10-25T12:00:00Z')
let cleanups

beforeEach(context => {
  cleanups = []
  context.mock.timers.enable({ apis: ['Date', 'setTimeout'], now: start })
  globalThis.window = new EventTarget()
  globalThis.document = Object.assign(new EventTarget(), { visibilityState: 'visible' })
})

afterEach(() => {
  cleanups.forEach(cleanup => cleanup())
  delete globalThis.window
  delete globalThis.document
})

function schedule(timestamp, refresh) {
  const cleanup = scheduleReviewRefresh(timestamp, refresh)
  cleanups.push(cleanup)
  return cleanup
}

test('SQLite timestamps and ISO timestamps refer to the same UTC instant', () => {
  assert.equal(parseDueAt('2026-10-25 12:15:00'), start + 15 * 60000)
  assert.equal(parseDueAt('2026-10-25T13:15:00+01:00'), start + 15 * 60000)
  assert.ok(Number.isNaN(parseDueAt('invalid')))
})

test('15 minutes of waiting makes no requests before a single due refresh', context => {
  let calls = 0
  schedule('2026-10-25T12:15:00Z', () => calls++)
  context.mock.timers.tick(15 * 60000)
  assert.equal(calls, 0)
  context.mock.timers.tick(1000)
  assert.equal(calls, 1)
  context.mock.timers.tick(60 * 60000)
  assert.equal(calls, 1)
})

test('hidden deadlines wait for the user to return, coalescing focus events', context => {
  let calls = 0
  schedule('2026-10-25T12:15:00Z', () => calls++)
  document.visibilityState = 'hidden'
  context.mock.timers.tick(16 * 60000)
  assert.equal(calls, 0)
  document.visibilityState = 'visible'
  document.dispatchEvent(new Event('visibilitychange'))
  window.dispatchEvent(new Event('focus'))
  assert.equal(calls, 1)
})

test('returning after five minutes leaves a 70-minute deadline untouched', context => {
  let calls = 0
  schedule('2026-10-25T13:10:00Z', () => calls++)
  document.visibilityState = 'hidden'
  context.mock.timers.tick(5 * 60000)
  document.visibilityState = 'visible'
  document.dispatchEvent(new Event('visibilitychange'))
  window.dispatchEvent(new Event('focus'))
  assert.equal(calls, 0)
  context.mock.timers.tick(65 * 60000)
  assert.equal(calls, 0)
  context.mock.timers.tick(1000)
  assert.equal(calls, 1)
})

test('cleanup cancels the old deadline and removes return listeners', context => {
  let calls = 0
  const cleanup = schedule('2026-10-25T12:15:00Z', () => calls++)
  cleanup()
  context.mock.timers.tick(16 * 60000)
  window.dispatchEvent(new Event('focus'))
  document.dispatchEvent(new Event('visibilitychange'))
  assert.equal(calls, 0)
})

test('deadlines beyond the browser timer limit do not refresh early', context => {
  let calls = 0
  const delay = 40 * 24 * 60 * 60000
  schedule(new Date(start + delay).toISOString(), () => calls++)
  context.mock.timers.tick(2 ** 31 - 1)
  assert.equal(calls, 0)
  context.mock.timers.tick(delay - (2 ** 31 - 1) + 1000)
  assert.equal(calls, 1)
})

test('a missing or invalid deadline makes no requests on return', context => {
  let calls = 0
  schedule(null, () => calls++)
  schedule('invalid', () => calls++)
  context.mock.timers.tick(24 * 60 * 60000)
  assert.equal(calls, 0)
  window.dispatchEvent(new Event('focus'))
  document.dispatchEvent(new Event('visibilitychange'))
  assert.equal(calls, 0)
})

test('an overdue deadline refreshes once without starting a retry loop', context => {
  let calls = 0
  schedule('2026-10-25T11:59:00Z', () => calls++)
  context.mock.timers.tick(0)
  assert.equal(calls, 1)
  context.mock.timers.tick(60 * 60000)
  assert.equal(calls, 1)
})

test('replacing a due date cancels the previous scheduled refresh', context => {
  let calls = 0
  const cleanup = schedule('2026-10-25T12:15:00Z', () => calls++)
  cleanup()
  schedule('2026-10-25T12:30:00Z', () => calls++)
  context.mock.timers.tick(15 * 60000 + 1000)
  assert.equal(calls, 0)
  context.mock.timers.tick(15 * 60000)
  assert.equal(calls, 1)
})
