/**
 * vendor-crates.mjs — mini cargo-vendor for machines whose schannel TLS is broken.
 *
 * Resolves the dependency graph of ../client/Cargo.toml against the crates.io
 * sparse index, downloads every needed .crate via node's fetch (openssl), and
 * materializes ../client/vendor + ../client/.cargo/config.toml so that
 * `cargo build --offline` works without touching the network.
 *
 * Usage: node vendor-crates.mjs
 */
import { createHash } from 'node:crypto'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, relative, sep } from 'node:path'
import { fileURLToPath } from 'node:url'
import { gunzipSync } from 'node:zlib'
import { parse } from 'smol-toml'
import { request as httpsRequest } from 'node:https'

const UA = 'dsh-tui-vendor/0.1 (node)'

const here = dirname(fileURLToPath(import.meta.url))
const CLIENT_ROOT = join(here, '..', 'client')
const CACHE = join(here, 'cache')
const VENDOR = join(CLIENT_ROOT, 'vendor')
const INDEX = process.env.VENDOR_INDEX ?? 'https://rsproxy.cn/index'
const DL = process.env.VENDOR_DL ?? 'https://rsproxy.cn/api/v1/crates'

mkdirSync(CACHE, { recursive: true })

// ---------- semver ----------
function stripBuildMetadata(v) {
  const i = v.indexOf('+')
  return i < 0 ? v : v.slice(0, i)
}
function cmpVersions(a, b) {
  const [ca, pa] = splitPre(stripBuildMetadata(a))
  const [cb, pb] = splitPre(stripBuildMetadata(b))
  const na = ca.split('.').map(Number)
  const nb = cb.split('.').map(Number)
  for (let i = 0; i < 3; i++) {
    const x = na[i] ?? 0
    const y = nb[i] ?? 0
    if (x !== y) return x - y
  }
  if (pa === undefined && pb === undefined) return 0
  if (pa === undefined) return 1
  if (pb === undefined) return -1
  return pa < pb ? -1 : pa > pb ? 1 : 0
}
function splitPre(v) {
  const i = v.indexOf('-')
  return i < 0 ? [v, undefined] : [v.slice(0, i), v.slice(i + 1)]
}
function satisfies(version, req) {
  if (!req || req === '*') return true
  return req.split(',').map((s) => s.trim()).filter(Boolean).every((one) => {
    const m = one.match(/^(\^|~|>=|<=|>|<|=)?\s*v?(\d+)(?:\.(\d+))?(?:\.(\d+))?/)
    if (!m) return true // unknown exotic req: be permissive
    const [, op, mj, mi = '0', pa = '0'] = m
    const ver = `${mj}.${mi}.${pa}`
    const c = cmpVersions(version, ver)
    const vp = version.split('.')
    switch (op) {
      case undefined: // bare version = caret
      case '^': {
        if (mj === '0') {
          if (mi === '0') return c >= 0 && vp[0] === '0' && vp[1] === '0'
          return c >= 0 && vp[0] === '0' && vp[1] === mi
        }
        return c >= 0 && vp[0] === mj
      }
      case '~': return c >= 0 && vp[0] === mj && vp[1] === mi
      case '>': return c > 0
      case '<': return c < 0
      case '>=': return c >= 0
      case '<=': return c <= 0
      default: return c === 0
    }
  })
}

// ---------- sparse index ----------
function indexUrl(name) {
  const n = name.toLowerCase()
  if (n.length === 1) return `${INDEX}/1/${n}`
  if (n.length === 2) return `${INDEX}/2/${n}`
  if (n.length === 3) return `${INDEX}/3/${n[0]}/${n}`
  return `${INDEX}/${n.slice(0, 2)}/${n.slice(2, 4)}/${n}`
}
// ---------- plain node:https fetch (fresh socket per attempt) ----------
// crates.io sporadically resets keepalive connections; agent:false gives every
// request its own connection, and a small inter-request pause keeps the rate
// gentle. Returns { status, body: Buffer }.
function httpGet(url, timeoutMs = 60000) {
  return new Promise((resolve, reject) => {
    const target = new URL(url)
    const req = httpsRequest(target, {
      method: 'GET',
      agent: false,
      headers: { 'user-agent': UA, accept: '*/*' },
      servername: target.hostname,
    }, (res) => {
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        res.resume()
        resolve(httpGet(new URL(res.headers.location, target).toString(), timeoutMs))
        return
      }
      const chunks = []
      res.on('data', (c) => chunks.push(c))
      res.on('end', () => resolve({ status: res.statusCode ?? 0, body: Buffer.concat(chunks) }))
      res.on('error', reject)
    })
    req.on('error', reject)
    req.setTimeout(timeoutMs, () => req.destroy(new Error(`timeout after ${timeoutMs}ms`)))
    req.end()
  })
}

let lastRequest = 0
async function fetchRetry(url, attempts = 6) {
  let lastError
  for (let i = 0; i < attempts; i++) {
    const wait = lastRequest + 40 - Date.now()
    if (wait > 0) await new Promise((r) => setTimeout(r, wait))
    lastRequest = Date.now()
    try {
      return await httpGet(url)
    } catch (error) {
      lastError = error
      await new Promise((r) => setTimeout(r, 1000 * 2 ** i))
    }
  }
  throw lastError
}
const indexCache = new Map()
async function indexVersions(name) {
  if (indexCache.has(name)) return indexCache.get(name)
  const promise = fetchRetry(indexUrl(name)).then((res) => {
    if (res.status !== 200) throw new Error(`index ${name}: HTTP ${res.status}`)
    return res.body.toString('utf8')
  }).then((text) => text.trim().split('\n').filter(Boolean).map((l) => JSON.parse(l)))
  indexCache.set(name, promise)
  return promise
}

// ---------- download ----------
const dlCache = new Map()
async function downloadCrate(name, version) {
  const key = `${name}-${version}`
  if (dlCache.has(key)) return dlCache.get(key)
  const file = join(CACHE, `${key}.crate`)
  const promise = (async () => {
    if (!exists(file)) {
      const res = await fetchRetry(`${DL}/${name}/${version}/download`)
      if (res.status !== 200) throw new Error(`download ${key}: HTTP ${res.status}`)
      writeFileSync(file, res.body)
    }
    return readFileSync(file)
  })()
  dlCache.set(key, promise)
  return promise
}
function exists(p) {
  try { readFileSync(p); return true } catch { return false }
}

// ---------- tar (ustar subset) ----------
function readTarString(buf, off, len) {
  const end = buf.indexOf(0, off)
  const slice = buf.subarray(off, off + len)
  return (end >= 0 && end < off + len ? slice.subarray(0, end - off) : slice).toString('utf8')
}
function untar(gz) {
  const buf = gunzipSync(gz)
  const files = new Map()
  let off = 0
  let longName = null
  while (off + 512 <= buf.length) {
    const header = buf.subarray(off, off + 512)
    if (header.every((b) => b === 0)) break
    let name = readTarString(header, 0, 100)
    const size = parseInt(readTarString(header, 124, 12).trim() || '0', 8) || 0
    const type = String.fromCharCode(header[156] ?? 48) || '0'
    const content = buf.subarray(off + 512, off + 512 + size)
    if (type === 'L') longName = content.toString('utf8').replace(/\0+$/, '')
    else {
      if (longName) { name = longName; longName = null }
      if (type === '0' || type === '' || type === ' ' || type === '\0' || type === '7') {
        files.set(name, content)
      }
    }
    off += 512 + Math.ceil(size / 512) * 512
  }
  return files
}

// ---------- dependency extraction ----------
// `cargo build` resolves `dependencies` (plus `build-dependencies` for any
// crate with a build.rs). Dev deps are excluded — they would explode the
// graph and are only needed for `cargo test`.
function collectDeps(toml) {
  const out = []
  const sections = ['dependencies', 'build-dependencies']
  const scan = (deps) => {
    if (!deps || typeof deps !== 'object') return
    for (const [name, spec] of Object.entries(deps)) {
      if (typeof spec === 'string') out.push({ name, req: spec })
      else if (spec && typeof spec === 'object') {
        if (spec.path || spec.git || spec.workspace) continue
        if (spec.version) out.push({ name: spec.package ?? name, req: spec.version })
      }
    }
  }
  for (const s of sections) scan(toml[s])
  const target = toml.target
  if (target && typeof target === 'object') {
    for (const t of Object.values(target)) {
      if (!t || typeof t !== 'object') continue
      for (const s of sections) scan(t[s])
    }
  }
  return out
}

// ---------- main resolution ----------
const chosen = new Map() // name -> Map(version -> { files, cksum })
const queue = []
const queued = new Set()
function enqueue(name, req) {
  const key = `${name}@${req}`
  if (queued.has(key)) return
  queued.add(key)
  queue.push({ name, req })
}

// Crash-resumable state: chosen name→versions plus the pending queue.
const stateFile = join(CACHE, 'state.json')
let state = { chosen: {}, queue: [], queued: [] }
if (exists(stateFile)) {
  try { state = JSON.parse(readFileSync(stateFile, 'utf8')) } catch {}
}
for (const item of state.queue ?? []) enqueue(item.name, item.req)
// NOTE: state.queued is deliberately NOT restored. Rebuilding the chosen
// entries re-enqueues every dependency; a restored dedup set would wrongly
// block requests the previous (possibly crashed) run had already consumed.
function saveState() {
  writeFileSync(stateFile, JSON.stringify({
    chosen: Object.fromEntries([...chosen].map(([n, vs]) => [n, [...vs.keys()]])),
    queue,
    queued: [...queued],
  }))
}

/** Download, unpack, digest one crate and record it; re-enqueues its deps. */
async function ensureEntry(name, version) {
  if (chosen.get(name)?.has(version)) return
  const crate = await downloadCrate(name, version)
  const files = untar(crate)
  const prefix = `${name}-${version}/`
  const manifest = files.get(`${prefix}Cargo.toml`)
  if (!manifest) {
    console.warn(`!! ${name}-${version}: no Cargo.toml inside`)
    return
  }
  let deps = []
  try { deps = collectDeps(parse(manifest.toString('utf8'))) } catch (e) {
    console.warn(`!! ${name}-${version}: Cargo.toml parse failed: ${e.message}`)
  }
  const filesDigest = {}
  const relFiles = []
  for (const [path, content] of files) {
    if (!path.startsWith(prefix)) continue
    const rel = path.slice(prefix.length).replaceAll('/', sep)
    relFiles.push([rel, content])
    filesDigest[rel.replaceAll(sep, '/')] = createHash('sha256').update(content).digest('hex')
  }
  if (!chosen.has(name)) chosen.set(name, new Map())
  chosen.get(name).set(version, {
    files: relFiles,
    checksum: {
      files: filesDigest,
      package: createHash('sha256').update(crate).digest('hex'),
    },
  })
  for (const dep of deps) enqueue(dep.name, dep.req)
}

const rootToml = parse(readFileSync(join(CLIENT_ROOT, 'Cargo.toml'), 'utf8'))
for (const dep of collectDeps(rootToml)) enqueue(dep.name, dep.req)

// Rebuild entries for crates resolved by a previous run (cache hit).
for (const [name, versions] of Object.entries(state.chosen ?? {})) {
  for (const version of versions) await ensureEntry(name, version)
}

let processed = chosen.size ? [...chosen.values()].reduce((n, vs) => n + vs.size, 0) : 0
let sinceSave = 0
while (queue.length) {
  const { name, req } = queue.shift()
  const chosenVersions = chosen.get(name)
  const chosenOk = (v) => satisfies(v, req) && (req.includes('-') || !v.includes('-'))
  if (chosenVersions && [...chosenVersions.keys()].some(chosenOk)) continue
  const all = await indexVersions(name)
  const candidates = all
    .filter((e) => !e.yanked
      && satisfies(e.vers, req)
      // cargo semantics: a req without a prerelease never matches one
      && (req.includes('-') || !e.vers.includes('-')))
    .map((e) => e.vers)
    .sort(cmpVersions)
  if (!candidates.length) {
    console.warn(`!! no version of ${name} satisfies "${req}"`)
    continue
  }
  const version = candidates.at(-1)
  const before = chosen.get(name)?.size ?? 0
  await ensureEntry(name, version)
  if ((chosen.get(name)?.size ?? 0) > before) {
    processed++
    console.log(`resolved ${processed}: ${name}-${version} (queue ${queue.length})`)
    if (++sinceSave >= 10) { saveState(); sinceSave = 0 }
  }
}
saveState()
console.log(`resolution done: ${processed} crates, ${chosen.size} names`)

// ---------- materialize vendor ----------
const vendorDir = VENDOR
for (const [name, versions] of chosen) {
  for (const [version, entry] of versions) {
    const dir = join(vendorDir, `${name}-${version}`)
    mkdirSync(dir, { recursive: true })
    for (const [rel, content] of entry.files) {
      const target = join(dir, rel)
      mkdirSync(dirname(target), { recursive: true })
      writeFileSync(target, content)
    }
    writeFileSync(join(dir, '.cargo-checksum.json'), JSON.stringify(entry.checksum, null, 2))
  }
}
console.log(`vendor written to ${relative(here, vendorDir)}`)
