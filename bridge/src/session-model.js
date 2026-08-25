// DSH session model-selection seam: the bridge's single adapter onto the Host's
// `session.models` / `session.selectModel` contract (reached through the
// injected `apiProxy.sessions`). The apiProxy accessor is resolved lazily per
// call because the service can appear after bridge composition, mirroring
// question.js's lazy resolution rather than an eager `ctx.get`.

import { randomUUID } from 'node:crypto'

function rpcRequest(payload) {
  return { rpcId: randomUUID(), payload }
}

/** Unwrap `{ rpcId, result: { ok, value|error } }`; throw a code-prefixed error
 * so the caller can surface `model-failed` without guessing the seam. */
function unwrap(response) {
  const result = response?.result
  if (!result || typeof result !== 'object') {
    throw new Error('session model API returned an invalid response')
  }
  if (result.ok === true) return result.value
  const error = result.error
  throw new Error(`${error?.code ?? 'model-failed'}: ${error?.message ?? 'model operation failed'}`)
}

export function createSessionModelAdapter(apiProxy) {
  const service = () => (typeof apiProxy === 'function' ? apiProxy() : apiProxy)
  const sessions = () => {
    const api = service()
    if (!api?.sessions) throw new Error('session model API (apiProxy.sessions) is unavailable')
    return api.sessions
  }
  const llm = () => {
    const api = service()
    if (!api?.llm) throw new Error('model catalog API (apiProxy.llm) is unavailable')
    return api.llm
  }
  return Object.freeze({
    /** Read the authoritative current selection plus the advisory catalog for
     * one ordinary session. Returns `{ current, routable, groups, failures }`. */
    async models(sessionId) {
      return unwrap(await sessions().models(rpcRequest({ sessionId })))
    },
    /** Select the complete provider/model/reasoningEffort triple. Returns the
     * resolved `{ selected }` on success and throws on rejection. */
    async selectModel({ sessionId, provider, model, reasoningEffort }) {
      const payload = {
        sessionId,
        provider,
        model,
        ...(reasoningEffort === undefined ? {} : { reasoningEffort }),
      }
      return unwrap(await sessions().selectModel(rpcRequest(payload)))
    },
    /** Read the host-scoped advisory catalog (the same groups as
     * `session.models`, without a per-session selection). Deliberately does NOT
     * go through `session.models` so reading the picker does not install the
     * Host's lazy `selectionFor` waterfall over the bridge's own. Returns
     * `{ groups, failures }`. */
    async catalogModels() {
      return unwrap(await llm().models(rpcRequest({})))
    },
  })
}
