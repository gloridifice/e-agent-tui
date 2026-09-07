import { randomUUID } from 'node:crypto'

// Explicit anti-corruption boundary around DSH's string-keyed service locator.
// Index/session code depends on this port rather than scattering undocumented
// ctx.get() knowledge through every handler.
export const REQUIRED_SERVICES = Object.freeze(['agents', 'sessionPersistence'])
export const OPTIONAL_SERVICES = Object.freeze([
  'agentPresets', 'workspaceRegistry', 'sessionQuery', 'sessionProjections',
  'sessionProjectionCache', 'commands', 'skills', 'llm', 'apiProxy',
  'agentDefaultModel', 'credentials', 'settings',
])

/** Open the host mux with the RpcRequest envelope required by ApiProxy. */
export function openApiProxyMux(apiProxy, signal, rpcId = randomUUID()) {
  if (typeof apiProxy?.events?.mux !== 'function') return null
  return apiProxy.events.mux({ rpcId, payload: {} }, signal)
}

export function createHostPort(ctx) {
  if (!ctx || typeof ctx.get !== 'function') throw new TypeError('DSH context must provide get(name)')
  const get = (name) => ctx.get(name)
  return Object.freeze({
    context: ctx,
    get,
    agents: () => get('agents'),
    persistence: () => get('sessionPersistence'),
    presets: () => get('agentPresets'),
    workspaces: () => get('workspaceRegistry'),
    sessionQuery: () => get('sessionQuery'),
    sessionProjections: () => get('sessionProjections'),
    sessionProjectionCache: () => get('sessionProjectionCache'),
    commands: () => get('commands'),
    skills: () => get('skills'),
    llm: () => get('llm'),
    apiProxy: () => get('apiProxy'),
    capabilities() {
      return Object.fromEntries([...REQUIRED_SERVICES, ...OPTIONAL_SERVICES].map((name) => [name, get(name) !== undefined]))
    },
    assertCore() {
      const missing = REQUIRED_SERVICES.filter((name) => get(name) === undefined)
      if (missing.length > 0) throw new Error(`DSH host missing required services: ${missing.join(', ')}`)
    },
  })
}
