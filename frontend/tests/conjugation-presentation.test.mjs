import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import ts from 'typescript'
const source = readFileSync(new URL('../src/lib/conjugation-presentation.ts',import.meta.url),'utf8')
const output = ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText
const { groupConjugations } = await import(`data:text/javascript;base64,${Buffer.from(output).toString('base64')}`)
const form = (slug, value) => ({form_slug:slug,form:value,ending:'-example',tense:null,speech_level:null})
test('client grouping and order do not depend on backend order or categories', () => {
  const groups = groupConjugations([form('connective_but','먹지만'),form('past_haeyo','먹었어요'),form('present_hasipsio','먹습니다'),form('present_haeyo','먹어요')])
  assert.deepEqual(groups.map(g=>g.label),['Present','Past','Connectives'])
  assert.deepEqual(groups[0].rows.map(r=>r.form_slug),['present_haeyo','present_hasipsio'])
  assert.deepEqual(groups[0].rows.map(r=>r.form),['먹어요','먹습니다'])
})
test('unknown form definitions remain visible rather than disappearing', () => {
  const groups = groupConjugations([form('new_backend_form','새 형태')])
  assert.equal(groups[0].label,'Other forms')
  assert.equal(groups[0].rows[0].form,'새 형태')
  assert.equal(groups[0].rows[0].ending,'-example')
  assert.deepEqual(groupConjugations([]),[])
})
