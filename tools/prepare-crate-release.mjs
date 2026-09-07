import { spawnSync } from 'node:child_process'
import { existsSync, readdirSync, rmSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const cargo = process.platform === 'win32' ? 'cargo.exe' : 'cargo'

export function localRegistrySources(cargoHome, name, version) {
  const sourceRoot = join(cargoHome, 'registry', 'src')
  let registries
  try {
    registries = readdirSync(sourceRoot, { withFileTypes: true })
  } catch (error) {
    if (error.code === 'ENOENT') return []
    throw error
  }

  // Cargo's path-backed registries have an empty hostname followed by a hash.
  return registries
    .filter((entry) => entry.isDirectory() && /^-[0-9a-f]+$/.test(entry.name))
    .map((entry) => join(sourceRoot, entry.name, `${name}-${version}`))
    .filter((path) => existsSync(path))
}

function run(args, capture = false) {
  const result = spawnSync(cargo, args, {
    cwd: root,
    stdio: capture ? ['ignore', 'pipe', 'inherit'] : 'inherit',
    encoding: 'utf8',
    shell: false,
  })
  if (result.error) throw result.error
  if (result.status !== 0) throw new Error(`cargo ${args[0]} failed (${result.signal ?? result.status})`)
  return result.stdout
}

function prepare(name) {
  const metadata = JSON.parse(run(['metadata', '--no-deps', '--format-version', '1', '--locked'], true))
  const pkg = metadata.packages.find((pkg) => pkg.name === name && metadata.workspace_members.includes(pkg.id))
  if (!pkg) throw new Error(`Expected a workspace package name, got: ${name}`)

  const cargoHome = process.env.CARGO_HOME ? resolve(root, process.env.CARGO_HOME) : join(homedir(), '.cargo')
  const sources = localRegistrySources(cargoHome, pkg.name, pkg.version)
  if (sources.length === 0) return

  // Clean first: registry dependencies can remain "Fresh" even after their sources change.
  run(['clean', '--package', pkg.name, '--target-dir', metadata.target_directory])
  for (const source of sources) {
    console.log(`Removing stale release source cache: ${source}`)
    rmSync(source, { recursive: true, force: true })
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  try {
    prepare(process.argv[2])
  } catch (error) {
    console.error(error.message)
    process.exitCode = 1
  }
}
