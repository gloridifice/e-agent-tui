import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const allowDirty = process.argv.includes('--allow-dirty')

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: root,
    stdio: 'inherit',
    shell: false,
  })
  if (result.error) throw result.error
  if (result.status !== 0) process.exit(result.status ?? 1)
}

run(process.execPath, ['tools/sync-release-assets.mjs', '--check'])
run(process.platform === 'win32' ? 'cargo.exe' : 'cargo', [
  'publish',
  '--workspace',
  '--dry-run',
  '--locked',
  ...(allowDirty ? ['--allow-dirty'] : []),
])
