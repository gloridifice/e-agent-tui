import { test } from 'node:test'
import assert from 'node:assert/strict'
import { installQuestionRelay, relayQuestionFrames } from '../src/question.js'

async function* frames(items) {
  yield* items
}

test('question relay waits for apiProxy then forwards requested and resolved frames', async () => {
  let activate
  const ctx = {
    inject(services, callback) {
      assert.deepEqual(services, ['apiProxy'])
      activate = callback
      return Symbol('fiber')
    },
  }
  const sent = []
  const conn = { ws: {} }
  const conns = { findAgent: (id) => id === 'session-1' ? conn : undefined }
  const questionSessions = new Map()
  let opened = false
  let dispose
  const result = installQuestionRelay(ctx, {
    conns,
    send: (ws, frame) => sent.push({ ws, frame }),
    questionSessions,
    openMux: (_apiProxy, signal) => {
      opened = true
      assert.equal(signal.aborted, false)
      return frames([
        {
          rpcId: 'rpc-1',
          payload: {
            type: 'question/requested',
            sessionId: 'session-1',
            questions: [{ id: 'choice', question: 'A or B?' }],
          },
        },
        {
          rpcId: 'push-1',
          payload: {
            type: 'question/resolved',
            sessionId: 'session-1',
            questionRpcId: 'rpc-1',
            outcome: 'answered',
          },
        },
      ])
    },
  })

  assert.equal(typeof activate, 'function')
  assert.equal(opened, false, 'the mux must not open before apiProxy is published')
  assert.equal(typeof result, 'symbol')

  activate({
    apiProxy: {},
    effect(run, label) {
      assert.equal(label, 'dsh-tui: question event relay')
      dispose = run()
    },
  })
  await new Promise((resolve) => setImmediate(resolve))

  assert.equal(opened, true)
  assert.deepEqual(sent.map(({ frame }) => frame), [
    {
      type: 'question',
      rpcId: 'rpc-1',
      sessionId: 'session-1',
      questions: [{ id: 'choice', question: 'A or B?' }],
    },
    {
      type: 'question-resolved',
      questionRpcId: 'rpc-1',
      outcome: 'answered',
    },
  ])
  assert.equal(sent.every(({ ws }) => ws === conn.ws), true)
  assert.equal(questionSessions.has('rpc-1'), false)
  assert.equal(typeof dispose, 'function')
  dispose()
})

test('question relay evicts the oldest pending rpcId instead of clearing all', async () => {
  const sent = []
  const conn = { ws: {} }
  const conns = { findAgent: () => conn }
  const questionSessions = new Map()
  for (let i = 0; i < 64; i += 1) {
    questionSessions.set(`old-${i}`, 'session-1')
  }
  await relayQuestionFrames({
    apiProxy: {},
    signal: new AbortController().signal,
    conns,
    send: (ws, frame) => sent.push(frame),
    questionSessions,
    openMux: () => frames([
      {
        rpcId: 'new-rpc',
        payload: {
          type: 'question/requested',
          sessionId: 'session-1',
          questions: [{ id: 'choice', question: 'A or B?' }],
        },
      },
    ]),
  })

  assert.equal(questionSessions.size, 64)
  assert.equal(questionSessions.has('new-rpc'), true)
  assert.equal(questionSessions.has('old-0'), false, 'oldest entry is evicted')
  assert.equal(questionSessions.has('old-1'), true)
  assert.equal(sent.length, 1)
})
