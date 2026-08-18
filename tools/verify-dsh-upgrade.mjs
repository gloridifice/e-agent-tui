// verify-dsh-upgrade.mjs — required compatibility gate after a DSH upgrade.
// It checks canonical protocol artifacts, the declared public model-selection
// export in the deployed profile, then executes deployed helper and
// session-routing smokes.
import { spawnSync } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, join } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { syncProtocolContract } from './sync-protocol-contract.mjs'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const packageJson = JSON.parse(readFileSync(join(root, 'bridge', 'package.json'), 'utf8'))
const compatibility = packageJson.dshCompatibility?.modelSelection
const profile = process.env.DSH_TUI_SMOKE_PROFILE ?? 'web'
const home = process.env.DSH_HOME ?? join(process.env.USERPROFILE ?? '.', '.dsh')
const deployedAdapter = join(home, 'profiles', profile, 'packages', 'dsh-tui-bridge', 'src', 'model-selection.js')

function fail(message) {
  console.error(`verify-dsh-upgrade: ${message}`)
  process.exit(1)
}

try {
  syncProtocolContract({ check: true })
} catch (error) {
  fail(String(error?.message ?? error))
}

if (!compatibility?.package || !compatibility?.export || !compatibility?.testedPackage) {
  fail('bridge/package.json lacks dshCompatibility.modelSelection metadata')
}
if (!existsSync(deployedAdapter)) {
  fail(`deployed adapter not found at ${deployedAdapter}; run tools/mount-bridge.ps1 then dsh plugin --profile ${profile} install`)
}

const resolver = createRequire(deployedAdapter)
let hostPackage
let agentPackage
let upstream
try {
  hostPackage = JSON.parse(readFileSync(resolver.resolve('@deepseek-ai/dsh/package.json'), 'utf8'))
  agentPackage = JSON.parse(readFileSync(resolver.resolve(`${compatibility.package}/package.json`), 'utf8'))
  upstream = await import(pathToFileURL(resolver.resolve(compatibility.package)).href)
} catch (error) {
  fail(`cannot resolve the tested DSH host or ${compatibility.package} from deployed profile: ${String(error?.message ?? error)}`)
}
if (hostPackage.version !== packageJson.dshCompatibility.testedHost) {
  fail(`@deepseek-ai/dsh is ${hostPackage.version}; expected tested ${packageJson.dshCompatibility.testedHost}`)
}
if (agentPackage.version !== compatibility.testedPackage) {
  fail(`${compatibility.package} is ${agentPackage.version}; expected tested ${compatibility.testedPackage}`)
}
if (typeof upstream[compatibility.export] !== 'function') {
  fail(`${compatibility.package}@${agentPackage.version} does not export ${compatibility.export}`)
}

for (const script of ['smoke-bridge.mjs', 'test-new-swap.mjs']) {
  const smoke = spawnSync(process.execPath, [join(root, 'tools', script)], {
    cwd: root,
    env: { ...process.env, DSH_TUI_SMOKE_PROFILE: profile },
    stdio: 'inherit',
  })
  if (smoke.status !== 0) process.exit(smoke.status ?? 1)
}
console.log(`verify-dsh-upgrade: protocol + DSH@${hostPackage.version} + ${compatibility.package}@${agentPackage.version} + deployed ${profile} bridge/session routes OK`)
