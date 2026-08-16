import { get as httpsGet } from 'node:https'

const urls = [
  'https://rsproxy.cn/index/config.json',
  'https://index.crates.io/config.json',
  'https://mirrors.tuna.tsinghua.edu.cn/crates.io-index/config.json',
  'https://mirrors.sjtug.sjtu.edu.cn/crates.io-index/config.json',
  'https://static.crates.io/crates/serde/1.0.0/download',
]
for (const u of urls) {
  try {
    const r = await new Promise((res, rej) => {
      const q = httpsGet(u, { agent: false, timeout: 15000, headers: { 'user-agent': 'dsh-tui-probe' } }, (x) => {
        let b = ''
        x.on('data', (c) => { b += c })
        x.on('end', () => res({ s: x.statusCode, b: b.slice(0, 80) }))
      })
      q.on('error', rej)
      q.setTimeout(15000, () => q.destroy(new Error('timeout')))
    })
    console.log(u, '=>', r.s, r.b.replace(/\n/g, ' '))
  } catch (e) {
    console.log(u, '=> FAIL', e.code || e.message)
  }
}
