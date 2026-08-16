/**
 * probe-bridge.mjs — dry-run the bridge plugin's apply() against a mock
 * Cordis context. Verifies the plugin body loads and registers the upgrade
 * route, and generates the real token file under $DSH_HOME.
 */
import { pathToFileURL } from 'node:url'
import { join } from 'node:path'

const bridgeEntry = join(
  process.env.DSH_HOME ?? 'C:\\Users\\11659\\.dsh',
  'profiles', 'web', 'packages', 'dsh-tui-bridge', 'src', 'index.js',
)
const { apply } = await import(pathToFileURL(bridgeEntry).toString())

const registered = []
const ctx = {
  webServer: {
    registerUpgrade(route) {
      registered.push(route)
      return () => {}
    },
  },
  get() { return undefined },
  on() { return () => {} },
  effect(fn) { fn(); return () => {} },
}

apply(ctx, {})
console.log('apply OK')
console.log('registered routes:', registered.map((r) => r.path).join(', '))
