import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import ts from 'typescript'
const url = source => `data:text/javascript;base64,${Buffer.from(source).toString('base64')}`
const transpile = name => ts.transpileModule(readFileSync(new URL(`../src/lib/${name}.ts`, import.meta.url), 'utf8'), { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 } }).outputText
const draftModule = url(transpile('card-edit-draft'))
const { adminCardViews, initialDrafts, updateDrafts, draftChanges } = await import(url(transpile('admin-card-draft').replace("'./card-edit-draft'", JSON.stringify(draftModule))))
const target = { form: '공부해요', alternatives: [], tense: 'present', speech_level: 'haeyo-che', is_honorific: false, is_humble: false, grammar_pattern: null }
const detail = {
  card_id: 1, content_revision: 4, word: '공부하다', hanja: '工夫',
  translations: { eng: { word: 'study' }, fra: { word: 'étudier' } },
  sentence: { sentence_id: 10, text: '한국어를 공부해요.', target, translations: { eng: 'I study Korean.', fra: "J’étudie le coréen." } },
}
test('the single sentence exposes all its translation views', () => {
  const views = adminCardViews(detail)
  assert.deepEqual(Object.keys(views), ['10:eng', '10:fra'])
  assert.deepEqual(draftChanges(views, initialDrafts(views)), [])
})
test('drafts retain separate translations while sharing card and sentence facts', () => {
  const views = adminCardViews(detail)
  let drafts = initialDrafts(views)
  drafts = updateDrafts(drafts, '10:eng', 'sentence_translation', 'Edited English.')
  drafts = updateDrafts(drafts, '10:fra', 'trans_word', 'apprendre')
  drafts = updateDrafts(drafts, '10:eng', 'hanja', '')
  drafts = updateDrafts(drafts, '10:fra', 'is_honorific', true)
  assert.equal(drafts['10:eng'].sentence_translation, 'Edited English.')
  assert.equal(drafts['10:fra'].sentence_translation, "J’étudie le coréen.")
  assert.equal(drafts['10:eng'].is_honorific, true)
  assert.ok(Object.values(drafts).every(d => d.hanja === ''))
  const changes = draftChanges(views, drafts)
  assert.equal(changes.length, 2)
  assert.ok(changes.every(c => !('expected_content_revision' in c)))
})

test('shared edits are sent once rather than repeated for each language', () => {
  const views = adminCardViews(detail)
  const drafts = updateDrafts(initialDrafts(views), '10:eng', 'hanja', '')
  const changes = draftChanges(views, drafts)
  assert.equal(changes.length, 1)
  assert.equal(changes[0].hanja, null)
})
