// /model layer: shape the session model directory into the wire view the TUI
// renders. Pure (test/model.test.js) — the async session.models read lives in
// index.js/session-model.js, which query `apiProxy.sessions`.

function shapeReasoning(reasoning) {
  return {
    efforts: (reasoning?.efforts ?? []).map((effort) => ({
      id: effort.id,
      name: effort.name ?? effort.id,
      ...(effort.description !== undefined ? { description: effort.description } : {}),
    })),
    ...(reasoning?.defaultEffort !== undefined ? { defaultEffort: reasoning.defaultEffort } : {}),
  }
}

/**
 * Project the `session.models` provider groups and the current selection into
 * the `model` frame payload. Only the fields the TUI needs cross the wire; the
 * current selection (including its optional reasoning effort) rides along so
 * the picker and status bar can reflect it.
 * @param groups - `session.models` `groups` entries ({id, name, models[]}).
 * @param current - `{provider, model, reasoningEffort?}` selection, or undefined.
 */
export function shapeModelFrame(groups, current) {
  return {
    providers: (groups ?? []).map((provider) => ({
      id: provider.id,
      name: provider.name ?? provider.id,
      models: (provider.models ?? []).map((model) => ({
        id: model.id,
        name: model.name ?? model.id,
        ...(model.description !== undefined ? { description: model.description } : {}),
        ...(model.reasoning !== undefined ? { reasoning: shapeReasoning(model.reasoning) } : {}),
      })),
    })),
    ...(current !== undefined ? { current } : {}),
  }
}
