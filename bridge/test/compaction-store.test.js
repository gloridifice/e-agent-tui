import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { createCompactionStore } from '../src/compaction-store.js'
import { createCompactionModels } from '../src/compaction.js'

test('global compaction route survives reconstruction, is shared across sessions and clears globally', async t => {
  const dir = mkdtempSync(join(tmpdir(), 'e-compaction-'))
  t.after(() => rmSync(dir, { recursive: true, force: true }))
  const path = join(dir, 'compaction-model.json')
  const store = createCompactionStore(path)
  assert.equal(store.load(), null)
  const listeners = new Map()
  const agent = { session: {} }
  const make = () => createCompactionModels({
    ctx: { on: (event, handler) => listeners.set(event, handler) },
    host: { agents: () => ({ get: () => agent }) },
    sessionModel: { catalogModels: async () => ({ groups: [{ id: 'p', models: [{ id: 'small' }] }] }) },
    store: createCompactionStore(path),
  })
  await make().configure(agent, 'set-model p/small', () => true)
  const next = make()
  const options = { purpose: 'compaction', sessionId: 'another-project', model: 'large' }
  listeners.get('llm/stream')(options, () => {})
  assert.equal(options.model, 'small')
  await next.configure(agent, 'unset-model', () => true)
  assert.equal(store.load(), null)
  writeFileSync(path, '{invalid')
  assert.throws(() => store.load())
  writeFileSync(path, '{"version":2,"provider":"p","model":"small"}')
  assert.throws(() => store.load(), /Invalid/)
})
