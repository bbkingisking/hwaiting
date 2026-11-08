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

afterEach(context => {
  cleanups.forEach(cleanup => cleanup())
  context.mock.timers.reset()
  delete globalThis.window
  delete globalThis.document
})

function schedule(timestamp, refresh) {
  const cleanup = scheduleReviewRefresh(timestamp, refresh)
  cleanups.push(cleanup)
  return cleanup
}

async function settleRefresh() {
  await Promise.resolve()
  await Promise.resolve()
}

test('SQLite timestamps and ISO timestamps refer to the same UTC instant', () => {
  assert.equal(parseDueAt('2026-10-25 12:15:00'), start + 15 * 60000)
  assert.equal(parseDueAt('2026-10-25T13:15:00+01:00'), start + 15 * 60000)
  assert.ok(Number.isNaN(parseDueAt('invalid')))
})

test('15 minutes of waiting makes no requests before a single due refresh', async context => {
  let calls = 0
  schedule('2026-10-25T12:15:00Z', () => calls++)
  context.mock.timers.tick(15 * 60000)
  assert.equal(calls, 0)
  context.mock.timers.tick(1000)
  await settleRefresh()
  assert.equal(calls, 1)
  context.mock.timers.tick(60 * 60000)
  assert.equal(calls, 1)
})

test('hidden tabs refresh at the deadline without polling, coalescing immediate return events', async context => {
  let calls = 0
  schedule('2026-10-25T12:15:00Z', () => calls++)
  document.visibilityState = 'hidden'
  document.dispatchEvent(new Event('visibilitychange'))
  context.mock.timers.tick(15 * 60000)
  assert.equal(calls, 0)
  context.mock.timers.tick(1000)
  await settleRefresh()
  assert.equal(calls, 1)
  document.visibilityState = 'visible'
  document.dispatchEvent(new Event('visibilitychange'))
  window.dispatchEvent(new Event('focus'))
  assert.equal(calls, 1)
  document.visibilityState = 'hidden'
  document.dispatchEvent(new Event('visibilitychange'))
  context.mock.timers.tick(60 * 60000)
  assert.equal(calls, 1)
})

test('returning catches up when a background timer has been suspended', async context => {
  let calls = 0
  schedule('2026-10-25T12:15:00Z', () => calls++)
  document.visibilityState = 'hidden'
  // Move the clock without executing timers, as if the browser suspended them.
  context.mock.timers.setTime(start + 16 * 60000)
  document.dispatchEvent(new Event('visibilitychange'))
  window.dispatchEvent(new Event('focus'))
  assert.equal(calls, 0)
  document.visibilityState = 'visible'
  document.dispatchEvent(new Event('visibilitychange'))
  window.dispatchEvent(new Event('focus'))
  assert.equal(calls, 1)
  context.mock.timers.tick(0)
  await settleRefresh()
  assert.equal(calls, 1)
  context.mock.timers.tick(5 * 60000)
  window.dispatchEvent(new Event('focus'))
  assert.equal(calls, 1)
})

test('returning later does not repeat a deadline handled in the background', async context => {
  let calls = 0
  schedule('2026-10-25T12:15:00Z', () => calls++)
  document.visibilityState = 'hidden'
  context.mock.timers.tick(15 * 60000 + 1000)
  await settleRefresh()
  assert.equal(calls, 1)
  context.mock.timers.tick(5 * 60000)
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

test('an overdue deadline refreshes once without starting a retry loop', async context => {
  let calls = 0
  schedule('2026-10-25T11:59:00Z', () => calls++)
  context.mock.timers.tick(0)
  await settleRefresh()
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

test('failed refreshes back off up to one minute and stop retrying after success', async context => {
  let calls = 0
  let succeeds = false
  schedule('2026-10-25T11:59:00Z', () => {
    calls++
    return succeeds
  })
  context.mock.timers.tick(0)
  await settleRefresh()
  assert.equal(calls, 1)

  for (const delay of [1000, 2000, 4000, 8000, 16000, 32000, 60000, 60000]) {
    const before = calls
    context.mock.timers.tick(delay - 1)
    window.dispatchEvent(new Event('focus'))
    document.dispatchEvent(new Event('visibilitychange'))
    assert.equal(calls, before)
    context.mock.timers.tick(1)
    await settleRefresh()
    assert.equal(calls, before + 1)
  }

  succeeds = true
  context.mock.timers.tick(60000)
  await settleRefresh()
  const completedCalls = calls
  context.mock.timers.tick(60 * 60000)
  window.dispatchEvent(new Event('focus'))
  assert.equal(calls, completedCalls)
})

test('a pending refresh coalesces return events and retries after rejection', async context => {
  let calls = 0
  let rejectRequest
  schedule('2026-10-25T11:59:00Z', () => {
    calls++
    if (calls === 1) return new Promise((_, reject) => { rejectRequest = reject })
    return Promise.resolve(true)
  })
  context.mock.timers.tick(0)
  context.mock.timers.tick(5 * 60000)
  window.dispatchEvent(new Event('focus'))
  document.dispatchEvent(new Event('visibilitychange'))
  assert.equal(calls, 1)

  rejectRequest(new Error('Network error'))
  await settleRefresh()
  context.mock.timers.tick(1000)
  await settleRefresh()
  assert.equal(calls, 2)
  context.mock.timers.tick(5 * 60000)
  window.dispatchEvent(new Event('focus'))
  assert.equal(calls, 2)
})

test('a synchronous failure is retried without an unhandled rejection', async context => {
  let calls = 0
  schedule('2026-10-25T11:59:00Z', () => {
    if (++calls === 1) throw new Error('Refresh failed')
  })
  context.mock.timers.tick(0)
  await settleRefresh()
  context.mock.timers.tick(1000)
  await settleRefresh()
  assert.equal(calls, 2)
})

test('cleanup cancels a scheduled failure retry', async context => {
  let calls = 0
  const cleanup = schedule('2026-10-25T11:59:00Z', () => {
    calls++
    return false
  })
  context.mock.timers.tick(0)
  await settleRefresh()
  cleanup()
  context.mock.timers.tick(5 * 60000)
  window.dispatchEvent(new Event('focus'))
  assert.equal(calls, 1)
})

test('cleanup during an in-flight refresh prevents retries after it rejects', async context => {
  let calls = 0
  let rejectRequest
  const cleanup = schedule('2026-10-25T11:59:00Z', () => {
    calls++
    return new Promise((_, reject) => { rejectRequest = reject })
  })
  context.mock.timers.tick(0)
  cleanup()
  rejectRequest(new Error('Network error'))
  await settleRefresh()
  context.mock.timers.tick(5 * 60000)
  window.dispatchEvent(new Event('focus'))
  assert.equal(calls, 1)
})
