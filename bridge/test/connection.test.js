import { test } from 'node:test'
import assert from 'node:assert/strict'
import { ConnectionRegistry } from '../src/connection.js'

function connection() {
  const calls = []
  const conn = {
    agent: { id: 'a1' },
    ws: { readyState: 1, close: (code) => calls.push(['close', code]) },
    off: () => calls.push(['off']),
    abort: { abort: () => calls.push(['abort']) },
    pending: new Map([['p1', (outcome) => calls.push(['pending', outcome])]]),
  }
  return { conn, calls }
}

test('detach owns listener, abort, approval, and socket cleanup', () => {
  const registry = new ConnectionRegistry()
  const { conn, calls } = connection()
  registry.add(conn)
  assert.equal(registry.detach(conn), true)
  assert.deepEqual(calls, [['off'], ['abort'], ['pending', 'cancelled'], ['close', 1000]])
  assert.equal(conn.pending.size, 0)
  assert.equal(registry.detach(conn), false, 'cleanup is idempotent')
})

test('keepSocket cleanup and stale-operation guard support reattach', () => {
  const registry = new ConnectionRegistry()
  const first = connection()
  const second = connection()
  second.conn.agent.id = 'a2'
  registry.add(first.conn)
  assert.equal(registry.isCurrent(first.conn, first.conn), true)
  registry.detach(first.conn, { keepSocket: true })
  registry.add(second.conn)
  assert.equal(registry.isCurrent(first.conn, second.conn), false)
  assert.equal(first.calls.some(([name]) => name === 'close'), false)
  assert.equal(registry.findAgent('a2'), second.conn)
})
