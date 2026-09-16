// Each run snapshots the global route; ordinary agent requests are untouched.
export function createCompactionModels({ ctx, host, sessionModel, store }) {
  let selected = null
  const selectionNow = () => {
    const saved = store ? store.load() : selected
    return saved && saved.provider === selected?.provider && saved.model === selected?.model ? selected : saved
  }
  const save = value => {
    store?.save(value)
    selected = value
  }
  const active = new WeakMap()
  const labels = new WeakMap()

  ctx.on('session/event', (session, event) => {
    const id = event.data?.compactionId
    if (event.type === 'compaction/start') {
      const selection = selectionNow()
      active.set(session, { id, selection, start: event, name: selection?.name })
      if (selection) labels.set(event, selection.name)
    } else if (event.type === 'compaction/end') {
      const run = active.get(session)
      if (run?.id !== id) return
      if (run.name) labels.set(event, run.name)
      active.delete(session)
    }
  })
  ctx.on('llm/stream', (options, next) => {
    if (options.purpose !== 'compaction') return next()
    const session = host.agents()?.get(options.sessionId)?.session
    if (!session) return next()
    const run = active.get(session)
    const selection = run ? run.selection : selectionNow()
    if (selection) {
      if (Object.isFrozen(options)) throw new Error('Cannot route an immutable compaction request')
      options.provider = selection.provider
      options.model = selection.model
    }
    if (run) {
      run.name = selection?.name ?? options.model
      labels.set(run.start, run.name)
    }
    return next()
  })

  return {
    async configure(agent, args, isCurrent) {
      const [operation, reference, extra] = args.trim().split(/\s+/)
      if (operation === 'unset-model' && reference === undefined) {
        if (isCurrent()) save(null)
        return 'Global compaction model unset'
      }
      if (operation !== 'set-model' || !reference || extra !== undefined) {
        throw new Error('Usage: /compact set-model <provider/model> or /compact unset-model')
      }
      const { groups } = await sessionModel.catalogModels()
      const routes = (groups ?? []).flatMap(provider => (provider.models ?? []).map(model => ({
        provider: provider.id, model: model.id, name: model.name ?? model.id,
      })))
      const canonical = routes.filter(route => `${route.provider}/${route.model}`.toLowerCase() === reference.toLowerCase())
      const matches = canonical.length ? canonical : routes.filter(route => route.model.toLowerCase() === reference.toLowerCase())
      if (matches.length !== 1) throw new Error(`Unknown or ambiguous compaction model: ${reference}`)
      if (isCurrent()) save(matches[0])
      return `Global compaction model set to ${matches[0].name}`
    },
    project(event) {
      const modelName = labels.get(event)
      return modelName ? { ...event, data: { ...event.data, modelName } } : event
    },
  }
}
