import { latestTitle } from './compose.js'

/** Normalize the settled observation shape returned by dsh-session-query.
 * Current DSH returns `{status:'fulfilled', value:{session,title:{title}}}`;
 * the compatibility branches keep older direct snapshots readable. */
export function titleFromObservation(observation, fallbackSessionId) {
  const value = observation?.status === 'fulfilled'
    ? observation.value
    : observation?.status === undefined
      ? observation
      : undefined
  const sessionId = value?.session?.id ?? value?.sessionId ?? fallbackSessionId
  const rawTitle = value?.title
  const title = typeof rawTitle === 'string' ? rawTitle : rawTitle?.title
  return {
    sessionId,
    title: typeof title === 'string' ? title : undefined,
  }
}

/** Build a progressive session lister.
 *
 * `onPartial` receives persisted headers plus zero-I/O titles from live logs
 * as soon as `sessionPersistence.list()` settles. The returned promise folds
 * persisted titles only for the capped rows and yields the enriched list. */
export function createSessionLister(host, limit = 200) {
  return async function listSessions(onPartial) {
    const persistence = host.persistence()
    if (!persistence) return []

    const liveAgents = host.agents()?.list() ?? []
    const liveById = new Map(liveAgents.map((agent) => [agent.id, agent]))
    const headers = (await persistence.list())
      .slice()
      .sort((left, right) => (right.createdAt ?? 0) - (left.createdAt ?? 0))
      .slice(0, limit)

    const sessions = headers.map((header) => {
      const liveAgent = liveById.get(header.id)
      return {
        id: header.id,
        title: latestTitle(liveAgent?.session?.events) ?? '',
        live: liveAgent !== undefined,
        createdAt: header.createdAt ?? 0,
      }
    })

    const query = host.sessionQuery()
    const missing = sessions.filter((session) => session.title === '')
    if (!query || missing.length === 0) return sessions

    onPartial?.(sessions.map((session) => ({ ...session })))
    let observations
    try {
      observations = await query.readTitleSnapshots(missing.map((session) => session.id))
    } catch {
      return sessions
    }
    const byId = new Map()
    for (let index = 0; index < observations.length; index += 1) {
      const snapshot = titleFromObservation(observations[index], missing[index]?.id)
      if (snapshot.sessionId && snapshot.title) byId.set(snapshot.sessionId, snapshot.title)
    }
    return sessions.map((session) => ({
      ...session,
      title: session.title || byId.get(session.id) || '',
    }))
  }
}
