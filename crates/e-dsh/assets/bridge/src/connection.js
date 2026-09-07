const WS_CONNECTING = 0
const WS_OPEN = 1

/** Testable owner of connection cleanup and stale-operation checks. */
export class ConnectionRegistry extends Set {
  isCurrent(current, actual) {
    return current === actual && this.has(current)
  }

  findAgent(agentId) {
    return [...this].find((conn) => conn.agent.id === agentId)
  }

  detach(conn, { keepSocket = false } = {}) {
    if (!this.delete(conn)) return false
    conn.off()
    conn.abort.abort()
    for (const done of conn.pending.values()) done('cancelled')
    conn.pending.clear()
    if (!keepSocket && (conn.ws.readyState === WS_OPEN || conn.ws.readyState === WS_CONNECTING)) {
      conn.ws.close(1000)
    }
    return true
  }
}
