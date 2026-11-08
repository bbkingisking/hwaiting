import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import ts from 'typescript'
async function module(name) {
  const source = readFileSync(new URL(`../src/lib/${name}.ts`, import.meta.url), 'utf8')
  const { outputText } = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2020 } })
  return import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`)
}
const { createRefreshTask } = await module('refresh-task')
const { registerMutation, subscribeLiveEvents, dispatchLiveEvent, connectLiveEvents } = await module('live-events')
const settle = async () => { for (let i = 0; i < 8; i++) await Promise.resolve() }

test('own events are recognised before the HTTP response and external operations remain external', () => {
  const received = []
  const unsubscribe = subscribeLiveEvents(event => received.push(event))
  const id = registerMutation()
  assert.match(id, /^[a-f0-9]{8}-[a-f0-9]{4}-4[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/)
  dispatchLiveEvent({ type: 'card-reviewed', card_id: 1, review_revision: 1, mutation_id: id })
  dispatchLiveEvent({ type: 'card-reviewed', card_id: 1, review_revision: 2, mutation_id: 'another-operation' })
  assert.deepEqual(received.map(event => event.own), [true, false])
  unsubscribe()
})

test('changes during reads discard old results and coalesce the follow-up', async () => {
  const reads = []
  const results = []
  const task = createRefreshTask(async current => {
    const result = await new Promise(resolve => reads.push(resolve))
    if (current()) results.push(result)
  })
  task.request()
  task.request()
  task.request()
  assert.equal(reads.length, 1)
  reads[0]('old')
  await settle()
  assert.deepEqual(results, [])
  assert.equal(reads.length, 2)
  reads[1]('new')
  await settle()
  assert.deepEqual(results, ['new'])
  task.dispose()
})

test('failed refresh retries without another event; disposal cancels retries', async context => {
  context.mock.timers.enable({ apis: ['setTimeout'] })
  let calls = 0
  const task = createRefreshTask(async () => { calls++; throw new Error('offline') })
  task.request()
  await settle()
  context.mock.timers.tick(1000)
  await settle()
  assert.equal(calls, 2)
  task.dispose()
  context.mock.timers.tick(60000)
  await settle()
  assert.equal(calls, 2)
  context.mock.timers.reset()
})

test('SSE connection carries cookies, survives network errors, and cleans up on logout', async () => {
  const sources = []
  class FakeSource extends EventTarget {
    static OPEN = 1
    static CLOSED = 2
    readyState = 1
    constructor(url, options) { super(); this.url = url; this.options = options; sources.push(this) }
    close() { this.readyState = 2 }
  }
  globalThis.EventSource = FakeSource
  globalThis.document = Object.assign(new EventTarget(), { visibilityState: 'visible' })
  globalThis.window = new EventTarget()
  const stop = connectLiveEvents('/api/events', async () => { throw new Error('offline') })
  assert.equal(sources[0].options.withCredentials, true)
  sources[0].onerror()
  await settle()
  assert.equal(sources[0].readyState, FakeSource.OPEN)
  document.dispatchEvent(new Event('visibilitychange'))
  assert.equal(sources.length, 1, 'returning with a live stream needs no fresh subscription/read')
  stop()
  assert.equal(sources[0].readyState, FakeSource.CLOSED)
  window.dispatchEvent(new Event('online'))
  assert.equal(sources.length, 1)
  delete globalThis.EventSource
  delete globalThis.document
  delete globalThis.window
})

const { canKeepSuccessorAfterReview } = await module('review-events')
test('a remote review preserves another due review but invalidates new or reviewed successors', () => {
  assert.equal(canKeepSuccessorAfterReview({ card_id: 2, difficulty: 3 }, 1), true)
  assert.equal(canKeepSuccessorAfterReview({ card_id: 2, difficulty: null }, 1), false)
  assert.equal(canKeepSuccessorAfterReview({ card_id: 2, difficulty: 3 }, 2), false)
  assert.equal(canKeepSuccessorAfterReview(undefined, 1), false)
})
