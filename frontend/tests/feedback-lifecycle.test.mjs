import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import ts from 'typescript'

// Exercise App's actual handlers with controlled refs and deferred state reads.
const source = readFileSync(new URL('../src/App.tsx', import.meta.url), 'utf8')
const file = ts.createSourceFile('App.tsx', source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX)
const app = file.statements.find(node => ts.isFunctionDeclaration(node) && node.name.text === 'AppContent')
const names = ['invalidateCardContent', 'handleLiveEvent']
const declarations = app.body.statements.filter(node => ts.isVariableStatement(node) &&
  node.declarationList.declarations.some(declaration => names.includes(declaration.name.getText(file))))
const validation = app.body.statements.find(node => ts.isExpressionStatement(node) &&
  node.getText(file).includes('validationTask.current = task'))
const code = ts.transpileModule([
  ...declarations.map(node => node.getText(file)), validation.getText(file),
  'return { invalidateCardContent, handleLiveEvent };',
].join('\n'), { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None } }).outputText

function harness() {
  const calls = []
  const active = { card_id: 1, sentence_id: 1, review_revision: 7, content_revision: 2 }
  let validate
  let readState = async () => ({ exists: true, suppressed: false, review_revision: 8, content_revision: 3 })
  const context = {
    isAuthenticated: true,
    currentCard: { current: active }, acceptedReview: { current: null },
    checkingRef: { current: false }, submissionVersion: { current: 0 },
    allowCurrentCandidate: { current: false }, validationTask: { current: null },
    refillTask: { current: { request: () => calls.push('refill') } }, prefetchRef: { current: null },
    setStatsKey: () => calls.push('stats'), cancelPrefetch: () => calls.push('cancel-prefetch'),
    setChecking: () => calls.push('checking'), setSubmitError: () => calls.push('submit-error'),
    setStaleReason: () => calls.push('stale'), discardReviewedCard: () => calls.push('discard'),
    loadCardCold: async () => { calls.push('cold-load'); return true },
    getCardState: () => readState(), useEffectEvent: fn => fn, useEffect: fn => fn(),
    createRefreshTask: fn => {
      validate = () => fn(() => true)
      return { request() {}, dispose() {} }
    },
    subscribeLiveEvents: () => () => {},
  }
  const handlers = new Function(...Object.keys(context), code)(...Object.values(context))
  return { ...handlers, context, calls, active, validate: () => validate(), setRead: fn => { readState = fn } }
}

for (const phase of ['accepted', 'pending']) {
  test(`${phase} attempts survive content, review, suppression and resync events`, async () => {
    const h = harness()
    if (phase === 'accepted') h.context.acceptedReview.current = h.active
    else h.context.checkingRef.current = true
    for (const event of [
      { type: 'card-updated', card_id: 1 },
      { type: 'card-reviewed', card_id: 1, review_revision: 9 },
      { type: 'queue-changed', card_id: 1, suppressed: true },
      { type: 'resync-required' }, { type: 'state-reset' },
    ]) h.handleLiveEvent(event)
    h.invalidateCardContent(1, true)
    assert.ok(h.calls.includes('refill'))
    assert.ok(h.calls.includes('stats'))
    assert.ok(!h.calls.some(call => ['discard', 'stale', 'cold-load', 'checking', 'submit-error'].includes(call)))
    assert.equal(h.context.submissionVersion.current, 0)
    if (phase === 'accepted') await h.validate()
    else await assert.rejects(h.validate(), /pending review/)
  })
}

test('a state read started before submission cannot dismiss accepted feedback', async () => {
  const h = harness()
  let finish
  h.setRead(() => new Promise(resolve => { finish = resolve }))
  const pending = h.validate()
  h.context.submissionVersion.current++
  h.context.acceptedReview.current = h.active
  finish({ exists: true, suppressed: false, review_revision: 9, content_revision: 3 })
  await pending
  assert.deepEqual(h.calls, [])
})

test('unanswered prompts still react to remote reviews and content edits', () => {
  const review = harness()
  review.handleLiveEvent({ type: 'card-reviewed', card_id: 1, review_revision: 8 })
  assert.ok(review.calls.includes('discard'))
  const edit = harness()
  edit.handleLiveEvent({ type: 'card-updated', card_id: 1 })
  assert.ok(edit.calls.includes('stale'))
})
