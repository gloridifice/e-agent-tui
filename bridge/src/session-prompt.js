import { randomUUID } from 'node:crypto'

const IMAGE_MEDIA_TYPES = new Set(['image/png', 'image/jpeg', 'image/webp', 'image/gif'])

function unwrap(response) {
  const result = response?.result
  if (!result || typeof result !== 'object') {
    throw new Error('session prompt API returned an invalid response')
  }
  if (result.ok === true) return result.value
  const error = result.error
  throw new Error(`${error?.code ?? 'prompt-failed'}: ${error?.message ?? 'prompt admission failed'}`)
}

export function normalizePromptContent(content) {
  if (!Array.isArray(content)) throw new Error('prompt content must be an array')
  const normalized = content.map((part) => {
    if (part?.type === 'text' && typeof part.text === 'string') {
      return { type: 'text', text: part.text }
    }
    if (
      part?.type === 'image'
      && IMAGE_MEDIA_TYPES.has(part.mediaType)
      && typeof part.data === 'string'
      && part.data !== ''
      && (part.name === undefined || typeof part.name === 'string')
    ) {
      return {
        type: 'image',
        mediaType: part.mediaType,
        data: part.data,
        ...(part.name === undefined ? {} : { name: part.name }),
      }
    }
    throw new Error('prompt content contains an invalid part')
  })
  if (!normalized.some((part) => part.type === 'image' || part.text !== '')) {
    throw new Error('prompt content is empty')
  }
  return normalized
}

export function normalizeCommandImages(images) {
  if (images === undefined) return []
  if (!Array.isArray(images)) throw new Error('command images must be an array')
  return images.map((image) => {
    const [part] = normalizePromptContent([{ ...image, type: 'image' }])
    const { type: _type, ...encoded } = part
    return encoded
  })
}

export function createSessionPromptAdapter(apiProxy) {
  const service = () => (typeof apiProxy === 'function' ? apiProxy() : apiProxy)
  return Object.freeze({
    async prompt(sessionId, content, mode = 'queue') {
      const api = service()
      if (typeof api?.sessions?.prompt !== 'function') {
        throw new Error('session prompt API (apiProxy.sessions.prompt) is unavailable')
      }
      if (mode !== 'queue' && mode !== 'steer') throw new Error('invalid prompt mode')
      const normalized = normalizePromptContent(content)
      return unwrap(await api.sessions.prompt({
        rpcId: randomUUID(),
        payload: { sessionId, mode, content: normalized },
      }))
    },
  })
}
