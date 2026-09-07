import { latestTitle } from './compose.js'

const CLASSIFY_BATCH_SIZE = 16

/** DSH's canonical conversation-start boundary: setup/state events do not make
 * a session historical; the first model-loop execution does. */
export function sessionIsBlank(events) {
  return !Array.isArray(events) || !events.some((event) => event?.type === 'turn/start')
}

function projectedBlank(snapshot) {
  const blank = snapshot?.values?.sessionListMetadata?.blank
  return typeof blank === 'boolean' ? blank : undefined
}

/** Resolve blankness with live memory first, then the persisted projection
 * ladder, and finally a typed log read. Operational failures fail open so a
 * real conversation is never hidden by an unavailable optimization. */
async function sessionIsBlankFor(host, persistence, header, liveAgent) {
  if (liveAgent !== undefined) return sessionIsBlank(liveAgent.session?.events)
  try {
    const cache = typeof host.sessionProjectionCache === 'function'
      ? host.sessionProjectionCache()
      : undefined
    const cached = cache?.cachedSnapshot?.(header)
    if (projectedBlank(cached) === false) return false
    const refreshed = cache?.coldSnapshot
      ? await cache.coldSnapshot(header.id)
      : undefined
    const projected = projectedBlank(refreshed ?? cached)
    if (projected !== undefined) return projected
  } catch {
    // A projection is a fold shortcut, never the authority. Fall through to
    // the typed persistence source before using the fail-open policy.
  }
  try {
    const { events } = await persistence.readFrom(header.id, 0)
    return sessionIsBlank(events)
  } catch {
    return false
  }
}

async function visibleHeaders(host, persistence, liveById, limit) {
  const byId = new Map((await persistence.list()).map((header) => [header.id, header]))
  for (const agent of liveById.values()) {
    if (!byId.has(agent.id) && agent.session?.header) byId.set(agent.id, agent.session.header)
  }
  const ordered = [...byId.values()]
    .sort((left, right) => (right.createdAt ?? 0) - (left.createdAt ?? 0))
  const visible = []
  for (let offset = 0; offset < ordered.length && visible.length < limit; offset += CLASSIFY_BATCH_SIZE) {
    const batch = ordered.slice(offset, offset + CLASSIFY_BATCH_SIZE)
    const blank = await Promise.all(batch.map((header) =>
      sessionIsBlankFor(host, persistence, header, liveById.get(header.id))))
    for (let index = 0; index < batch.length && visible.length < limit; index += 1) {
      if (!blank[index]) visible.push(batch[index])
    }
  }
  return visible
}

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
    // Eligibility precedes the cap: newer setup-only sessions must never crowd
    // older real conversations out of the picker.
    const headers = await visibleHeaders(host, persistence, liveById, limit)

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
