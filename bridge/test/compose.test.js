// Pure session-composition helper contracts.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import {
  isExistingDirectory,
  latestTitle,
  sessionPresetOf,
} from '../src/compose.js'

test('latestTitle returns the newest session/title', () => {
  const events = [
    { type: 'user/message' },
    { type: 'session/title', data: { title: '第一个' } },
    { type: 'session/title', data: { title: '第二个' } },
  ]
  assert.equal(latestTitle(events), '第二个')
  assert.equal(latestTitle([]), undefined)
  assert.equal(latestTitle(undefined), undefined)
})

test('sessionPresetOf prefers agent-preset/selected over the header', () => {
  const events = [{ type: 'agent-preset/selected', data: { agentPreset: 'minimal' } }]
  assert.equal(sessionPresetOf({ agentPreset: 'standard' }, events), 'minimal')
  assert.equal(sessionPresetOf({ agentPreset: 'cordis' }, []), 'cordis')
  assert.equal(sessionPresetOf({}, []), undefined)
})

test('isExistingDirectory accepts real dirs only', () => {
  const dir = mkdtempSync(join(tmpdir(), 'dsh-tui-compose-'))
  assert.equal(isExistingDirectory(dir), true)
  assert.equal(isExistingDirectory(join(dir, 'missing')), false)
  assert.equal(isExistingDirectory(''), false)
  assert.equal(isExistingDirectory(undefined), false)
})
