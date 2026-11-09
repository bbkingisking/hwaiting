import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import ts from 'typescript'

const source = readFileSync(new URL('../src/lib/settings-draft.ts', import.meta.url), 'utf8')
const { outputText } = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
})
const { changedSettings } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`)

test('Save sends only changed fields and ignores the FSRS action flag', () => {
  const base = { desiredRetention: 0.9, dailyNewCardLimit: 20, debugStatusBar: false, hasFsrsParameters: false }
  assert.deepEqual(changedSettings(base, base), {})
  assert.deepEqual(changedSettings(base, { ...base, desiredRetention: 0.85, dailyNewCardLimit: 0, hasFsrsParameters: true }), {
    desiredRetention: 0.85,
    dailyNewCardLimit: 0,
  })
  assert.deepEqual(changedSettings(base, { ...base, desiredRetention: 0.9 }), {})
})
