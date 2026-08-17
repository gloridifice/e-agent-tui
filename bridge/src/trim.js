// Bounded presentation payloads for snapshot and realtime event forwarding.
export const TOOL_RESULT_TAIL_CHARS = 2000
export const TOOL_META_MAX_CHARS = 8000
export const COMPACTION_SUMMARY_MAX_CHARS = 32000
const READ_TOOLS = new Set(['read', 'read_text', 'read_image'])

function trimTextBlocks(blocks, limit, tail = false) {
  if (!Array.isArray(blocks)) return { blocks, changed: false }
  let changed = false
  const output = blocks.map((block) => {
    if (block?.type !== 'text' || typeof block.text !== 'string' || block.text.length <= limit) return block
    changed = true
    return { ...block, text: limit === 0 ? '' : (tail ? block.text.slice(-limit) : block.text.slice(0, limit)) }
  })
  return { blocks: output, changed }
}

function boundedMeta(meta) {
  if (meta === undefined) return { meta, changed: false }
  let serialized
  try { serialized = JSON.stringify(meta) } catch { serialized = '' }
  if (serialized.length <= TOOL_META_MAX_CHARS) return { meta, changed: false }
  return {
    meta: { dshTuiTrimmed: true, preview: serialized.slice(-TOOL_META_MAX_CHARS) },
    changed: true,
  }
}

function boundedUnknownSurfaceEvent(event) {
  const surfaceOp = event.surfaceOp === 'append'
    ? 'append'
    : event.surfaceOp?.op === 'replace'
      ? {
          op: 'replace',
          ...(Number.isSafeInteger(event.surfaceOp.start) ? { start: event.surfaceOp.start } : {}),
          ...(Number.isSafeInteger(event.surfaceOp.end) ? { end: event.surfaceOp.end } : {}),
        }
      : { op: 'invalid' }
  return {
    type: typeof event.type === 'string' ? [...event.type].slice(0, 160).join('') : 'unknown',
    ...(Number.isSafeInteger(event.seq) ? { seq: event.seq } : {}),
    ...(Number.isSafeInteger(event.time) ? { time: event.time } : {}),
    surfaceOp,
    ...(Array.isArray(event.sourceEventSeqs)
      ? { sourceEventSeqs: event.sourceEventSeqs.filter(Number.isSafeInteger).slice(0, 256) }
      : {}),
    data: { dshTuiUnknownSurface: true },
  }
}

/**
 * Trim event payloads used by the terminal presentation. Returns the same
 * object when unchanged and a shallow clone when a bounded projection lands.
 */
export function trimToolResultEvent(e, toolNames, knownSurfaceTypes) {
  if (e?.surfaceOp !== undefined && knownSurfaceTypes && !knownSurfaceTypes.has(e.type)) {
    return boundedUnknownSurfaceEvent(e)
  }

  if (e?.type === 'tool/code-dispatch') {
    const projected = trimTextBlocks(e.data?.content, TOOL_RESULT_TAIL_CHARS, true)
    if (!projected.changed) return e
    return { ...e, data: { ...e.data, content: projected.blocks, dshTuiTrimmed: true } }
  }

  if (e?.type === 'compaction/summary') {
    const projected = trimTextBlocks(e.data?.summary, COMPACTION_SUMMARY_MAX_CHARS, false)
    if (!projected.changed) return e
    return { ...e, data: { ...e.data, summary: projected.blocks, dshTuiTrimmed: true } }
  }

  if (e?.type !== 'tool/result') return e
  const blocks = e.data?.message?.content
  if (!Array.isArray(blocks) || blocks.length === 0) return e
  const head = blocks[0]
  if (!head || !Array.isArray(head.content)) return e
  const toolName = toolNames.get(head.toolCallId)
  const tail = READ_TOOLS.has(toolName) ? 0 : TOOL_RESULT_TAIL_CHARS
  const inner = trimTextBlocks(head.content, tail, true)
  const meta = boundedMeta(e.data?.meta)
  if (!inner.changed && !meta.changed) return e
  return {
    ...e,
    data: {
      ...e.data,
      ...(meta.changed ? { meta: meta.meta } : {}),
      ...(inner.changed ? { dshTuiOutputTrimmed: true } : {}),
      dshTuiTrimmed: true,
      message: { ...e.data.message, content: [{ ...head, content: inner.blocks }] },
    },
  }
}

/** callId -> tool name, for payload-trim decisions. */
export function buildToolNames(events) {
  const names = new Map()
  for (const e of events) {
    if (e.type === 'tool/call' && e.data?.callId) names.set(e.data.callId, e.data?.name)
  }
  return names
}
