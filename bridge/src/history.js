/** Session-surface cache and paging policy, independent of socket dispatch. */
export function createHistoryStore({
  host,
  surfaceTypes,
  trimEvent,
  buildToolNames,
  snapshotCap,
  maxSessions = 64,
}) {
  const surfaceState = new Map()
  const isSurfaceEvent = (event) => surfaceTypes.has(event.type) || event.surfaceOp !== undefined
  const isDetached = (conn) => conn.abort?.signal.aborted === true

  function surfaceFor(conn) {
    const live = conn.agent.session?.events
    if (!live) {
      if (!conn.surface) conn.surface = (conn.log ?? []).filter(isSurfaceEvent)
      return conn.surface
    }
    if (surfaceState.size > maxSessions) surfaceState.clear()
    let state = surfaceState.get(conn.agent.id)
    if (!state) {
      state = { list: [], lastSeq: -1 }
      for (const event of live) {
        const seq = event.seq ?? 0
        if (seq > state.lastSeq) state.lastSeq = seq
        if (isSurfaceEvent(event)) state.list.push(event)
      }
      surfaceState.set(conn.agent.id, state)
    } else {
      for (const event of live) {
        const seq = event.seq ?? 0
        if (seq > state.lastSeq) {
          state.lastSeq = seq
          if (isSurfaceEvent(event)) state.list.push(event)
        }
      }
    }
    return state.list
  }

  function page(conn, beforeSeq, limit) {
    const filtered = beforeSeq === undefined
      ? surfaceFor(conn)
      : surfaceFor(conn).filter((event) => (event.seq ?? 0) < beforeSeq)
    const tail = filtered.slice(-limit)
    return {
      events: tail.map((event) => trimEvent(event, conn.toolNames)),
      hasMore: filtered.length > tail.length,
    }
  }

  function sendSnapshot(conn, send) {
    if (conn.agent.session?.events) {
      const { events, hasMore } = page(conn, undefined, snapshotCap)
      send(conn.ws, { type: 'snapshot', events, truncated: hasMore })
      return
    }
    const persistence = host.persistence()
    if (!persistence) {
      send(conn.ws, { type: 'snapshot', events: [], truncated: false })
      return
    }
    persistence.readFrom(conn.agent.id, 0).then(
      ({ events }) => {
        if (isDetached(conn)) return
        conn.log = events
        conn.toolNames = buildToolNames(events)
        const { events: tail, hasMore } = page(conn, undefined, snapshotCap)
        if (!isDetached(conn)) send(conn.ws, { type: 'snapshot', events: tail, truncated: hasMore })
      },
      () => {
        if (!isDetached(conn)) send(conn.ws, { type: 'snapshot', events: [], truncated: false })
      },
    )
  }

  return Object.freeze({
    historyEvents: (conn, beforeSeq, limit) => page(conn, beforeSeq, limit),
    sendSnapshot,
    surfaceFor,
  })
}
