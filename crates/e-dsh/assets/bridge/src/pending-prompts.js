function preview(message) {
  return (message.content ?? []).map((part) => part.type === 'text' ? part.text : '[Image]').join('\n').slice(0, 4096)
}

export function createPendingPrompts({ send, isCurrent }) {
  const busy = new WeakSet()
  const tails = new WeakMap()

  function publish(conn, operation, error) {
    if (!isCurrent(conn)) return
    send(conn.ws, {
      type: 'asap-queue', sessionId: conn.agent.id,
      prompts: (conn.agent.inbox?.nextStep ?? []).map(preview),
      ...(operation === undefined ? {} : { operation }),
      ...(error === undefined ? {} : { error: String(error?.message ?? error) }),
    })
  }

  function run(conn, operation, action) {
    const next = (tails.get(conn) ?? Promise.resolve()).then(async () => {
      if (!isCurrent(conn)) return
      busy.add(conn)
      let failure
      try { await action() } catch (error) { failure = error }
      finally { busy.delete(conn) }
      publish(conn, operation, failure)
    })
    tails.set(conn, next.catch(() => {}))
    return next
  }

  return {
    submit: (conn, action) => run(conn, 'submit', action),
    clear: (conn) => run(conn, 'clear', () => {
      const inbox = conn.agent.inbox
      if (!inbox || typeof inbox.splice !== 'function') throw new Error('DSH inbox cancellation is unavailable')
      inbox.splice('next-step', 0, inbox.nextStep.length, [])
    }),
    watch(ctx, conn) {
      const off = ['inserted', 'claimed', 'discarded'].map((kind) =>
        ctx.on(`agent/inbox/${kind}`, ({ agent }) => {
          if (agent.id === conn.agent.id && !busy.has(conn)) publish(conn)
        }))
      publish(conn)
      return () => off.forEach((dispose) => dispose())
    },
  }
}
