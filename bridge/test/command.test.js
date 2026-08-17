import { test } from 'node:test'
import assert from 'node:assert/strict'
import { shapeCommandResultFrame, shapeCommandsFrame, watchCommandChanges } from '../src/command.js'

test('command directory projects only DSH handler-free metadata', () => {
  const frame = shapeCommandsFrame([
    { name: 'plan', description: 'Plan mode', input: { hint: '[off|message]' }, handler: () => {} },
    { name: 'feedback', description: 'Record feedback', input: { hint: '<text>' } },
    { name: 'Bad Name', description: 'invalid' },
  ])
  assert.deepEqual(frame, {
    type: 'commands',
    commands: [
      { name: 'feedback', description: 'Record feedback', input: { hint: '<text>' } },
      { name: 'plan', description: 'Plan mode', input: { hint: '[off|message]' } },
    ],
  })
  assert.equal('handler' in frame.commands[1], false)
})

test('commands/change refreshes every agent-scoped connection', () => {
  let listener
  let disposed = false
  const ctx = {
    on: (name, callback) => {
      assert.equal(name, 'commands/change')
      listener = callback
      return () => { disposed = true }
    },
  }
  const connections = [{ agent: { id: 'a' } }, { agent: { id: 'b' } }]
  const refreshed = []
  const off = watchCommandChanges(ctx, connections, (connection) => refreshed.push(connection.agent.id))
  listener()
  assert.deepEqual(refreshed, ['a', 'b'])
  off()
  assert.equal(disposed, true)
})

test('command execution outcome becomes a generic direct-result frame', () => {
  assert.deepEqual(shapeCommandResultFrame({
    commandId: 'cmd-a-1',
    result: { kind: 'error', text: 'bad input' },
  }), {
    type: 'command-result', commandId: 'cmd-a-1', kind: 'error', text: 'bad input',
  })
  assert.equal(shapeCommandResultFrame(undefined), undefined)
})
