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
 *         | login{apiKeyConfigured,apiKeyWritable,apiKeySource?,apiKeyHint?,
 *                account?,proxy?,error?} | error{code,message} | pong{}
 *
 * Module layout (index.js keeps only the socket/session lifecycle):
 *   trim.js    — payload trimming (pure)
 *   compose.js — harness-home paths, session meta, model-selection hooks
 *   login.js   — the /login fields and their file/credentials seams
 */
import { randomBytes, randomUUID } from 'node:crypto'
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { WebSocket, WebSocketServer } from 'ws'
import { createUserMessage } from '@deepseek-ai/dsh-llm'
import { buildToolNames, trimToolResultEvent } from './trim.js'
import {
  defaultModelSelection,
  dshHome,
  installModelSelection,
  isExistingDirectory,
  latestTitle,
  sessionPresetOf,
} from './compose.js'
import { createProxy, runCodexLogin, sendLogin, setProviderApiKey } from './login.js'
import { shapeModelFrame } from './model.js'
import { parseSkillCommand, renderSkillContent, skillInvocationSource } from './skill.js'

const name = 'tui-bridge'
const inject = ['webServer']

function apply(ctx, config = {}) {
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
  const conns = new Set()
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
  const apiProxy = ctx.get('apiProxy')
  /** rpcId -> sessionId, for answer routing even if the conn re-attaches. */
  const questionSessions = new Map()

  const send = (ws, message) => {
    if (ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify(message))
  }

  /**
   * Push the model catalog (providers × models) plus the agent's current
   * selection to one socket. Listing every provider's models is advisory
   * (`ctx.llm.listModels` may throw for an adapter without a catalog); a
   * failed provider is omitted rather than failing the whole frame.
   */
  async function sendModel(ws, agent) {
    const llm = ctx.get('llm')
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
  // Surface events only (raw stream chunks are skipped because
  // assistant/message carries the assembled text).
  const SNAPSHOT_SURFACE = new Set([
    'user/message', 'assistant/message', 'tool/call', 'tool/result',
    'turn/start', 'turn/end', 'todo/write',
  ])
  // Replay-window budgets. SNAPSHOT_CAP is the WIRE budget — one welcome
  // frame must stay small for a cold terminal attach, so it is deliberately
  // lower than the client's own local replay guard (model.rs
  // SNAPSHOT_SURFACE_CAP=2000): the client cap protects its render cache
  // from pathological logs, the bridge cap protects the socket frame.
  // HISTORY_CAP bounds one lazy scroll-back page (independent knob — a
  // PageUp burst must not flood the client).
  const SNAPSHOT_CAP = 600
  const HISTORY_CAP = 2000
  const trimEvent = trimToolResultEvent

  // Per-session caches: the surface list is built once from the in-memory log
  // and appended incrementally, so reconnects don't re-filter hundreds of
  // thousands of events.
  const surfaceState = new Map() // sessionId -> { list, lastSeq }
  function surfaceFor(conn) {
    const live = conn.agent.session?.events
    if (!live) {
      // Non-live session: cache the filtered log on the connection.
      if (!conn.surface) conn.surface = (conn.log ?? []).filter((e) => SNAPSHOT_SURFACE.has(e.type))
      return conn.surface
    }
    if (surfaceState.size > 64) surfaceState.clear()
    let st = surfaceState.get(conn.agent.id)
    if (!st) {
      st = { list: [], lastSeq: -1 }
      for (const e of live) {
        const seq = e.seq ?? 0
        if (seq > st.lastSeq) st.lastSeq = seq
        if (SNAPSHOT_SURFACE.has(e.type)) st.list.push(e)
      }
      surfaceState.set(conn.agent.id, st)
    } else {
      // Append events newer than the last observed seq.
      for (const e of live) {
        const seq = e.seq ?? 0
        if (seq > st.lastSeq) {
          st.lastSeq = seq
          if (SNAPSHOT_SURFACE.has(e.type)) st.list.push(e)
        }
      }
    }
    return st.list
  }

  function recentEvents(conn, limit) {
    const filtered = surfaceFor(conn)
    const tail = filtered.slice(-limit)
    return {
      events: tail.map((e) => trimEvent(e, conn.toolNames)),
      hasMore: filtered.length > tail.length,
    }
  }

  /** One page of history before `beforeSeq` (lazy scroll-back). */
  function historyEvents(conn, beforeSeq, limit) {
    const older = surfaceFor(conn).filter((e) => (e.seq ?? 0) < beforeSeq)
    const page = older.slice(-limit)
    return {
      events: page.map((e) => trimEvent(e, conn.toolNames)),
      hasMore: older.length > page.length,
    }
  }

  function sendSnapshot(conn) {
    const live = conn.agent.session?.events
    if (live) {
      const { events, hasMore } = recentEvents(conn, SNAPSHOT_CAP)
      send(conn.ws, { type: 'snapshot', events, truncated: hasMore })
      return
    }
    // Non-live session: read once, cache on the connection.
    const persistence = ctx.get('sessionPersistence')
    if (!persistence) {
      send(conn.ws, { type: 'snapshot', events: [], truncated: false })
      return
    }
    persistence.readFrom(conn.agent.id, 0).then(
      ({ events }) => {
        conn.log = events
        conn.toolNames = buildToolNames(events)
        const { events: tail, hasMore } = recentEvents(conn, SNAPSHOT_CAP)
        send(conn.ws, { type: 'snapshot', events: tail, truncated: hasMore })
      },
      () => send(conn.ws, { type: 'snapshot', events: [], truncated: false }),
    )
  }

  /**
   * `/new [mode]` and the hello-time session creation: create a fresh
   * session mirroring the base agent's model and preset (a bare `/new`), or
   * the named mode (a preset id from the agentPresets roster —
   * `standard`/`code`/`minimal`/`cordis`/…), then claim it for the
   * workspace that owns its cwd and re-attach this socket. The DSH host has
   * no `/new` command of its own (its commands registry knows
   * compact/goal/plan/…), so the bridge implements it with the same
   * `agents.create` primitive the host's own session creation uses,
   * followed by the same workspace attach the host performs after
   * `session.create`.
   *
   * The workspace comes from the TUI's own launch directory when the client
   * sent one (`hello.cwd`) — that is "the current directory" from the
   * terminal's point of view. Without it the session would always re-open
   * in the ATTACHED session's header cwd (or the host's launch cwd), which
   * is where the old `/new` stranded every session.
   *
   * `conn` is null on the hello path (no session to mirror yet) — the
   * connection object is created by the `attach` this function returns.
   * With `fallbackStandard`, a stale/unknown mode (the TUI's configurable
   * default-mode setting can outlive its preset) falls back to `standard`,
   * then to the roster default, instead of failing the connection.
   */
  async function createNewSession(ws, conn, mode, opts = {}) {
    const agents = ctx.get('agents')
    if (!agents) throw new Error('agents service unavailable')
    const current = conn?.agent
    const clientCwd = conn?.clientCwd ?? opts.clientCwd
    const agentOptions = {}
    if (current?.options?.provider) agentOptions.provider = current.options.provider
    if (current?.options?.model) agentOptions.model = current.options.model
    // Workspace: the client's launch directory wins when it is a real dir;
    // sessions predating per-cwd headers fall back to the host's own
    // default (its launch cwd).
    const inheritedCwd = current?.session?.header?.cwd ?? process.cwd()
    const cwd = isExistingDirectory(clientCwd) ? clientCwd : inheritedCwd
    // Mode: the named preset id wins; a bare `/new` mirrors the current
    // session's composition (the roster default when the current agent
    // joined none). `resolve` throws for unknown ids and carries the
    // available ids on the error.
    const agentPresets = ctx.get('agentPresets')
    let preset
    if (agentPresets) {
      const wanted = mode
        ?? agentPresets.composedPreset(current?.ctx)
        ?? current?.session?.header?.agentPreset
      const attempts = opts.fallbackStandard ? [wanted, 'standard', undefined] : [wanted]
      let lastError
      for (const candidate of attempts) {
        try {
          preset = await agentPresets.resolve(candidate)
          break
        } catch (error) {
          lastError = error
          preset = undefined
        }
      }
      if (preset === undefined) {
        const available = Array.isArray(lastError?.available) && lastError.available.length > 0
          ? `（可用: ${lastError.available.join(', ')}）`
          : ''
        throw new Error(`agent-presets: ${String(lastError?.message ?? lastError)}${available}`)
      }
    }
    // Create first (still attached to the old session), then swap the
    // connection over synchronously so no message lands on a dead conn.
    // The socket itself survives the swap — closing it here would kill the
    // client mid-re-attach (the `/new` crash).
    // Model selection: `/new` mirrors the current session's provider/model
    // (the persona's `{{model}}` variable and the request routing read this
    // selection); a fresh process falls back to the host's default.
    const mirror = current?.options?.provider !== undefined && current?.options?.model !== undefined
      ? { provider: current.options.provider, model: current.options.model }
      : undefined
    const modelSelection = { current: mirror ?? defaultModelSelection(ctx), assembled: undefined }
    const { agent } = await agents.create({
      sessionId: `session-${randomUUID()}`,
      meta: {
        cwd,
        ...(preset ? { agentPreset: preset.id } : {}),
      },
      agentOptions,
      // `setup` installs the model selection first, then mounts the preset's
      // tools/prompt sections into the fresh agent — the header field alone
      // only labels it. The host's `session.create` composes exactly this
      // way (setup runs inside the creation window, so a broken preset rolls
      // back instead of publishing a half-composed session).
      setup: async (agentCtx) => {
        installModelSelection(agentCtx, modelSelection)
        if (preset) await agentPresets.mount(agentCtx, preset.id)
      },
    })
    modelSelections.set(agent.id, modelSelection)
    // The cwd header alone does not make the session a member of its
    // workspace: the workspace registry keeps an explicit session account
    // (and a session-path index) that only `attachSession` feeds. Without
    // it the conversation opens fine but shows up outside the current
    // workspace in the web sidebar.
    const workspaceRegistry = ctx.get('workspaceRegistry')
    if (workspaceRegistry) {
      let workspace
      try {
        workspace = await workspaceRegistry.resolveByPath(cwd)
      } catch {
        workspace = undefined
      }
      try {
        if (workspace === undefined) workspace = await workspaceRegistry.create(cwd)
        await workspace.attachSession(agent.id)
      } catch (error) {
        // The conversation is already live — a workspace-claim failure must
        // not strand the socket on the old session. Log it; the session
        // keeps its cwd header either way.
        console.warn(`[dsh-tui] /new: session ${agent.id} created but could not join the workspace at ${cwd}: ${String(error?.message ?? error)}`)
      }
    }
    if (conn) detach(conn, { keepSocket: true })
    return attach(ws, agent, clientCwd)
  }

  /**
   * Load a persisted-but-cold session onto a live agent (hello/attach
   * resume). Mirrors the host's cold-resume: compose the preset the session
   * RECORDED (its history must not be replayed under a different tool set),
   * so `presets.mount` runs inside the resume `setup` like it does for
   * creation. Returns undefined when the session is not persisted, its
   * recorded preset is gone, or any step fails — callers then open a fresh
   * session instead of killing the connection (a stale resume id is a
   * restart race, not a user error).
   */
  async function resumePersistedSession(sessionId) {
    const agents = ctx.get('agents')
    const persistence = ctx.get('sessionPersistence')
    const agentPresets = ctx.get('agentPresets')
    if (!agents || !persistence) return undefined
    try {
      const headers = await persistence.list()
      if (!headers.some((h) => h.id === sessionId)) return undefined
      const inspected = await persistence.inspect(sessionId)
      let preset
      if (agentPresets) {
        try {
          preset = await agentPresets.resolve(sessionPresetOf(inspected.meta, inspected.events))
        } catch {
          return undefined
        }
      }
      const selection = { current: defaultModelSelection(ctx), assembled: undefined }
      const { agent } = await agents.resume({
        resumeSessionId: sessionId,
        // Same composition as the host's cold resume: the default model
        // selection first (the persona `{{model}}` variable and request
        // routing read it), then the preset the session recorded.
        setup: async (agentCtx) => {
          installModelSelection(agentCtx, selection)
          if (preset) await agentPresets.mount(agentCtx, preset.id)
        },
      })
      modelSelections.set(agent.id, selection)
      return agent
    } catch {
      return undefined
    }
  }

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
    const skills = ctx.get('skills')
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
      if (conn !== current || !conns.has(current)) return
      if (!skill) {
        send(ws, { type: 'error', code: 'skill-unknown', message: `skill "${name}" is unknown or no longer available` })
        return
      }
      current.agent.followup(createUserMessage({
        content: [{ type: 'text', text: renderSkillContent(skill) }],
        source: skillInvocationSource(name),
      }))
    } catch (error) {
      if (conn !== current || !conns.has(current)) return
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
    sendSnapshot(conn)
    // Agent-preset roster for the client's `/new <mode>` suggestion popup.
    // Sent on every attach (hello, `/new`, picker) because the roster is
    // re-discovered on demand and edits to presets should reach the popup.
    void (async () => {
      try {
        const agentPresets = ctx.get('agentPresets')
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
        const query = ctx.get('sessionQuery')
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
  function detach(conn, { keepSocket = false } = {}) {
    conns.delete(conn)
    conn.off()
    conn.abort.abort()
    // Never leave a pending approval hanging the agent: withdraw them.
    for (const done of conn.pending.values()) done('cancelled')
    conn.pending.clear()
    if (
      !keepSocket
      && (conn.ws.readyState === WebSocket.OPEN || conn.ws.readyState === WebSocket.CONNECTING)
    ) {
      conn.ws.close(1000)
    }
  }

  wss.on('connection', (ws) => {
    /** @type {ReturnType<typeof attach> | null} */
    let conn = null
    /** @type {AbortController | null} in-flight Codex device login */
    let codexAbort = null

    ws.on('message', (data) => {
      let msg
      try { msg = JSON.parse(data.toString()) } catch { return }

      switch (msg?.type) {
        case 'hello': {
          if (msg.token !== token) {
            send(ws, { type: 'error', code: 'bad-token', message: 'token rejected' })
            ws.close(4003)
            return
          }
          // The TUI's launch directory: new sessions open here (validated
          // as an existing dir at creation time).
          const clientCwd = typeof msg.cwd === 'string' && msg.cwd.trim() !== '' ? msg.cwd.trim() : undefined
          void (async () => {
            try {
              let agent
              if (typeof msg.resumeSessionId === 'string' && msg.resumeSessionId !== '') {
                // Explicit resume (`dsh tui <id>`, remember-last-session):
                // attach the live agent, or reload the persisted session
                // (it is cold right after a host restart). A missing target
                // must never kill the client — the fresh-session default
                // below catches it.
                agent = ctx.get('agents')?.get(msg.resumeSessionId)
                  ?? await resumePersistedSession(msg.resumeSessionId)
              }
              conn = agent !== undefined
                ? attach(ws, agent, clientCwd)
                // A fresh TUI process without a resume target (or with a
                // stale one) opens a NEW session — each process shows one
                // session — in its launch directory, on the client's
                // configured default mode (fallback: standard).
                : await createNewSession(ws, null, msg.mode, { clientCwd, fallbackStandard: true })
            } catch (error) {
              send(ws, { type: 'error', code: 'hello-failed', message: String(error?.message ?? error) })
              ws.close(4001)
            }
          })()
          break
        }
        case 'input': {
          if (!conn || typeof msg.text !== 'string' || msg.text === '') return
          conn.agent.followup(createUserMessage({
            content: [{ type: 'text', text: msg.text }],
            source: { kind: 'user' },
          }))
          break
        }
        case 'command': {
          if (!conn || typeof msg.line !== 'string') return
          const trimmed = msg.line.trim()
          // `/new` and `/new <mode>` are the bridge's own command; the mode
          // is one preset id from the agentPresets roster.
          if (trimmed === '/new' || trimmed.startsWith('/new ')) {
            const tokens = trimmed.slice(4).trim().split(/\s+/).filter(Boolean)
            if (tokens.length > 1) {
              send(ws, { type: 'error', code: 'new-failed', message: '用法: /new 或 /new <模式>' })
              return
            }
            createNewSession(ws, conn, tokens[0])
              .then((next) => { conn = next })
              .catch((error) => {
                send(ws, { type: 'error', code: 'new-failed', message: String(error?.message ?? error) })
              })
            return
          }
          // `/skill:<name>` is the bridge's own command: inject the skill's
          // instructions into the session (DSH has no such slash command).
          const skillName = parseSkillCommand(trimmed)
          if (skillName !== undefined) {
            injectSkill(ws, conn, skillName)
            return
          }
          const commands = ctx.get('commands')
          if (!commands) {
            send(ws, { type: 'error', code: 'no-commands', message: 'commands service unavailable' })
            return
          }
          commands.execute(conn.agent, msg.line, conn.abort.signal).catch((error) => {
            send(ws, { type: 'error', code: 'command-failed', message: String(error?.message ?? error) })
          })
          break
        }
        case 'attach': {
          // Capture the connection this request belongs to BEFORE any
          // await: while `resumePersistedSession` runs, another attach or
          // `/new` message may swap the closure's `conn`, and detaching the
          // newer connection would strand or corrupt this socket.
          const current = conn
          if (!current || typeof msg.sessionId !== 'string') return
          void (async () => {
            // Picker / `/resume <id>`: live agents attach directly; a
            // persisted-but-cold session is resumed first (same path as
            // hello). The connection itself never closes on a miss.
            const agents = ctx.get('agents')
            const nextAgent = agents?.get(msg.sessionId) ?? await resumePersistedSession(msg.sessionId)
            if (!nextAgent) {
              send(ws, { type: 'error', code: 'no-live-session', message: `no live agent ${msg.sessionId}` })
              return
            }
            // A later message already moved this socket — abandon silently
            // (the newer operation owns it now).
            if (conn !== current || !conns.has(current)) return
            const clientCwd = current.clientCwd
            detach(current, { keepSocket: true })
            conn = attach(ws, nextAgent, clientCwd)
          })().catch((error) => {
            send(ws, { type: 'error', code: 'attach-failed', message: String(error?.message ?? error) })
          })
          break
        }
        case 'history': {
          if (!conn || typeof msg.beforeSeq !== 'number' || typeof msg.limit !== 'number') return
          const limit = Math.min(Math.max(1, msg.limit | 0), HISTORY_CAP)
          const { events, hasMore } = historyEvents(conn, msg.beforeSeq, limit)
          send(ws, { type: 'history', events, hasMore })
          break
        }
        case 'list-sessions': {
          void (async () => {
            try {
              const sessions = await listSessions()
              send(ws, { type: 'sessions', sessions })
            } catch (error) {
              send(ws, { type: 'error', code: 'sessions-failed', message: String(error?.message ?? error) })
            }
          })()
          break
        }
        case 'approval-answer': {
          if (!conn || typeof msg.id !== 'string') return
          const done = conn.pending.get(msg.id)
          if (!done) return
          done(msg.allow ? 'allowed-once' : 'rejected')
          break
        }
        case 'answer-questions': {
          if (!conn || !apiProxy || typeof msg.rpcId !== 'string' || !Array.isArray(msg.answers)) return
          const sessionId = questionSessions.get(msg.rpcId)
          if (sessionId === undefined) {
            send(ws, { type: 'error', code: 'answer-failed', message: 'question no longer pending' })
            return
          }
          apiProxy.respond({
            type: 'client-response',
            rpcId: msg.rpcId,
            result: {
              ok: true,
              value: { sessionId, answer: { answers: msg.answers } },
            },
          }, conn.abort.signal).then(
            (receipt) => {
              if (!receipt.accepted) {
                send(ws, { type: 'error', code: 'answer-failed', message: receipt.reason ?? 'rejected' })
              }
            },
            () => {},
          )
          break
        }
        case 'cancel-questions': {
          if (!conn || !apiProxy || typeof msg.rpcId !== 'string') return
          const sessionId = questionSessions.get(msg.rpcId)
          if (sessionId === undefined) return
          apiProxy.respond({
            type: 'client-response',
            rpcId: msg.rpcId,
            result: {
              ok: false,
              error: {
                code: 'cancelled',
                message: 'the user cancelled ask_user_question',
                details: {},
              },
            },
          }, conn.abort.signal).catch(() => {})
          break
        }
        case 'login-get': {
          if (!conn) return
          sendLogin(ctx, send, ws).catch(() => {})
          break
        }
        case 'login-set-api-key': {
          if (!conn || typeof msg.provider !== 'string' || typeof msg.value !== 'string') return
          void (async () => {
            try {
              await setProviderApiKey(ctx, msg.provider, msg.value)
              await sendLogin(ctx, send, ws)
            } catch (error) {
              // The state frame carries the message so the login page can
              // show it inline (the transcript error path stays reserved).
              await sendLogin(ctx, send, ws, String(error?.message ?? error))
            }
          })()
          break
        }
        case 'login-codex-start': {
          if (!conn || codexAbort) return
          codexAbort = new AbortController()
          runCodexLogin(send, ws, codexAbort.signal)
            .then(() => {
              codexAbort = null
              sendLogin(ctx, send, ws).catch(() => {})
            })
            .catch((error) => {
              codexAbort = null
              const message = String(error?.message ?? error)
              if (message !== 'Login cancelled') {
                send(ws, { type: 'login-codex', status: 'error', error: message })
              }
            })
          break
        }
        case 'login-codex-cancel': {
          codexAbort?.abort()
          codexAbort = null
          break
        }
        case 'login-proxy-create': {
          if (!conn || typeof msg.baseUrl !== 'string') return
          void (async () => {
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
          })()
          break
        }
        case 'login-proxy-delete': {
          if (!conn || typeof msg.id !== 'string') return
          void (async () => {
            try {
              deleteProxy(msg.id)
              await sendLogin(ctx, send, ws)
            } catch (error) {
              await sendLogin(ctx, send, ws, String(error?.message ?? error))
            }
          })()
          break
        }
        case 'model-get': {
          if (!conn) return
          sendModel(ws, conn.agent).catch((error) => {
            send(ws, { type: 'error', code: 'model-failed', message: String(error?.message ?? error) })
          })
          break
        }
        case 'model-set': {
          if (!conn || typeof msg.provider !== 'string' || typeof msg.model !== 'string') return
          const current = conn
          const selection = modelSelections.get(current.agent.id)
          if (selection) selection.current = { provider: msg.provider, model: msg.model }
          // Keep the agent's own metadata in step so `welcome`/status and any
          // later `/new` mirror reflect the switch.
          try {
            current.agent.options = { ...(current.agent.options ?? {}), provider: msg.provider, model: msg.model }
          } catch {}
          sendModel(ws, current.agent).catch((error) => {
            send(ws, { type: 'error', code: 'model-failed', message: String(error?.message ?? error) })
          })
          break
        }
        case 'interrupt': {
          conn?.agent.cancel({ kind: 'user' })
          break
        }
        case 'ping': {
          send(ws, { type: 'pong' })
          break
        }
        default:
          break
      }
    })

    ws.on('close', () => { if (conn) detach(conn) })
    ws.on('error', () => {})
  })

  /** List live + persisted sessions with titles, newest first. */
  async function listSessions() {
    const query = ctx.get('sessionQuery')
    const persistence = ctx.get('sessionPersistence')
    if (!query || !persistence) return []
    const headers = await persistence.list()
    const live = new Set((ctx.get('agents')?.list() ?? []).map((a) => a.id))
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
    const conn = [...conns].find((c) => c.agent.id === req.agent.id)
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
          const conn = [...conns].find((c) => c.agent.id === payload.sessionId)
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
          const conn = [...conns].find((c) => c.agent.id === payload.sessionId)
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
