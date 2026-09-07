// DSH model-selection adapter. This is the only bridge module that knows the
// upstream helper's package/export; session setup receives this narrow adapter
// rather than carrying a copied Cordis waterfall implementation.

/**
 * Compatibility conclusion recorded against the supported DSH release.
 * `@deepseek-ai/dsh-agent` exports this helper from its public package root;
 * its declaration is `(agentCtx, { current, assembled }) => disposer`.
 */
export const MODEL_SELECTION_UPSTREAM = Object.freeze({
  package: '@deepseek-ai/dsh-agent',
  export: 'installModelSelection',
  testedHost: '0.1.1-rc.2',
  testedPackage: '0.1.1-rc.2',
  signature: '(agentCtx, selection) => disposer',
})

/**
 * Resolve the published helper lazily. The source-tree unit tests deliberately
 * do not install a whole DSH host; deployed profile resolution supplies the
 * declared peer dependency before a session reaches setup.
 */
export async function loadOfficialModelSelection() {
  let upstream
  try {
    upstream = await import(MODEL_SELECTION_UPSTREAM.package)
  } catch (error) {
    throw new Error(
      `[dsh-tui] cannot load ${MODEL_SELECTION_UPSTREAM.package}: ${String(error?.message ?? error)}`,
    )
  }
  const install = upstream?.[MODEL_SELECTION_UPSTREAM.export]
  if (typeof install !== 'function') {
    throw new Error(
      `[dsh-tui] ${MODEL_SELECTION_UPSTREAM.package} does not export ${MODEL_SELECTION_UPSTREAM.export}`,
    )
  }
  return install
}

/**
 * Create the session-facing port. `loadInstaller` is injectable only to make
 * adapter contracts deterministic in source-tree tests; production always
 * loads the public DSH export above and never contains a local waterfall copy.
 */
export function createModelSelectionAdapter({ loadInstaller = loadOfficialModelSelection } = {}) {
  let installerPromise
  const installer = () => {
    installerPromise ??= Promise.resolve()
      .then(loadInstaller)
      .then((install) => {
        if (typeof install !== 'function') {
          throw new Error('[dsh-tui] model-selection installer must be a function')
        }
        return install
      })
    return installerPromise
  }

  return Object.freeze({
    defaultSelection(ctx) {
      const service = ctx?.get?.('agentDefaultModel')
      return typeof service?.currentSelection === 'function'
        ? service.currentSelection()
        : undefined
    },
    async install(agentCtx, selection) {
      return (await installer())(agentCtx, selection)
    },
  })
}
