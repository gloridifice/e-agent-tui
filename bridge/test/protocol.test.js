import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import {
  HISTORY_CAP,
  MAX_FRAME_BYTES,
  PROTOCOL_VERSION,
  SNAPSHOT_CAP,
  SNAPSHOT_SURFACE,
  WIRE_CONTRACT,
  supportsClientMessage,
} from '../src/protocol.js'

test('canonical contract owns protocol capacities and message roster', () => {
  assert.equal(PROTOCOL_VERSION, 10)
  assert.equal(SNAPSHOT_CAP, 600)
  assert.equal(HISTORY_CAP, 2000)
  assert.equal(MAX_FRAME_BYTES, 16 * 1024 * 1024)
  assert.ok(SNAPSHOT_SURFACE.has('assistant/message'))
  for (const type of ['command/run', 'compaction/summary', 'llm/retry', 'tool/code-dispatch', 'tool-workflow/run-end', 'todo/write']) {
    assert.ok(SNAPSHOT_SURFACE.has(type), `history reconstructs ${type}`)
  }
  assert.equal(SNAPSHOT_SURFACE.has('approval/asked'), false, 'audit-only events stay out')
  assert.ok(supportsClientMessage('answer-questions'))
  assert.ok(supportsClientMessage('new-input'))
  assert.ok(supportsClientMessage('clear-asap'))
  assert.ok(WIRE_CONTRACT.serverMessages.includes('asap-queue'))
  assert.equal(supportsClientMessage('made-up-message'), false)
  assert.equal(new Set(WIRE_CONTRACT.clientMessages).size, WIRE_CONTRACT.clientMessages.length)
  assert.equal(new Set(WIRE_CONTRACT.serverMessages).size, WIRE_CONTRACT.serverMessages.length)
  const packageJson = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'))
  assert.equal(
    packageJson.dshCompatibility?.wireProtocol,
    PROTOCOL_VERSION,
    'package compatibility metadata follows the canonical wire contract',
  )
  const docs = readFileSync(new URL('../../doco/protocol.md', import.meta.url), 'utf8')
  assert.match(docs, /Protocol version \| 10/)
  assert.match(docs, /`commands`/)
  assert.match(docs, /`skills`/)
  assert.match(docs, /`command-result`/)
  assert.match(docs, /Snapshot surface events \| 600/)
})
