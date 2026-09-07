import {
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { dirname, join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const bridgeRoot = join(root, 'bridge')
const outputRoot = join(root, 'crates', 'e-dsh', 'assets', 'bridge')

function productionFiles() {
  const files = ['package.json', 'protocol-contract.json']
  const modules = readdirSync(join(bridgeRoot, 'src'), { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith('.js'))
    .map((entry) => `src/${entry.name}`)
    .sort()
  return [...files, ...modules]
}

function mirroredFiles(directory, prefix = '') {
  let entries
  try {
    entries = readdirSync(directory, { withFileTypes: true })
  } catch (error) {
    if (error.code === 'ENOENT') return []
    throw error
  }

  return entries
    .sort((left, right) => left.name.localeCompare(right.name))
    .flatMap((entry) => {
      const path = join(directory, entry.name)
      const name = prefix ? `${prefix}/${entry.name}` : entry.name
      return entry.isDirectory() ? mirroredFiles(path, name) : [name]
    })
}

export function syncReleaseAssets({ check = false } = {}) {
  const expected = productionFiles()
  if (!check) rmSync(outputRoot, { recursive: true, force: true })

  const stale = []
  for (const name of expected) {
    const source = join(bridgeRoot, ...name.split('/'))
    const destination = join(outputRoot, ...name.split('/'))
    const content = readFileSync(source)

    if (check) {
      let mirrored
      try {
        mirrored = readFileSync(destination)
      } catch (error) {
        if (error.code !== 'ENOENT') throw error
      }
      if (!mirrored || !mirrored.equals(content)) stale.push(name)
      continue
    }

    mkdirSync(dirname(destination), { recursive: true })
    writeFileSync(destination, content)
  }

  if (check) {
    const expectedSet = new Set(expected)
    for (const name of mirroredFiles(outputRoot)) {
      if (!expectedSet.has(name)) stale.push(name)
    }
  }

  if (stale.length > 0) {
    const output = relative(root, outputRoot).replaceAll('\\', '/')
    throw new Error(
      `generated release assets are stale under ${output}:\n${[...new Set(stale)]
        .sort()
        .map((name) => `- ${name}`)
        .join('\n')}\nRun: node tools/sync-release-assets.mjs`,
    )
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  try {
    syncReleaseAssets({ check: process.argv.includes('--check') })
  } catch (error) {
    console.error(error.message)
    process.exitCode = 1
  }
}
