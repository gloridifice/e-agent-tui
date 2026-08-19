import { openApiProxyMux } from './host.js'

/** Relay one active API-proxy mux into the TUI connection for each session. */
export async function relayQuestionFrames({
  apiProxy,
  signal,
  conns,
  send,
  questionSessions,
  openMux = openApiProxyMux,
}) {
  const frames = openMux(apiProxy, signal)
  if (!frames) return
  for await (const frame of frames) {
    const payload = frame?.payload
    if (!payload || typeof payload !== 'object') continue
    if (payload.type === 'question/requested') {
      const conn = conns.findAgent(payload.sessionId)
      if (!conn) continue
      if (questionSessions.size >= 64) {
        const oldest = questionSessions.keys().next().value
        if (oldest !== undefined) questionSessions.delete(oldest)
      }
      questionSessions.set(frame.rpcId, payload.sessionId)
      send(conn.ws, {
        type: 'question',
        rpcId: frame.rpcId,
        sessionId: payload.sessionId,
        questions: payload.questions,
      })
    } else if (payload.type === 'question/resolved') {
      questionSessions.delete(payload.questionRpcId)
      const conn = conns.findAgent(payload.sessionId)
      if (!conn) continue
      send(conn.ws, {
        type: 'question-resolved',
        questionRpcId: payload.questionRpcId,
        outcome: payload.outcome,
      })
    }
  }
}

/**
 * apiProxy is optional when the bridge first composes. A child injection starts
 * the mux when the service appears and owns cleanup across service reloads.
 */
export function installQuestionRelay(ctx, dependencies) {
  return ctx.inject(['apiProxy'], (apiCtx) => {
    apiCtx.effect(() => {
      const abort = new AbortController()
      relayQuestionFrames({
        ...dependencies,
        apiProxy: apiCtx.apiProxy,
        signal: abort.signal,
      }).catch((error) => {
        if (!abort.signal.aborted) {
          apiCtx.logger?.warn?.(`[dsh-tui] question event stream failed: ${String(error?.message ?? error)}`)
        }
      })
      return () => abort.abort()
    }, 'dsh-tui: question event relay')
  })
}
