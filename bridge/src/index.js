/**
 * dsh-tui-bridge — WebSocket bridge for the dsh-tui terminal client.
 *
 * Host-composition plugin: registers one WebSocket upgrade route on the DSH
 * webServer and forwards session events / agent status to connected TUI
 * clients, while accepting user input, slash commands, and interrupts.
 *
 * Protocol (JSON, one message per frame; both directions carry `type`):
 *   up:   hello{token, resumeSessionId?, cwd?, mode?} | input{text}
 *         | command{line} | login-get{} | login-set{field,value}
 *         | interrupt{} | ping{}
 *   down: welcome{sessionId,status,provider?,model?,title?} | snapshot{events[]}
 *         | event{event} | status{status} | presets{presets[]} | title{title}
 *         | commands{commands[]} | command-result{commandId,kind,text?}
 *         | login{apiKeyConfigured,apiKeyWritable,apiKeySource?,apiKeyHint?,
 *                account?,proxy?,error?} | error{code,message} | pong{}
 *
 * Module layout (index.js keeps only the socket/session lifecycle):
 *   trim.js    — payload trimming (pure)
 *   compose.js — harness-home paths, session meta, model-selection hooks
 *   command.js — DSH command directory/result wire projection
 *   login.js   — the /login fields and their file/credentials seams
 */
import { randomBytes } from 'node:crypto'
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { WebSocket, WebSocketServer } from 'ws'
import { createUserMessage } from '@deepseek-ai/dsh-llm'
import { buildToolNames, trimToolResultEvent } from './trim.js'
import {
  dshHome,
  latestTitle,
  sessionPresetOf,
} from './compose.js'
import { shapeModelFrame } from './model.js'
import { renderSkillContent, skillInvocationSource } from './skill.js'
import { ConnectionRegistry } from './connection.js'
import { createHostPort } from './host.js'
import { createHistoryStore } from './history.js'
import { createSessionService } from './session.js'
import { createClientDispatcher } from './dispatcher.js'
import { shapeCommandsFrame, watchCommandChanges } from './command.js'
import { encodeBoundedFrame } from './frame.js'
import {
  MAX_FRAME_BYTES,
  PROTOCOL_VERSION,
  SNAPSHOT_CAP,
  SNAPSHOT_SURFACE,
} from './protocol.js'

const name = 'tui-bridge'
const inject = ['webServer']

function apply(ctx, config = {}) {
  const host = createHostPort(ctx)
  host.assertCore()
  const routePath = config.path ?? '/dsh-tui'
  const tokenFile = config.tokenFile ?? join(dshHome(), 'dsh-tui.token')

  // ---- connection token (D15) ----
  let token = null
  try {
    if (existsSync(tokenFile)) token = readFileSync(tokenFile, 'utf8').trim()
  } catch {}
  if (!token) {
    token = randomBytes(24).toString('hex')
    try {
      writeFileSync(tokenFile, `${token}\n`, 'utf8')
    } catch (error) {
      console.warn(`[dsh-tui] cannot persist token at ${tokenFile}: ${error.message}`)
    }
  }

  const wss = new WebSocketServer({ noServer: true })
  /** @type {Set<{ws: WebSocket, agent: object, abort: AbortController, off: () => void}>} */
  const conns = new ConnectionRegistry()
  /**
   * agentId → the mutable `{ current, assembled }` pair installModelSelection
   * closed over. `/model` mutates `.current` so the next turn's assembly (and
   * request routing) use the newly selected provider/model.
   */
  const modelSelections = new Map()

  // ---- user questions (ask_user_question) ----
  // The host's web UI owns the single userQuestions provider slot, so the
  // bridge does not register one. Instead it subscribes to the apiproxy mux
  // (the same broadcast the browser consumes) and relays question frames to
  // the TUI attached to that session; answers go back through apiProxy.respond.
  // Both UIs can answer — the host settles the first claimant.
  const apiProxy = host.apiProxy()
  /** rpcId -> sessionId, for answer routing even if the conn re-attaches. */
  const questionSessions = new Map()

  const send = (ws, message) => {
    if (ws.readyState === WebSocket.OPEN) {
      ws.send(encodeBoundedFrame(message, MAX_FRAME_BYTES))
    }
  }

  /** Project the effective, agent-scoped DSH command registry to one TUI. */
  function sendCommands(conn) {
    if (!conn || !conns.has(conn)) return
    try {
      const descriptors = host.commands()?.list(conn.agent) ?? []
      send(conn.ws, shapeCommandsFrame(descriptors))
    } catch (error) {
      ctx.logger?.warn?.(`[dsh-tui] command discovery failed: ${String(error?.message ?? error)}`)
      // A transient list() failure must not wipe the client's open plugin
      // directory — keep the previous roster and let the next commands/change
      // or attach refresh it.
    }
  }

  /**
   * Push the model catalog (providers × models) plus the agent's current
   * selection to one socket. Listing every provider's models is advisory
   * (`ctx.llm.listModels` may throw for an adapter without a catalog); a
   * failed provider is omitted rather than failing the whole frame.
   */
  async function sendModel(ws, agent) {
    const llm = host.llm()
    const providers = llm?.listProviders?.() ?? []
    const modelLists = {}
    await Promise.all(providers.map(async (p) => {
      try {
        modelLists[p.id] = await llm.listModels(p.id)
      } catch {
        modelLists[p.id] = []
      }
    }))
    const selection = modelSelections.get(agent.id)
    const current = selection?.current
      ?? (agent.options?.provider !== undefined && agent.options?.model !== undefined
        ? { provider: agent.options.provider, model: agent.options.model }
        : undefined)
    send(ws, { type: 'model', ...shapeModelFrame(providers, modelLists, current) })
  }

  // ---- snapshot / lazy history paging ----
  // Event roster and capacities come from protocol-contract.json, shared
  // with the Rust build; this module only owns filtering/caching behavior.
  const trimEvent = (event, toolNames) =>
    trimToolResultEvent(event, toolNames, SNAPSHOT_SURFACE)

  const historyStore = createHistoryStore({
    host,
    surfaceTypes: SNAPSHOT_SURFACE,
    trimEvent,
    buildToolNames,
    snapshotCap: SNAPSHOT_CAP,
  })
  const { historyEvents, sendSnapshot } = historyStore

  /**
   * `/skill:<name>` (and `/skill <name>`): look the skill up through the
   * host's `skills` registry (which already discovers `~/.agents/skills/`
   * and `<workspace>/.agents/skills/` with project over user priority) and
   * inject the rendered instructions as a user-explicit skill invocation,
   * mirroring dsh-tool-skill's `<skill_content>` injection. A missing skill
   * is an error, not a connection failure.
   */
  async function injectSkill(ws, conn, name) {
    const current = conn
    if (!current) return
    const skills = host.skills()
    if (!skills) {
      send(ws, { type: 'error', code: 'skill-unavailable', message: 'skills service unavailable' })
      return
    }
    try {
      const skill = await skills.get(name, {
        cwd: current.agent.session?.header?.cwd,
        signal: current.abort.signal,
        scope: current.agent,
      })
      // The socket may have been re-attached while the lookup ran.
      if (!conns.isCurrent(current, conn)) return
      if (!skill) {
        send(ws, { type: 'error', code: 'skill-unknown', message: `skill "${name}" is unknown or no longer available` })
        return
      }
      current.agent.followup(createUserMessage({
        content: [{ type: 'text', text: renderSkillContent(skill) }],
        source: skillInvocationSource(name),
      }))
    } catch (error) {
      if (!conns.isCurrent(current, conn)) return
      send(ws, { type: 'error', code: 'skill-failed', message: String(error?.message ?? error) })
    }
  }

  /** Bind one authenticated socket to a live agent session. */
  function attach(ws, agent, clientCwd) {
    const conn = {
      ws,
      agent,
      abort: new AbortController(),
      off: () => {},
      /** approval id -> resolve(outcome) */
      pending: new Map(),
      /** persisted log cache (non-live sessions only) */
      log: null,
      /** cached surface list (non-live sessions only) */
      surface: null,
      /** callId -> tool name (payload trimming) */
      toolNames: buildToolNames(agent.session?.events ?? []),
      /** the TUI's launch directory (hello.cwd), for /new workspace claims */
      clientCwd,
    }
    conns.add(conn)

    send(ws, {
      type: 'welcome',
      protocolVersion: PROTOCOL_VERSION,
      maxFrameBytes: MAX_FRAME_BYTES,
      sessionId: agent.id,
      status: agent.status,
      provider: agent.options?.provider,
      model: agent.options?.model,
      // Latest session/title of the log (undefined until the session has
      // one); live title updates ride the ordinary event frames.
      title: latestTitle(agent.session?.events),
      // Workspace path of the attached session (its header cwd) — the TUI
      // renders it right-aligned in the title row below the status bar.
      cwd: agent.session?.header?.cwd,
    })
    sendSnapshot(conn, send)
    // DSH/plugin commands are effective per agent (scoped definitions may
    // shadow globals), so discover them after every attach/session switch.
    sendCommands(conn)
    // Agent-preset roster for the client's `/new <mode>` suggestion popup.
    // Sent on every attach (hello, `/new`, picker) because the roster is
    // re-discovered on demand and edits to presets should reach the popup.
    void (async () => {
      try {
        const agentPresets = host.presets()
        const roster = agentPresets ? await agentPresets.list() : []
        send(ws, {
          type: 'presets',
          presets: roster.map((p) => ({
            id: p.id,
            ...(p.name !== undefined ? { name: p.name } : {}),
            ...(p.description !== undefined ? { description: p.description } : {}),
            ...(p.order !== undefined ? { order: p.order } : {}),
            ...(p.broken !== undefined ? { broken: p.broken } : {}),
          })),
        })
      } catch {
        send(ws, { type: 'presets', presets: [] })
      }
    })()
    // Cold-resumed sessions have their log on disk, not in memory: the
    // welcome title is absent then. Fetch the title snapshot (the same
    // cheap projection read the session picker uses) and push a `title`
    // frame — guarded so a reconnect/session switch in between cannot
    // stamp another session's title on this socket.
    void (async () => {
      try {
        if (latestTitle(agent.session?.events) !== undefined) return
        const query = host.sessionQuery()
        if (query === undefined) return
        const snapshots = await query.readTitleSnapshots([agent.id])
        const title = snapshots?.[0]?.title
        if (typeof title !== 'string' || title === '') return
        if (!conns.has(conn) || conn.agent.id !== agent.id) return
        send(ws, { type: 'title', title })
      } catch {}
    })()

    const offEvent = ctx.on('session/event', function (session, event) {
      if (session.id !== agent.id) return
      if (event.type === 'tool/call' && event.data?.callId) {
        conn.toolNames.set(event.data.callId, event.data?.name)
      }
      // Trim huge tool results on the wire too — the TUI never renders them.
      send(ws, { type: 'event', event: trimEvent(event, conn.toolNames) })
    })
    const offStatus = ctx.on('agent/status', function (payload) {
      if (payload.agent.id === agent.id) send(ws, { type: 'status', status: payload.status })
    })
    conn.off = () => { offEvent(); offStatus() }
    return conn
  }

  /**
   * Unbind a connection. `keepSocket` keeps the underlying WebSocket open —
   * used when the socket is immediately re-attached to another session
   * (`/new`, picker attach). Otherwise the socket is closed so the client
   * knows the connection is gone.
   */
  function detach(conn, options) {
    conns.detach(conn, options)
  }

  const sessionService = createSessionService({
    host,
    ctx,
    modelSelections,
    attach,
    detach,
    isCurrent: (conn) => conns.has(conn),
  })

  wss.on('connection', (ws) => {
    const dispatcher = createClientDispatcher({
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
    })
    ws.on('message', dispatcher.handle)
    ws.on('close', dispatcher.close)
    ws.on('error', () => {})
  })

  /** List live + persisted sessions with titles, newest first. */
  async function listSessions() {
    const query = host.sessionQuery()
    const persistence = host.persistence()
    if (!query || !persistence) return []
    const headers = await persistence.list()
    const live = new Set((host.agents()?.list() ?? []).map((a) => a.id))
    let titles = []
    try {
      const observations = await query.readTitleSnapshots(headers.map((h) => h.id))
      titles = observations
    } catch {
      titles = []
    }
    const byId = new Map()
    for (const t of titles) {
      if (t?.sessionId) byId.set(t.sessionId, t.title ?? '')
    }
    return headers
      .slice(0, 200)
      .map((h) => ({
        id: h.id,
        title: byId.get(h.id) ?? '',
        live: live.has(h.id),
        createdAt: h.createdAt,
      }))
      .sort((a, b) => b.createdAt - a.createdAt)
  }

  // Approval answerer (design §4.4): a connected TUI answers for its own
  // agent; without one the waterfall delegates to the next answerer.
  let approvalSeq = 0
  ctx.on('approval/request', (req, next) => {
    const conn = conns.findAgent(req.agent.id)
    if (!conn) return next()
    const id = `approval-${++approvalSeq}`
    send(conn.ws, {
      type: 'approval',
      id,
      toolName: req.toolName,
      reason: req.reason ?? '',
      callId: req.callId,
    })
    return new Promise((resolve) => {
      let settled = false
      const done = (outcome) => {
        if (settled) return
        settled = true
        conn.pending.delete(id)
        resolve(outcome)
      }
      conn.pending.set(id, done)
      req.signal?.addEventListener('abort', () => done('cancelled'), { once: true })
    })
  })

  // Relay the apiproxy mux's question frames to the attached TUI. One
  // subscription per bridge instance; frames are broadcast for every
  // session, so filter by the sessions a TUI is currently attached to.
  let questionSub = null
  if (apiProxy?.events?.mux) {
    const muxAbort = new AbortController()
    questionSub = { abort: () => muxAbort.abort() }
    ;(async () => {
      for await (const frame of apiProxy.events.mux({}, muxAbort.signal)) {
        const payload = frame?.payload
        if (!payload || typeof payload !== 'object') continue
        if (payload.type === 'question/requested') {
          if (questionSessions.size > 64) questionSessions.clear()
          const conn = conns.findAgent(payload.sessionId)
          if (!conn) continue
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
    })().catch(() => {})
  }

  // Registrations can change at runtime (plugin reload or agent-scoped
  // composition). DSH emits one unfiltered notification; recompute each
  // attached agent's effective view instead of trying to patch it locally.
  ctx.effect(
    () => watchCommandChanges(ctx, conns, sendCommands),
    'dsh-tui: command directory updates',
  )

  ctx.effect(() => {
    const disposeRoute = ctx.webServer.registerUpgrade({
      path: routePath,
      handler: (req, socket, head) => {
        wss.handleUpgrade(req, socket, head, (ws) => wss.emit('connection', ws, req))
      },
    })
    return () => {
      questionSub?.abort()
      disposeRoute()
      for (const conn of [...conns]) detach(conn)
      wss.close()
    }
  })
}

export {
  apply,
  inject,
  name,
  trimToolResultEvent as _trimToolResultEvent,
  latestTitle as _latestTitle,
  sessionPresetOf as _sessionPresetOf,
}
