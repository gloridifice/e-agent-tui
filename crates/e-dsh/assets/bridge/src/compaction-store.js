import { randomUUID } from 'node:crypto'
import { mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, isAbsolute, join } from 'node:path'

export function compactionModelPath() {
  const home = homedir()
  const base = process.platform === 'win32'
    ? process.env.APPDATA ?? join(home, 'AppData', 'Roaming')
    : process.platform !== 'darwin' && process.env.XDG_CONFIG_HOME && isAbsolute(process.env.XDG_CONFIG_HOME)
      ? process.env.XDG_CONFIG_HOME : join(home, '.config')
  return join(base, 'e', 'compaction-model.json')
}

export function createCompactionStore(path = compactionModelPath()) {
  return {
    load() {
      let text
      try { text = readFileSync(path, 'utf8') } catch (error) {
        if (error.code === 'ENOENT') return null
        throw error
      }
      const value = JSON.parse(text)
      if (value === null) return null
      if (value.version !== 1 || !['provider', 'model'].every(key => typeof value[key] === 'string' && value[key].trim())) {
        throw new Error(`Invalid compaction model configuration: ${path}`)
      }
      return { provider: value.provider, model: value.model, name: value.model }
    },
    save(value) {
      mkdirSync(dirname(path), { recursive: true })
      const temporary = `${path}.${randomUUID()}.tmp`
      try {
        writeFileSync(temporary, JSON.stringify(value ? { version: 1, provider: value.provider, model: value.model } : null) + '\n', { flag: 'wx' })
        renameSync(temporary, path)
      } finally {
        rmSync(temporary, { force: true })
      }
    },
  }
}
