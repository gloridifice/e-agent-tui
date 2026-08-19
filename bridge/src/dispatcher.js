import { createProxy, deleteProxy, sendLogin, setProviderApiKey } from './login.js'
import { shapeCommandResultFrame } from './command.js'
import { parseSkillCommand } from './skill.js'
import { HISTORY_CAP, PROTOCOL_VERSION } from './protocol.js'

/** Per-socket client-message router. It owns mutable attachment/login state;
 * the bridge composition root only wires lifecycle effects and host adapters. */
export function createClientDispatcher({
  ws,
  token,
  ctx,
  host,
  send,
  conns,
  attach,
  detach,
  sessionService,
  injectSkill,
  historyEvents,
  listSessions,
  apiProxy,
  questionSessions,
  sendModel,
  modelSelections,
  createUserMessage,
}) {
  let conn = null

  async function hello(msg) {
    if (msg.token !== token) {
      send(ws, { type: 'error', code: 'bad-token', message: 'token rejected' })
      ws.close(4003)
      return
    }
    if (typeof msg.protocolVersion === 'number' && msg.protocolVersion > PROTOCOL_VERSION) {
      send(ws, { type: 'error', code: 'protocol-newer', message: `client protocol ${msg.protocolVersion} is newer than bridge protocol ${PROTOCOL_VERSION}` })
      ws.close(4002)
      return
    }
    const clientCwd = typeof msg.cwd === 'string' && msg.cwd.trim() !== '' ? msg.cwd.trim() : undefined
    try {
      let agent
      if (typeof msg.resumeSessionId === 'string' && msg.resumeSessionId !== '') {
        agent = host.agents()?.get(msg.resumeSessionId)
          ?? await sessionService.resumePersistedSession(msg.resumeSessionId)
      }
      conn = agent !== undefined
        ? attach(ws, agent, clientCwd)
        : await sessionService.createNewSession(ws, null, msg.mode, { clientCwd, fallbackStandard: true })
    } catch (error) {
      send(ws, { type: 'error', code: 'hello-failed', message: String(error?.message ?? error) })
      ws.close(4001)
    }
  }

  async function newInput(msg) {
    if (!conn || typeof msg.mode !== 'string' || typeof msg.text !== 'string') return
    const mode = msg.mode.trim()
    if (mode === '' || mode.split(/\s+/).length !== 1 || msg.text === '') {
      send(ws, { type: 'error', code: 'new-failed', message: 'invalid new conversation input' })
      return
    }
    const current = conn
    let next
    try {
      next = await sessionService.createNewSession(ws, current, mode)
    } catch (error) {
      if (conns.isCurrent(current, conn)) {
        send(ws, { type: 'error', code: 'new-failed', message: String(error?.message ?? error) })
      }
      return
    }
    if (next === undefined || !(conn === current || !conns.has(current))) return
    conn = next
    try {
      next.agent.followup(createUserMessage({
        content: [{ type: 'text', text: msg.text }],
        source: { kind: 'user' },
      }))
    } catch (error) {
      if (conns.isCurrent(next, conn)) {
        send(ws, { type: 'error', code: 'new-input-failed', message: String(error?.message ?? error) })
      }
    }
  }

  function command(msg) {
    if (!conn || typeof msg.line !== 'string') return
    const trimmed = msg.line.trim()
    if (trimmed === '/new' || trimmed.startsWith('/new ')) {
      const tokens = trimmed.slice(4).trim().split(/\s+/).filter(Boolean)
      if (tokens.length > 1) {
        send(ws, { type: 'error', code: 'new-failed', message: '用法: /new 或 /new <模式>' })
        return
      }
      const current = conn
      sessionService.createNewSession(ws, current, tokens[0])
        .then((next) => {
          if (next !== undefined && (conn === current || !conns.has(current))) conn = next
        })
        .catch((error) => send(ws, { type: 'error', code: 'new-failed', message: String(error?.message ?? error) }))
      return
    }
    const skillName = parseSkillCommand(trimmed)
    if (skillName !== undefined) {
      injectSkill(ws, conn, skillName)
      return
    }
    const commands = host.commands()
    if (!commands) {
      send(ws, { type: 'error', code: 'no-commands', message: 'commands service unavailable' })
      return
    }
    // DSH execution is async and the same socket closure can be re-attached
    // meanwhile. Never deliver one session's direct command result to the
    // next session occupying that socket. Each execution gets its own abort
    // controller so Esc can cancel commands without detaching the session.
    const current = conn
    const commandAbort = new AbortController()
    current.commandAborts ??= new Set()
    current.commandAborts.add(commandAbort)
    const abortOnDetach = () => commandAbort.abort()
    current.abort.signal.addEventListener?.('abort', abortOnDetach, { once: true })
    Promise.resolve()
      .then(() => commands.execute(current.agent, msg.line, commandAbort.signal))
      .then((execution) => {
        if (!conns.isCurrent(current, conn)) return
        if (commandAbort.signal.aborted) {
          send(ws, { type: 'error', code: 'command-cancelled', message: 'command cancelled' })
          return
        }
        if (execution === undefined) {
          send(ws, { type: 'error', code: 'command-unknown', message: `unknown command: ${trimmed}` })
          return
        }
        const frame = shapeCommandResultFrame(execution)
        if (frame === undefined) {
          send(ws, { type: 'error', code: 'command-invalid-result', message: `command returned an invalid result: ${trimmed}` })
          return
        }
        send(ws, frame)
      })
      .catch((error) => {
        if (!conns.isCurrent(current, conn)) return
        if (commandAbort.signal.aborted) {
          send(ws, { type: 'error', code: 'command-cancelled', message: 'command cancelled' })
        } else {
          send(ws, { type: 'error', code: 'command-failed', message: String(error?.message ?? error) })
        }
      })
      .finally(() => {
        current.commandAborts.delete(commandAbort)
        current.abort.signal.removeEventListener?.('abort', abortOnDetach)
      })
  }

  async function attachSession(msg) {
    const current = conn
    if (!current || typeof msg.sessionId !== 'string') return
    try {
      const nextAgent = host.agents()?.get(msg.sessionId)
        ?? await sessionService.resumePersistedSession(msg.sessionId)
      if (!nextAgent) {
        send(ws, { type: 'error', code: 'no-live-session', message: `no live agent ${msg.sessionId}` })
        return
      }
      if (!conns.isCurrent(current, conn)) return
      const clientCwd = current.clientCwd
      detach(current, { keepSocket: true })
      conn = attach(ws, nextAgent, clientCwd)
    } catch (error) {
      send(ws, { type: 'error', code: 'attach-failed', message: String(error?.message ?? error) })
    }
  }

  function answerQuestions(msg, cancelled) {
    const currentApiProxy = typeof apiProxy === 'function' ? apiProxy() : apiProxy
    if (!conn || !currentApiProxy || typeof msg.rpcId !== 'string') return
    const sessionId = questionSessions.get(msg.rpcId)
    if (sessionId === undefined) {
      if (!cancelled) send(ws, { type: 'error', code: 'answer-failed', message: 'question no longer pending' })
      return
    }
    const result = cancelled
      ? { ok: false, error: { code: 'cancelled', message: 'the user cancelled ask_user_question', details: {} } }
      : { ok: true, value: { sessionId, answer: { answers: msg.answers } } }
    currentApiProxy.respond({ type: 'client-response', rpcId: msg.rpcId, result }, conn.abort.signal).then(
      (receipt) => {
        if (!cancelled && !receipt.accepted) {
          send(ws, { type: 'error', code: 'answer-failed', message: receipt.reason ?? 'rejected' })
        }
      },
      () => {},
    )
  }

  async function setApiKey(msg) {
    try {
      await setProviderApiKey(ctx, msg.provider, msg.value)
      await sendLogin(ctx, send, ws)
    } catch (error) {
      await sendLogin(ctx, send, ws, String(error?.message ?? error))
    }
  }

  async function saveProxy(msg) {
    try {
      createProxy({
        baseUrl: msg.baseUrl,
        apiKey: typeof msg.apiKey === 'string' ? msg.apiKey : '',
        protocol: typeof msg.protocol === 'string' ? msg.protocol : '',
        model: typeof msg.model === 'string' ? msg.model : '',
      })
      await sendLogin(ctx, send, ws)
    } catch (error) {
      await sendLogin(ctx, send, ws, String(error?.message ?? error))
    }
  }

  async function removeProxy(msg) {
    try {
      deleteProxy(msg.id)
      await sendLogin(ctx, send, ws)
    } catch (error) {
      await sendLogin(ctx, send, ws, String(error?.message ?? error))
    }
  }

  function setModel(msg) {
    const current = conn
    if (!current || typeof msg.provider !== 'string' || typeof msg.model !== 'string') return
    const selection = modelSelections.get(current.agent.id)
    if (selection) selection.current = { provider: msg.provider, model: msg.model }
    try {
      current.agent.options = { ...(current.agent.options ?? {}), provider: msg.provider, model: msg.model }
    } catch {}
    sendModel(ws, current.agent).catch((error) => {
      send(ws, { type: 'error', code: 'model-failed', message: String(error?.message ?? error) })
    })
  }

  function handle(data) {
    let msg
    try { msg = JSON.parse(data.toString()) } catch { return false }
    switch (msg?.type) {
      case 'hello': void hello(msg); break
      case 'input':
        if (conn && typeof msg.text === 'string' && msg.text !== '') {
          conn.agent.followup(createUserMessage({ content: [{ type: 'text', text: msg.text }], source: { kind: 'user' } }))
        }
        break
      case 'new-input': void newInput(msg); break
      case 'command': command(msg); break
      case 'attach': void attachSession(msg); break
      case 'history':
        if (conn && typeof msg.beforeSeq === 'number' && typeof msg.limit === 'number') {
          const limit = Math.min(Math.max(1, msg.limit | 0), HISTORY_CAP)
          const { events, hasMore } = historyEvents(conn, msg.beforeSeq, limit)
          send(ws, { type: 'history', events, hasMore })
        }
        break
      case 'list-sessions': {
        const current = conn
        if (!current) break
        const sendSessions = (sessions, titlesPending = false) => {
          if (conns.isCurrent(current, conn)) send(ws, {
            type: 'sessions', sessions, ...(titlesPending ? { titlesPending: true } : {}),
          })
        }
        listSessions((sessions) => sendSessions(sessions, true)).then(
          sendSessions,
          (error) => {
            if (conns.isCurrent(current, conn)) {
              send(ws, { type: 'error', code: 'sessions-failed', message: String(error?.message ?? error) })
            }
          },
        )
        break
      }
      case 'approval-answer': {
        const done = conn?.pending.get(msg.id)
        if (done) done(msg.allow ? 'allowed-once' : 'rejected')
        break
      }
      case 'answer-questions':
        if (Array.isArray(msg.answers)) answerQuestions(msg, false)
        break
      case 'cancel-questions': answerQuestions(msg, true); break
      case 'login-get': if (conn) sendLogin(ctx, send, ws).catch(() => {}); break
      case 'login-set-api-key':
        if (conn && typeof msg.provider === 'string' && typeof msg.value === 'string') void setApiKey(msg)
        break
      case 'login-proxy-create': if (conn && typeof msg.baseUrl === 'string') void saveProxy(msg); break
      case 'login-proxy-delete': if (conn && typeof msg.id === 'string') void removeProxy(msg); break
      case 'model-get':
        if (conn) sendModel(ws, conn.agent).catch((error) => {
          send(ws, { type: 'error', code: 'model-failed', message: String(error?.message ?? error) })
        })
        break
      case 'model-set': setModel(msg); break
      case 'interrupt':
        if (conn) {
          for (const commandAbort of conn.commandAborts ?? []) commandAbort.abort()
          conn.agent.cancel({ kind: 'user' })
        }
        break
      case 'ping': send(ws, { type: 'pong' }); break
      default: return false
    }
    return true
  }

  function close() {
    if (conn) detach(conn)
    conn = null
  }

  return Object.freeze({ handle, close, connection: () => conn })
}
