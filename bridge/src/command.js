// DSH human-command compatibility projection. DSH owns registration,
// agent-scoped shadowing, parsing, and execution; the bridge only projects
// handler-free descriptors and direct UI outcomes onto the TUI wire.

const COMMAND_NAME = /^[a-z][a-z0-9_-]*$/

/** Subscribe to DSH's unfiltered registry notification. Effective command
 * views are agent-scoped, so every live connection must be recomputed. */
export function watchCommandChanges(ctx, connections, refresh) {
  return ctx.on('commands/change', () => {
    for (const connection of connections) refresh(connection)
  })
}

/** Shape `ctx.commands.list(agent)` into a bounded, handler-free wire frame. */
export function shapeCommandsFrame(descriptors) {
  const commands = []
  for (const descriptor of Array.isArray(descriptors) ? descriptors : []) {
    if (!descriptor || typeof descriptor.name !== 'string' || !COMMAND_NAME.test(descriptor.name)) continue
    if (typeof descriptor.description !== 'string') continue
    const hint = descriptor.input?.hint
    commands.push({
      name: descriptor.name,
      description: descriptor.description,
      ...(typeof hint === 'string' && hint !== '' ? { input: { hint } } : {}),
    })
  }
  commands.sort((left, right) => left.name.localeCompare(right.name))
  return { type: 'commands', commands }
}

/** Shape the settled return from `commands.execute`; undefined is an
 * admission miss and is reported by the dispatcher as command-unknown. */
export function shapeCommandResultFrame(execution) {
  if (!execution || typeof execution.commandId !== 'string') return undefined
  const result = execution.result
  if (!result || (result.kind !== 'success' && result.kind !== 'error')) return undefined
  return {
    type: 'command-result',
    commandId: execution.commandId,
    kind: result.kind,
    ...(typeof result.text === 'string' ? { text: result.text } : {}),
  }
}
