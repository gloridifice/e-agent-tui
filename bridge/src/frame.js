import { Buffer } from 'node:buffer'
import { latestTitle, sessionPresetOf } from './compose.js'

/** Project the authoritative attached-session metadata into a welcome frame. */
export function shapeWelcomeFrame(agent, protocolVersion, maxFrameBytes) {
  const events = agent.session?.events ?? []
  return {
    type: 'welcome',
    protocolVersion,
    maxFrameBytes,
    sessionId: agent.id,
    status: agent.status,
    provider: agent.options?.provider,
    model: agent.options?.model,
    // Creation records the initial preset in the frozen header; a later
    // blank-session recompose records agent-preset/selected in the log.
    mode: sessionPresetOf(agent.session?.header, events),
    title: latestTitle(events),
    cwd: agent.session?.header?.cwd,
  }
}

function encode(message) {
  return JSON.stringify(message)
}

function byteLength(wire) {
  return Buffer.byteLength(wire, 'utf8')
}

function oversizedError(maxBytes) {
  return {
    type: 'error',
    code: 'frame-too-large',
    message: `bridge frame exceeded ${maxBytes} bytes and was not sent`,
  }
}

/**
 * Encode one server frame without exceeding the canonical wire budget.
 * Snapshot/history frames retain the newest suffix and advertise that older
 * entries remain available; singular oversized frames become a small error.
 */
export function encodeBoundedFrame(message, maxBytes) {
  const wire = encode(message)
  if (byteLength(wire) <= maxBytes) return wire

  if (Array.isArray(message?.events)) {
    let low = 0
    let high = message.events.length
    let best = null
    while (low <= high) {
      const count = Math.floor((low + high) / 2)
      const candidate = {
        ...message,
        events: count === 0 ? [] : message.events.slice(-count),
        ...(message.type === 'snapshot' ? { truncated: true } : {}),
        ...(message.type === 'history' ? { hasMore: true } : {}),
      }
      const candidateWire = encode(candidate)
      if (byteLength(candidateWire) <= maxBytes) {
        best = candidateWire
        low = count + 1
      } else {
        high = count - 1
      }
    }
    if (best !== null) return best
  }

  return encode(oversizedError(maxBytes))
}
