import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createResourceReload } from '../src/reload.js'

test('reload invalidates the backend before discovering fresh scoped skills and commands', async () => {
  let rows = []
  let cache = []
  let registrations = 0
  const conn = { agent: { session: { header: { cwd: '/project' } } } }
  const skills = {
    registerProvider(create) {
      registrations++
      create({ signal: new AbortController().signal, invalidate: () => { cache = undefined } })
    },
    async snapshot(options) {
      assert.equal(options.cwd, '/project')
      assert.equal(options.scope, conn.agent)
      cache ??= rows
      return { complete: true, skills: cache }
    },
  }
  const reload = createResourceReload({ host: {
    skills: () => ({ ...skills }),
    commands: () => ({ list: () => [] }),
  } })
  rows = [{ name: 'new-skill', description: 'new', invocation: { userInvocable: true } }]
  const frames = await reload(conn, new AbortController().signal)
  assert.deepEqual(frames[0].skills, [{ name: 'new-skill', description: 'new' }])
  rows = []
  assert.deepEqual((await reload(conn, new AbortController().signal))[0].skills, [])
  assert.equal(registrations, 1)
})

test('reload reports unavailable and incomplete discovery rather than success', async () => {
  const signal = new AbortController().signal
  await assert.rejects(createResourceReload({ host: { skills: () => null } })({}, signal), /unavailable/)
  const reload = createResourceReload({ host: { skills: () => ({
    registerProvider: create => create({ signal, invalidate() {} }),
    snapshot: async () => ({ complete: false, skills: [] }),
  }) } })
  await assert.rejects(reload({ agent: {} }, signal), /incomplete/)
})

test('reload reports a registry that withholds provider invalidation', async () => {
  const signal = new AbortController().signal
  const deferred = createResourceReload({ host: { skills: () => ({
    registerProvider: () => {},
    snapshot: async () => ({ complete: true, skills: [] }),
  }) } })
  await assert.rejects(deferred({ agent: {} }, signal), /did not expose provider invalidation/)
  const bare = createResourceReload({ host: { skills: () => ({
    registerProvider: create => create({ signal }),
    snapshot: async () => ({ complete: true, skills: [] }),
  }) } })
  await assert.rejects(bare({ agent: {} }, signal), /did not expose provider invalidation/)
})
