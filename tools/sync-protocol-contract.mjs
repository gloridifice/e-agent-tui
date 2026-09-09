import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const contractPath = join(root, 'bridge', 'protocol-contract.json')
const packagePath = join(root, 'bridge', 'package.json')
const docsPath = join(root, 'docs', 'protocol.md')
const rustFixturesPath = join(root, 'crates', 'e-dsh', 'testdata', 'wire-contract-fixtures.json')
const nodeFixturesPath = join(root, 'bridge', 'test', 'fixtures', 'wire-contract-fixtures.json')

const own = (object, key) => Object.prototype.hasOwnProperty.call(object, key)
const sorted = (values) => [...values].sort()

function assertObject(value, path) {
  if (value === null || Array.isArray(value) || typeof value !== 'object') {
    throw new Error(`${path} must be an object`)
  }
  return value
}

function validateShape(shape, path, knownTypes) {
  assertObject(shape, path)
  const required = assertObject(shape.required, `${path}.required`)
  const optional = assertObject(shape.optional, `${path}.optional`)
  for (const field of Object.keys(required)) {
    if (own(optional, field)) throw new Error(`${path}.${field} is both required and optional`)
  }
  for (const [presence, fields] of [['required', required], ['optional', optional]]) {
    for (const [field, type] of Object.entries(fields)) {
      if (field === 'type') throw new Error(`${path}.${presence} must not redeclare discriminator type`)
      if (typeof type !== 'string') throw new Error(`${path}.${presence}.${field} type must be a string`)
      const element = type.endsWith('[]') ? type.slice(0, -2) : type
      if (!knownTypes.has(element)) throw new Error(`${path}.${presence}.${field} has unknown type ${type}`)
    }
  }
}

export function validateContract(contract) {
  assertObject(contract, 'contract')
  if (!Number.isSafeInteger(contract.protocolVersion) || contract.protocolVersion < 1) {
    throw new Error('protocolVersion must be a positive integer')
  }
  for (const [name, value] of Object.entries(assertObject(contract.limits, 'limits'))) {
    if (!Number.isSafeInteger(value) || value < 1) throw new Error(`limits.${name} must be a positive integer`)
  }
  for (const roster of ['surfaceEvents', 'clientMessages', 'serverMessages']) {
    if (!Array.isArray(contract[roster]) || contract[roster].some((item) => typeof item !== 'string')) {
      throw new Error(`${roster} must be an array of strings`)
    }
    if (new Set(contract[roster]).size !== contract[roster].length) {
      throw new Error(`${roster} contains duplicates`)
    }
  }

  const records = assertObject(contract.records, 'records')
  const primitives = new Set(contract.shapeTypes?.primitives ?? [])
  const knownTypes = new Set([...primitives, ...Object.keys(records)])
  for (const [name, shape] of Object.entries(records)) {
    validateShape(shape, `records.${name}`, knownTypes)
  }
  for (const [event, shape] of Object.entries(contract.hostEventDataExtensions ?? {})) {
    if (!contract.surfaceEvents.includes(event)) throw new Error(`Unknown extended host event: ${event}`)
    validateShape(shape, `hostEventDataExtensions.${event}`, knownTypes)
  }
  const messageShapes = assertObject(contract.messageShapes, 'messageShapes')
  for (const [direction, rosterName] of [['client', 'clientMessages'], ['server', 'serverMessages']]) {
    const shapes = assertObject(messageShapes[direction], `messageShapes.${direction}`)
    const actual = Object.keys(shapes)
    const expected = contract[rosterName]
    if (JSON.stringify(actual) !== JSON.stringify(expected)) {
      throw new Error(`messageShapes.${direction} keys must match ${rosterName} in roster order; expected ${expected.join(', ')}, got ${actual.join(', ')}`)
    }
    for (const [type, shape] of Object.entries(shapes)) {
      validateShape(shape, `messageShapes.${direction}.${type}`, knownTypes)
    }
  }
  return contract
}

const stringSamples = {
  token: 'contract-token',
  text: 'sample text',
  line: '/help',
  sessionId: 'session-1',
  resumeSessionId: 'session-1',
  rpcId: 'rpc-1',
  questionRpcId: 'rpc-1',
  id: 'id-1',
  provider: 'provider-1',
  model: 'model-1',
  baseUrl: 'https://example.invalid/v1',
  apiKey: 'secret',
  value: 'secret',
  protocol: 'openai-completions',
  status: 'idle',
  kind: 'success',
  outcome: 'answered',
  code: 'sample-error',
  message: 'sample message',
  cwd: 'C:\\workspace',
  mode: 'standard',
  title: 'Sample session',
  name: 'sample',
  description: 'sample description',
  question: 'Choose one?',
  label: 'Option A',
  hint: '<text>',
  toolName: 'read',
  reason: 'sample reason',
  callId: 'call-1',
  apiKeySource: 'credentials',
  apiKeyHint: '…1234',
}

const integerSamples = {
  protocolVersion: undefined,
  maxFrameBytes: undefined,
  beforeSeq: 10,
  limit: 20,
  createdAt: 1,
  order: 1,
}

function sampleValue(type, field, contract, includeOptional, stack = []) {
  if (type.endsWith('[]')) {
    return [sampleValue(type.slice(0, -2), field, contract, includeOptional, stack)]
  }
  if (type === 'string') return stringSamples[field] ?? `${field}-sample`
  if (type === 'boolean') return true
  if (type === 'integer') {
    if (field === 'protocolVersion') return contract.protocolVersion
    if (field === 'maxFrameBytes') return contract.limits.maxFrameBytes
    return integerSamples[field] ?? 1
  }
  if (type === 'host-event') {
    return { seq: 1, time: 1, type: 'user/message', data: { content: [{ type: 'text', text: 'sample' }] } }
  }
  if (stack.includes(type)) throw new Error(`recursive record sample: ${[...stack, type].join(' -> ')}`)
  return sampleShape(contract.records[type], contract, includeOptional, [...stack, type])
}

function sampleShape(shape, contract, includeOptional, stack = []) {
  const sample = {}
  for (const [field, type] of Object.entries(shape.required)) {
    sample[field] = sampleValue(type, field, contract, includeOptional, stack)
  }
  if (includeOptional) {
    for (const [field, type] of Object.entries(shape.optional)) {
      sample[field] = sampleValue(type, field, contract, includeOptional, stack)
    }
  }
  return sample
}

export function buildFixtures(contract) {
  const direction = (shapes) => Object.fromEntries(Object.entries(shapes).map(([type, shape]) => [
    type,
    {
      minimal: { type, ...sampleShape(shape, contract, false) },
      full: { type, ...sampleShape(shape, contract, true) },
    },
  ]))
  return {
    generatedFrom: 'bridge/protocol-contract.json',
    protocolVersion: contract.protocolVersion,
    hostEventExtensions: Object.fromEntries(Object.entries(contract.hostEventDataExtensions ?? {}).map(([type, shape]) => [type, {
      minimal: { type, data: sampleShape(shape, contract, false) },
      full: { type, data: sampleShape(shape, contract, true) },
    }])),
    client: direction(contract.messageShapes.client),
    server: direction(contract.messageShapes.server),
  }
}

const escapeCell = (value) => String(value).replaceAll('|', '\\|')

function shapeTable(shape) {
  const rows = []
  for (const [field, type] of Object.entries(shape.required)) rows.push(`| required | \`${escapeCell(field)}\` | \`${escapeCell(type)}\` |`)
  for (const [field, type] of Object.entries(shape.optional)) rows.push(`| optional | \`${escapeCell(field)}\` | \`${escapeCell(type)}\` |`)
  if (rows.length === 0) rows.push('| — | _(no payload fields)_ | — |')
  return `| Presence | Field | Type |\n|---|---|---|\n${rows.join('\n')}`
}

function shapeSections(shapes) {
  return Object.entries(shapes)
    .map(([type, shape]) => `### \`${type}\`\n\n${shapeTable(shape)}`)
    .join('\n\n')
}

function buildDocs(contract) {
  const bullets = (items) => items.map((item) => `- \`${item}\``).join('\n')
  const records = Object.entries(contract.records)
    .map(([name, shape]) => `### \`${name}\`\n\n${shapeTable(shape)}`)
    .join('\n\n')
  return `<!-- Generated by tools/sync-protocol-contract.mjs from bridge/protocol-contract.json. -->
# DSH TUI Wire Protocol

Canonical machine-readable source: [\`bridge/protocol-contract.json\`](../bridge/protocol-contract.json).
Do not edit capacities, rosters, or payload fields here by hand.

## Version and limits

| Field | Value |
|---|---:|
| Protocol version | ${contract.protocolVersion} |
| Snapshot surface events | ${contract.limits.snapshotEvents} |
| History page events | ${contract.limits.historyEvents} |
| Maximum normal frame | ${contract.limits.maxFrameBytes} bytes |
| Client replay guard | ${contract.limits.clientReplayEvents} events |

## Client → bridge messages

${bullets(contract.clientMessages)}

${shapeSections(contract.messageShapes.client)}

## Bridge → client messages

${bullets(contract.serverMessages)}

${shapeSections(contract.messageShapes.server)}

## Payload records

Array types use the \`[]\` suffix. \`host-event\` is the bounded typed event envelope described by the snapshot roster and client parser.

${records}

## Host event data extensions

The bridge may add these optional presentation fields to host event data. They do not alter the durable host event. Missing model metadata must not be inferred from current preferences.

${shapeSections(contract.hostEventDataExtensions ?? {})}

## Snapshot surface events

${bullets(contract.surfaceEvents)}

The client sends \`hello.protocolVersion\`; \`welcome\` replies with optional
\`protocolVersion\`, \`maxFrameBytes\`, and the attached session's authoritative
\`mode\`. Older peers may omit optional fields. The \`list-sessions\` response is
progressive: the bridge may first send \`sessions{sessions,titlesPending:true}\`,
then send a final \`sessions{sessions}\`. Clients preserve selection by session
id across the refresh. A legacy bridge that emits larger frames requires an
explicit \`DSHE_LEGACY_MAX_FRAME_MB\` client override.
`
}

function outputMap(contract) {
  const fixtures = `${JSON.stringify(buildFixtures(contract), null, 2)}\n`
  const packageJson = JSON.parse(readFileSync(packagePath, 'utf8'))
  packageJson.dshCompatibility ??= {}
  packageJson.dshCompatibility.wireProtocol = contract.protocolVersion
  return new Map([
    [docsPath, buildDocs(contract)],
    [rustFixturesPath, fixtures],
    [nodeFixturesPath, fixtures],
    [packagePath, `${JSON.stringify(packageJson, null, 2)}\n`],
  ])
}

export function syncProtocolContract({ check = false } = {}) {
  const contract = validateContract(JSON.parse(readFileSync(contractPath, 'utf8')))
  const stale = []
  for (const [path, expected] of outputMap(contract)) {
    let actual
    try { actual = readFileSync(path, 'utf8') } catch { actual = undefined }
    if (actual === expected) continue
    if (check) stale.push(path.slice(root.length + 1))
    else writeFileSync(path, expected, 'utf8')
  }
  if (stale.length > 0) {
    throw new Error(`generated protocol files are stale:\n${sorted(stale).map((path) => `- ${path}`).join('\n')}\nRun: node tools/sync-protocol-contract.mjs`)
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  try {
    syncProtocolContract({ check: process.argv.includes('--check') })
  } catch (error) {
    console.error(error.message)
    process.exitCode = 1
  }
}
