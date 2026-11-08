import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import ts from 'typescript'

const source = readFileSync(new URL('../src/lib/http.ts', import.meta.url), 'utf8')
const { outputText } = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2020 },
})
const { fetchWithCookies } = await import(
  `data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`
)

test('reads send cookies without custom headers, enabling native cookie-based SSE', async context => {
  const requests = []
  context.mock.method(globalThis, 'fetch', (url, options) => {
    requests.push(options)
    return Promise.resolve(new Response())
  })
  await fetchWithCookies('http://localhost/api/user/me')
  assert.equal(requests[0].credentials, 'include')
  assert.equal(requests[0].headers.has('Authorization'), false)
  assert.equal(requests[0].headers.has('X-Requested-With'), false)
})

test('every write sends cookies and the CSRF header while preserving request data', async context => {
  const requests = []
  context.mock.method(globalThis, 'fetch', (url, options) => {
    requests.push(options)
    return Promise.resolve(new Response())
  })
  const signal = new AbortController().signal
  for (const method of ['POST', 'PUT', 'PATCH', 'DELETE', 'post']) {
    await fetchWithCookies('http://localhost/api/auth/login', {
      method, body: '{}', headers: { 'Content-Type': 'application/json' }, signal,
    })
  }
  for (const request of requests) {
    assert.equal(request.credentials, 'include')
    assert.equal(request.headers.get('X-Requested-With'), 'hwaiting')
    assert.equal(request.headers.get('Content-Type'), 'application/json')
    assert.equal(request.headers.has('Authorization'), false)
    assert.equal(request.body, '{}')
    assert.equal(request.signal, signal)
  }
})
