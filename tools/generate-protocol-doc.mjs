// Backward-compatible entry point. Protocol docs, fixtures, package metadata,
// and validation are now synchronized together from the canonical contract.
import { syncProtocolContract } from './sync-protocol-contract.mjs'

try {
  syncProtocolContract({ check: process.argv.includes('--check') })
} catch (error) {
  console.error(error.message)
  process.exitCode = 1
}
