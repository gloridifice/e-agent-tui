import test from 'node:test'

import { syncReleaseAssets } from '../../tools/sync-release-assets.mjs'

test('packaged Rust bridge assets match production sources', () => {
  syncReleaseAssets({ check: true })
})
