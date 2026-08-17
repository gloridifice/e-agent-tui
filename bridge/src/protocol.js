// Canonical wire-contract loader. The JSON file is shipped with the bridge and
// is also consumed by client/build.rs, so capacities and protocol versions have
// one owner across the Node and Rust processes.
import { readFileSync } from 'node:fs'

const contractUrl = new URL('../protocol-contract.json', import.meta.url)
export const WIRE_CONTRACT = Object.freeze(JSON.parse(readFileSync(contractUrl, 'utf8')))
export const PROTOCOL_VERSION = WIRE_CONTRACT.protocolVersion
export const SNAPSHOT_CAP = WIRE_CONTRACT.limits.snapshotEvents
export const HISTORY_CAP = WIRE_CONTRACT.limits.historyEvents
export const MAX_FRAME_BYTES = WIRE_CONTRACT.limits.maxFrameBytes
export const SNAPSHOT_SURFACE = new Set(WIRE_CONTRACT.surfaceEvents)

export function supportsClientMessage(type) {
  return WIRE_CONTRACT.clientMessages.includes(type)
}
