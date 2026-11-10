import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import ts from 'typescript'

const source = readFileSync(new URL('../src/lib/card-edit-draft.ts', import.meta.url), 'utf8')
const { outputText } = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
})
const { cardEditPatch, toFormState } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`)
const card = {
  sentence_id: 3, content_revision: 7, word: 'word', hanja: '漢字',
  trans_word: 'translation', alternatives: ['one', 'two'], is_honorific: true,
}
const precondition = { sentence_id: 3, expected_content_revision: 7 }

test('unchanged and reverted form fields are omitted', () => {
  assert.deepEqual(cardEditPatch(card, toFormState(card)), precondition)
  const form = toFormState(card)
  form.word = 'temporary'
  form.word = card.word
  assert.deepEqual(cardEditPatch(card, form), precondition)
})

test('only edited fields are sent, including explicit clears and false', () => {
  assert.deepEqual(cardEditPatch(card, { ...toFormState(card), trans_word: ' new translation ' }), {
    ...precondition, trans_word: 'new translation',
  })
  assert.deepEqual(cardEditPatch(card, {
    ...toFormState(card), hanja: '', alternatives: '', is_honorific: false,
  }), { ...precondition, hanja: null, alternatives: [], is_honorific: false })
})
