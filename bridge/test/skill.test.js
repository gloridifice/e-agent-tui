// /skill layer pure-function contracts (node --test test/skill.test.js).
import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  isSkillName,
  parseSkillCommand,
  renderSkillContent,
  shapeSkillsFrame,
  skillInvocationSource,
  watchSkillChanges,
} from '../src/skill.js'

test('isSkillName accepts kebab-case skill names only', () => {
  assert.equal(isSkillName('deepseek-e'), true)
  assert.equal(isSkillName('cordis-plugin-development'), true)
  assert.equal(isSkillName('a1-b2'), true)
  assert.equal(isSkillName('BadName'), false)
  assert.equal(isSkillName('has space'), false)
  assert.equal(isSkillName(''), false)
  assert.equal(isSkillName(undefined), false)
})

test('skill roster exposes only valid user-invocable summaries', () => {
  assert.deepEqual(shapeSkillsFrame([
    { name: 'zeta-skill', description: 'Z', invocation: { userInvocable: true } },
    { name: 'alpha-skill', description: 'A', invocation: { userInvocable: true } },
    { name: 'model-only', description: 'hidden', invocation: { userInvocable: false } },
    { name: 'Bad Name', description: 'invalid', invocation: { userInvocable: true } },
    { name: 'missing-description', invocation: { userInvocable: true } },
  ]), {
    type: 'skills',
    skills: [
      { name: 'alpha-skill', description: 'A' },
      { name: 'zeta-skill', description: 'Z' },
    ],
  })
})

test('skills/change refreshes every cwd/scope-sensitive connection', () => {
  let listener
  let disposed = false
  const ctx = {
    on: (name, callback) => {
      assert.equal(name, 'skills/change')
      listener = callback
      return () => { disposed = true }
    },
  }
  const connections = [{ agent: { id: 'a' } }, { agent: { id: 'b' } }]
  const refreshed = []
  const off = watchSkillChanges(ctx, connections, (connection) => refreshed.push(connection.agent.id))
  listener()
  assert.deepEqual(refreshed, ['a', 'b'])
  off()
  assert.equal(disposed, true)
})

test('parseSkillCommand accepts colon and space forms, lowercased', () => {
  assert.deepEqual(parseSkillCommand('/skill:cordis-plugin-development'), { name: 'cordis-plugin-development', prompt: '' })
  assert.deepEqual(parseSkillCommand('/skill:DeepSeek-Dev'), { name: 'deepseek-dev', prompt: '' })
  assert.deepEqual(parseSkillCommand('/skill foo-bar'), { name: 'foo-bar', prompt: '' })
  assert.deepEqual(parseSkillCommand('/skill   foo  '), { name: 'foo', prompt: '' })
  assert.equal(parseSkillCommand('/skill'), undefined)
  assert.equal(parseSkillCommand('/skill:'), undefined)
  assert.deepEqual(parseSkillCommand('/skill foo bar'), { name: 'foo', prompt: 'bar' })
  assert.equal(parseSkillCommand('/skills:foo'), undefined)
  assert.equal(parseSkillCommand('/new'), undefined)
  assert.equal(parseSkillCommand('hello /skill:x'), undefined)
})

test('skill parser retains the complete trailing prompt after separator whitespace', () => {
  for (const prefix of ['/skill:review', '/skill review']) {
    assert.deepEqual(parseSkillCommand(`${prefix} \t\n 检查  code\n  next line  `), {
      name: 'review', prompt: '检查  code\n  next line  ',
    })
    assert.deepEqual(parseSkillCommand(`${prefix}\t\n `), { name: 'review', prompt: '' })
  }
  assert.equal(parseSkillCommand('/skill:review! text'), undefined)
})

test('renderSkillContent wraps instructions and escapes the name', () => {
  const skill = {
    name: 'foo"&<',
    provider: 'filesystem',
    resourceBase: { kind: 'directory', path: 'D:\\ws\\<x>' },
    content: 'do <the> & thing',
  }
  const rendered = renderSkillContent(skill)
  assert.ok(rendered.startsWith('<skill_content name="foo&quot;&amp;&lt;">'))
  assert.ok(rendered.includes('<skill_instructions>\ndo <the> & thing\n</skill_instructions>'))
  assert.ok(rendered.includes('Base directory for this skill: D:\\ws\\&lt;x&gt;'))
  assert.ok(rendered.endsWith('</skill_content>'))
})

test('renderSkillContent without a resource base falls back to a provider hint', () => {
  const rendered = renderSkillContent({ name: 'x', provider: 'runtime', content: 'c' })
  assert.ok(rendered.includes('managed by provider "runtime"'))
})

test('skillInvocationSource matches dsh-tool-skill', () => {
  assert.deepEqual(skillInvocationSource('foo-bar'), {
    kind: 'skill-invocation',
    name: 'foo-bar',
    form: 'instructions',
  })
})
