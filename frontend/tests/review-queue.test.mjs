import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { afterEach, beforeEach, test } from 'node:test'
import ts from 'typescript'

// Exercise the real API functions without Vite or additional test dependencies.
function transpile(file) {
  const source = readFileSync(new URL(file, import.meta.url), 'utf8')
  return ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2020 },
  }).outputText
}

function moduleUrl(source) {
  return `data:text/javascript;base64,${Buffer.from(source).toString('base64')}`
}

const webauthnUrl = moduleUrl(transpile('../src/lib/webauthn.ts').replaceAll('import.meta.env', '({})'))
const liveUrl = moduleUrl(transpile('../src/lib/live-events.ts'))
const httpUrl = moduleUrl(transpile('../src/lib/http.ts').replace("'./live-events'", JSON.stringify(liveUrl)))
const api = await import(moduleUrl(
  transpile('../src/lib/api.ts')
    .replace("'./live-events'", JSON.stringify(liveUrl))
    .replace("'./webauthn'", JSON.stringify(webauthnUrl))
    .replace("'./http'", JSON.stringify(httpUrl))
))

let changes
let unsubscribe
let requests
let respond
const originals = new Map(['window', 'localStorage'].map(key => [key, Object.getOwnPropertyDescriptor(globalThis, key)]))

beforeEach(context => {
  Object.defineProperty(globalThis, 'window', {
    configurable: true, value: { location: { origin: 'http://localhost' } },
  })
  Object.defineProperty(globalThis, 'localStorage', {
    configurable: true, value: { getItem: () => { throw new Error('API requests must not read stored JWTs') } },
  })
  changes = []
  requests = []
  respond = () => Response.json({ success: true })
  context.mock.method(globalThis, 'fetch', (url, options) => {
    requests.push({ url, options })
    return Promise.resolve(respond(url, options))
  })
  unsubscribe = api.subscribeReviewQueueChanges(change => changes.push(change))
})

afterEach(() => {
  unsubscribe()
  for (const [key, descriptor] of originals) {
    if (descriptor) Object.defineProperty(globalThis, key, descriptor)
    else delete globalThis[key]
  }
})

test('restoring a card invalidates the queue only after the server succeeds', async () => {
  let complete
  respond = () => new Promise(resolve => { complete = resolve })
  const restoring = api.unsuppressCard(42)
  assert.deepEqual(changes, [])
  complete(Response.json({ success: true }))
  await restoring
  assert.deepEqual(changes, ['availability'])

  respond = () => Response.json({ error: 'Save failed' }, { status: 500 })
  await assert.rejects(api.unsuppressCard(42))
  assert.deepEqual(changes, ['availability'])
})

test('an import announces replaced review state only after a successful import', async () => {
  const file = new File(['{}'], 'reviews.json')
  await api.importUserData(file, true)
  assert.deepEqual(changes, ['import'])

  respond = () => Response.json({ error: 'Invalid import' }, { status: 400 })
  await assert.rejects(api.importUserData(file, true))
  assert.deepEqual(changes, ['import'])
})

test('queue settings invalidate availability, including zero limits and midnight boundaries', async () => {
  for (const settings of [
    { daily_new_card_limit: 20 }, { daily_new_card_limit: 0 },
    { day_boundary_hour: 4 }, { day_boundary_hour: 0 },
    { study_timezone: 'Asia/Seoul' }, { initial_study_timezone: 'Europe/London' },
  ]) {
    await api.updateUserSettings(settings)
  }
  assert.deepEqual(changes, Array(6).fill('availability'))

  respond = () => Response.json({ error: 'Save failed' }, { status: 500 })
  await assert.rejects(api.updateUserSettings({ daily_new_card_limit: 30 }))
  assert.equal(changes.length, 6)
})

test('display settings and reads do not cause queue refreshes', async () => {
  await api.updateUserSettings({ red_threshold: 45, show_hanja_hints: true })
  await api.getStats()
  await api.listSuppressedCards()
  assert.deepEqual(changes, [])
})

test('a queue change during a queue request discards its stale response', async () => {
  let completeOldRead
  let reads = 0
  respond = url => {
    if (!url.includes('/api/cards/queue')) return Response.json({ success: true })
    reads++
    if (reads === 1) return new Promise(resolve => { completeOldRead = resolve })
    return Response.json({ card: { card_id: 42 } })
  }
  const nextCard = api.getReviewQueue()
  await api.unsuppressCard(42)
  completeOldRead(Response.json({ card: null, next_due_at: '2026-11-09T12:00:00Z' }))

  assert.deepEqual(await nextCard, { card: { card_id: 42 } })
  assert.equal(reads, 2)
  assert.ok(requests.filter(request => request.url.includes('/api/cards/queue'))
    .every(request => request.url.endsWith('/api/cards/queue?limit=2')))
})

test('removing a queue subscription stops further notifications', async () => {
  unsubscribe()
  await api.unsuppressCard(42)
  assert.deepEqual(changes, [])
})

test('an own SSE echo does not restart an in-flight queue read', async () => {
  const { registerMutation, dispatchLiveEvent } = await import(liveUrl)
  let complete
  let reads = 0
  respond = () => { reads++; return new Promise(resolve => { complete = resolve }) }
  const next = api.getReviewQueue()
  dispatchLiveEvent({ type: 'card-reviewed', card_id: 1, review_revision: 1, mutation_id: registerMutation() })
  complete(Response.json({ card: { card_id: 2 } }))
  assert.deepEqual(await next, { card: { card_id: 2 } })
  assert.equal(reads, 1)
})

test('batch queue reads retry when overtaken by a queue mutation', async () => {
  let finish
  let reads = 0
  respond = url => {
    if (!url.includes('/api/cards/queue')) return Response.json({ success: true })
    reads++
    if (reads === 1) return new Promise(resolve => { finish = resolve })
    return Response.json({ cards: [{ card_id: 8 }], refresh_at: '2026-11-10T04:00:00Z' })
  }
  const pending = api.getReviewQueue()
  await api.unsuppressCard(8)
  finish(Response.json({ cards: [{ card_id: 1 }] }))
  assert.deepEqual((await pending).cards, [{ card_id: 8 }])
  assert.equal(reads, 2)
  assert.ok(requests.filter(r => r.url.includes('/api/cards/queue')).every(r => r.url.endsWith('/api/cards/queue?limit=2')))
})

const { reconcileQueue } = await import(moduleUrl(transpile('../src/lib/review-queue.ts')))
test('batch reconciliation replaces stale data and preserves legitimate repetitions', () => {
  const card = { card_id: 1, sentence_id: 1, review_revision: 0, content_revision: 0 }
  const other = { ...card, card_id: 2 }
  assert.deepEqual(reconcileQueue([card, other, other], card), [other])
  const repetition = { ...card, review_revision: 1, sentence_id: 2 }
  assert.deepEqual(reconcileQueue([repetition], card), [repetition])
  assert.deepEqual(reconcileQueue([], card), [])
  assert.deepEqual(reconcileQueue([card], null), [card])
})


test('saved study timezone is not changed by the browser timezone', async () => {
  respond = () => Response.json({ study_timezone: 'Asia/Seoul' })
  assert.equal((await api.getUserSettings()).study_timezone, 'Asia/Seoul')
  assert.equal(requests.length, 1)
  assert.deepEqual(changes, [])
})

test('first settings load proposes the browser timezone and uses the saved winner', async () => {
  let reads = 0
  respond = (_url, options) => {
    if (options.method === 'PATCH') {
      assert.deepEqual(JSON.parse(options.body), {
        initial_study_timezone: Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC',
      })
      return Response.json({ success: true })
    }
    return Response.json({ study_timezone: reads++ === 0 ? null : 'Asia/Seoul' })
  }
  assert.equal((await api.getUserSettings()).study_timezone, 'Asia/Seoul')
  assert.equal(requests.length, 3)
  assert.deepEqual(changes, ['availability'])
})
