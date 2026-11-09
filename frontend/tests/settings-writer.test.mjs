import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import ts from 'typescript'

const source = readFileSync(new URL('../src/lib/settings-writer.ts', import.meta.url), 'utf8')
const { outputText } = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
})
const { SettingsWriter } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`)
const initial = { redThreshold: 50, yellowThreshold: 70, dailyNewCardLimit: 20, debugStatusBar: false, hasFsrsParameters: false }
const settle = async () => { for (let i = 0; i < 8; i++) await Promise.resolve() }
const deferred = () => {
  let resolve, reject
  const promise = new Promise((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}

test('rapid edits are serialized and the last value is confirmed', async () => {
  const requests = []
  const completions = []
  const writer = new SettingsWriter(initial, patch => {
    const done = deferred()
    requests.push(patch)
    completions.push(done)
    return done.promise
  }, () => {}, () => {})
  writer.update({ dailyNewCardLimit: 21 })
  writer.update({ dailyNewCardLimit: 22 })
  writer.update({ dailyNewCardLimit: 23, redThreshold: 55 })
  assert.deepEqual(requests, [{ dailyNewCardLimit: 21 }])
  assert.equal(writer.snapshot().status, 'saving')
  assert.equal(writer.snapshot().draft.dailyNewCardLimit, 23)
  assert.equal(writer.snapshot().confirmed.dailyNewCardLimit, 20)

  completions[0].resolve()
  await settle()
  assert.deepEqual(requests, [
    { dailyNewCardLimit: 21 },
    { dailyNewCardLimit: 23, redThreshold: 55 },
  ])
  assert.equal(writer.snapshot().confirmed.dailyNewCardLimit, 21)
  const staleReadVersion = writer.getVersion()
  completions[1].resolve()
  await settle()
  assert.equal(writer.hydrate({ ...initial, dailyNewCardLimit: 21 }, staleReadVersion), false)
  assert.equal(writer.snapshot().status, 'saved')
  assert.equal(writer.snapshot().confirmed.dailyNewCardLimit, 23)
  assert.equal(writer.snapshot().confirmed.redThreshold, 55)
})

test('a failed write remains visibly unsaved and retry repeats the latest intent', async () => {
  const requests = []
  const first = deferred()
  const writer = new SettingsWriter(initial, patch => {
    requests.push(patch)
    return requests.length === 1 ? first.promise : Promise.resolve()
  }, () => {}, () => {})
  writer.update({ dailyNewCardLimit: 21 })
  writer.update({ dailyNewCardLimit: 23 })
  first.reject(new Error('Network unavailable'))
  await settle()
  assert.equal(writer.snapshot().status, 'failed')
  assert.equal(writer.snapshot().error, 'Network unavailable')
  assert.equal(writer.snapshot().draft.dailyNewCardLimit, 23)
  assert.equal(writer.snapshot().confirmed.dailyNewCardLimit, 20)
  writer.retry()
  await settle()
  assert.deepEqual(requests[1], { dailyNewCardLimit: 23 })
  assert.equal(writer.snapshot().status, 'saved')
  assert.equal(writer.snapshot().confirmed.dailyNewCardLimit, 23)
})

test('stale reads cannot overwrite a draft and discard reads back uncertain writes', async () => {
  const first = deferred()
  let reads = 0
  const writer = new SettingsWriter(initial, () => first.promise, () => {}, () => { reads++ })
  const readVersion = writer.getVersion()
  writer.update({ redThreshold: 60 })
  assert.equal(writer.hydrate({ ...initial, redThreshold: 45 }, readVersion), false)
  first.reject(new Error('Response lost'))
  await settle()
  assert.equal(writer.snapshot().status, 'failed')
  writer.discard()
  assert.equal(writer.snapshot().draft.redThreshold, 50)
  assert.equal(reads, 1)
  assert.equal(writer.hydrate({ ...initial, redThreshold: 60 }, writer.getVersion()), true)
  assert.equal(writer.snapshot().confirmed.redThreshold, 60)
})
