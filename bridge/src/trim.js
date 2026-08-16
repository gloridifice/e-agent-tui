// Payload trimming (pure, no imports) — extracted from index.js so the
// wire-size logic has its own module and tests (node --test test/trim.test.js).
//
// tool/result payloads are the bulk of a session log. The TUI only needs
// the toolCallId plus the tail of the output (exit marker, line count).
// Reads (file contents) are not displayed at all — strip them entirely.
export const TOOL_RESULT_TAIL_CHARS = 2000
const READ_TOOLS = new Set(['read', 'read_text', 'read_image'])

/**
 * Trim one event's tool/result payload. Returns the same object when
 * nothing changed (no clone), a shallow clone otherwise.
 */
export function trimToolResultEvent(e, toolNames) {
  if (e?.type !== 'tool/result') return e
  const blocks = e.data?.message?.content
  if (!Array.isArray(blocks) || blocks.length === 0) return e
  const head = blocks[0]
  if (!head || !Array.isArray(head.content)) return e
  const toolName = toolNames.get(head.toolCallId)
  const tail = READ_TOOLS.has(toolName) ? 0 : TOOL_RESULT_TAIL_CHARS
  let changed = false
  const inner = head.content.map((b) => {
    if (b?.type === 'text' && typeof b.text === 'string' && b.text.length > tail) {
      changed = true
      return { ...b, text: tail > 0 ? b.text.slice(-tail) : '' }
    }
    return b
  })
  if (!changed) return e
  return {
    ...e,
    data: {
      ...e.data,
      message: { ...e.data.message, content: [{ ...head, content: inner }] },
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
