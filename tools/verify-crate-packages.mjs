import { spawnSync } from 'node:child_process'
import { copyFileSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { basename, dirname, join } from 'node:path'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const allowDirty = process.argv.includes('--allow-dirty')
const cargo = process.platform === 'win32' ? 'cargo.exe' : 'cargo'

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: root,
    stdio: 'inherit',
    shell: false,
    ...options,
  })
  if (result.error) throw result.error
  if (result.status !== 0) {
    const error = new Error(`${command} failed (${result.signal ?? result.status})`)
    error.exitCode = result.status ?? 1
    throw error
  }
  return result.stdout
}

function verify() {
  run(process.execPath, ['tools/sync-release-assets.mjs', '--check'])
  const metadata = JSON.parse(run(cargo, [
    'metadata', '--no-deps', '--format-version', '1', '--locked',
  ], { stdio: ['ignore', 'pipe', 'inherit'], encoding: 'utf8' }))
  const packages = metadata.packages.filter((pkg) => metadata.workspace_members.includes(pkg.id))

  run(cargo, [
    'package',
    '--workspace',
    '--no-verify',
    '--locked',
    ...(allowDirty ? ['--allow-dirty'] : []),
  ])

  const packageRoot = join(metadata.target_directory, 'package')
  const verificationRoot = mkdtempSync(join(packageRoot, 'verify-'))
  try {
    const members = packages.map((pkg) => `${pkg.name}-${pkg.version}`)
    for (const member of members) {
      run('tar', ['-xzf', `${member}.crate`, '-C', basename(verificationRoot)], { cwd: packageRoot })
    }

    // Only archived sources may satisfy internal dependencies, never crates.io or the checkout.
    const patches = packages.map((pkg, index) =>
      `${JSON.stringify(pkg.name)} = { path = ${JSON.stringify(members[index])} }`,
    )
    writeFileSync(join(verificationRoot, 'Cargo.toml'), [
      '[workspace]',
      `members = ${JSON.stringify(members)}`,
      'resolver = "2"',
      '',
      '[patch.crates-io]',
      ...patches,
      '',
    ].join('\n'))
    copyFileSync(join(root, 'Cargo.lock'), join(verificationRoot, 'Cargo.lock'))

    // Build separately so workspace feature unification cannot hide missing dependencies.
    for (const pkg of packages) {
      run(cargo, [
        'build', '--package', pkg.name, '--locked',
        '--target-dir', metadata.target_directory,
      ], { cwd: verificationRoot })
    }
  } finally {
    rmSync(verificationRoot, { recursive: true, force: true })
  }
}

try {
  verify()
} catch (error) {
  console.error(error.message)
  process.exitCode = error.exitCode ?? 1
}
