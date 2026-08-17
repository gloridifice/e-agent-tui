import { randomUUID } from 'node:crypto'
import {
  defaultModelSelection,
  installModelSelection,
  isExistingDirectory,
  sessionPresetOf,
} from './compose.js'

/**
 * Session creation/resume policy isolated from WebSocket dispatch. `attach`
 * and `detach` are injected effects, which makes workspace/preset composition
 * testable without opening a socket.
 */
export function createSessionService({
  host,
  ctx,
  modelSelections,
  attach,
  detach,
  isCurrent = () => true,
  warn = console.warn,
}) {
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
    const mirror = current?.options?.provider !== undefined && current?.options?.model !== undefined
      ? { provider: current.options.provider, model: current.options.model }
      : undefined
    const selection = { current: mirror ?? defaultModelSelection(ctx), assembled: undefined }
    const { agent } = await agents.create({
      sessionId: `session-${randomUUID()}`,
      meta: { cwd, ...(preset ? { agentPreset: preset.id } : {}) },
      agentOptions,
      setup: async (agentCtx) => {
        installModelSelection(agentCtx, selection)
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
      const selection = { current: defaultModelSelection(ctx), assembled: undefined }
      const { agent } = await agents.resume({
        resumeSessionId: sessionId,
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

  return Object.freeze({ createNewSession, resumePersistedSession })
}
