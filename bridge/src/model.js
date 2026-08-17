// /model layer: shape the provider/model catalog into the wire view the TUI
// renders. Pure (test/model.test.js) — the async listing stays in index.js,
// which queries `ctx.llm.listProviders()` + `ctx.llm.listModels(provider)`.

/**
 * Project providers and their models into the `model` frame payload. Only
 * the fields the TUI needs cross the wire; the current selection rides along
 * so the picker can highlight it.
 * @param providers - `ctx.llm.listProviders()` entries ({id, name}).
 * @param modelLists - providerId → `ctx.llm.listModels(provider)` entries.
 * @param current - {provider, model} selection, or undefined.
 */
export function shapeModelFrame(providers, modelLists, current) {
  return {
    providers: (providers ?? []).map((p) => ({
      id: p.id,
      name: p.name ?? p.id,
      models: (modelLists?.[p.id] ?? []).map((m) => ({
        id: m.id,
        name: m.name ?? m.id,
        ...(m.description !== undefined ? { description: m.description } : {}),
      })),
    })),
    ...(current !== undefined ? { current } : {}),
  }
}
