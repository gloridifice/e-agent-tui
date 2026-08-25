import { randomUUID } from 'node:crypto'
import { isExistingDirectory, sessionPresetOf } from './compose.js'

/**
 * Session creation/resume policy isolated from WebSocket dispatch. `attach`
 * and `detach` are injected effects, which makes workspace/preset composition
 * testable without opening a socket.
 */
export function createSessionService({
  host,
  ctx,
  modelSelections,
  modelSelection,
  sessionModel,
  attach,
  detach,
  isCurrent = () => true,
  warn = console.warn,
}) {
  if (typeof modelSelection?.defaultSelection !== 'function'
    || typeof modelSelection?.install !== 'function') {
    throw new Error('model-selection adapter is required')
  }

  async function resolvePreset(agentPresets, wanted, fallbackStandard) {
    if (!agentPresets) return undefined
    const attempts = fallbackStandard ? [wanted, 'standard', undefined] : [wanted]
    let lastError
    for (const candidate of attempts) {
      try {
        return await agentPresets.resolve(candidate)
      } catch (error) {
        lastError = error
      }
    }
    const available = Array.isArray(lastError?.available) && lastError.available.length > 0
      ? `（可用: ${lastError.available.join(', ')}）`
      : ''
    throw new Error(`agent-presets: ${String(lastError?.message ?? lastError)}${available}`)
  }

  async function claimWorkspace(agent, cwd) {
    const registry = host.workspaces()
    if (!registry) return
    try {
      let workspace
      try { workspace = await registry.resolveByPath(cwd) } catch { workspace = undefined }
      if (workspace === undefined) workspace = await registry.create(cwd)
      await workspace.attachSession(agent.id)
    } catch (error) {
      warn(`[dsh-tui] /new: session ${agent.id} created but could not join the workspace at ${cwd}: ${String(error?.message ?? error)}`)
    }
  }

  async function createNewSession(ws, conn, mode, opts = {}) {
    const agents = host.agents()
    if (!agents) throw new Error('agents service unavailable')
    const current = conn?.agent
    const clientCwd = conn?.clientCwd ?? opts.clientCwd
    const inheritedCwd = current?.session?.header?.cwd ?? process.cwd()
    const cwd = isExistingDirectory(clientCwd) ? clientCwd : inheritedCwd
    const agentPresets = host.presets()
    const wanted = mode
      ?? agentPresets?.composedPreset(current?.ctx)
      ?? current?.session?.header?.agentPreset
    const preset = await resolvePreset(agentPresets, wanted, opts.fallbackStandard)
    const agentOptions = {}
    if (current?.options?.provider) agentOptions.provider = current.options.provider
    if (current?.options?.model) agentOptions.model = current.options.model
    // Inherit the previous session's complete selection (provider + model +
    // reasoningEffort) so a deferred /new carries the effort through. The
    // bridge mirror is authoritative for bridge-created sessions; a
    // host-created live session has no mirror, so hydrate it from the
    // authoritative per-session selection instead of the provider/model-only
    // `agent.options` fallback, which would silently drop the effort.
    const inherited = current?.id !== undefined ? modelSelections.get(current.id)?.current : undefined
    let mirror = inherited
      ?? (current?.options?.provider !== undefined && current?.options?.model !== undefined
        ? { provider: current.options.provider, model: current.options.model }
        : undefined)
    if (inherited === undefined && mirror !== undefined && current?.id !== undefined && sessionModel) {
      try {
        const models = await sessionModel.models(current.id)
        if (models?.current !== undefined) mirror = { ...models.current }
      } catch {}
    }
    const selection = {
      current: mirror ?? modelSelection.defaultSelection(ctx),
      assembled: undefined,
    }
    const { agent } = await agents.create({
      sessionId: `session-${randomUUID()}`,
      meta: { cwd, ...(preset ? { agentPreset: preset.id } : {}) },
      agentOptions,
      setup: async (agentCtx) => {
        await modelSelection.install(agentCtx, selection)
        if (preset) await agentPresets.mount(agentCtx, preset.id)
      },
    })
    modelSelections.set(agent.id, selection)
    await claimWorkspace(agent, cwd)
    // The request may have lost a race to a later attach/new while creation
    // awaited host services. Never detach or overwrite the newer connection.
    if (conn && !isCurrent(conn)) return undefined
    if (conn) detach(conn, { keepSocket: true })
    return attach(ws, agent, clientCwd)
  }

  async function resumePersistedSession(sessionId) {
    const agents = host.agents()
    const persistence = host.persistence()
    const agentPresets = host.presets()
    if (!agents || !persistence) return undefined
    try {
      const headers = await persistence.list()
      if (!headers.some((header) => header.id === sessionId)) return undefined
      const inspected = await persistence.inspect(sessionId)
      let preset
      if (agentPresets) {
        try {
          preset = await agentPresets.resolve(sessionPresetOf(inspected.meta, inspected.events))
        } catch {
          return undefined
        }
      }
      const selection = {
        current: modelSelection.defaultSelection(ctx),
        assembled: undefined,
      }
      const { agent } = await agents.resume({
        resumeSessionId: sessionId,
        setup: async (agentCtx) => {
          await modelSelection.install(agentCtx, selection)
          if (preset) await agentPresets.mount(agentCtx, preset.id)
        },
      })
      // A resumed session's authoritative selection lives in its log, not in
      // the deployment default. Hydrate it so the status bar and next prompt
      // use the session's own provider/model/reasoningEffort triple.
      if (sessionModel) {
        try {
          const models = await sessionModel.models(sessionId)
          if (models?.current !== undefined) selection.current = { ...models.current }
        } catch {}
      }
      modelSelections.set(agent.id, selection)
      return agent
    } catch {
      return undefined
    }
  }

  return Object.freeze({ createNewSession, resumePersistedSession })
}
