#!/usr/bin/env node
// Record a disposable pie session. Run from the repository root on Windows.
import { spawn, execFile } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { mkdtemp, mkdir, readFile, readdir, rename, rm, stat, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { promisify } from 'node:util';

const exec = promisify(execFile);
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const digest = data => createHash('sha256').update(data).digest('hex');
const routeShown = (text, model) => text.split('\n').some(line => line.includes('e·pi ') && [model.name, model.id].some(name => line.toLowerCase().includes(name.toLowerCase())));
const limits = { calls: 4, generationMs: 180_000, totalMs: 15 * 60_000 };
const instructionGapMs = 1_000;
const characterDelayMs = 100;
const sample = 'pub fn double(n: i32) -> i32 { n * 2 }\n';

function usage() {
  return `Usage: node tools/record-demo.mjs --pie <release-pie.exe> --model <provider/model> --output <video.mp4> [--compare-model <provider/model>] [--preflight]\n\nWindows only. Requires pi, tui-test, FFmpeg and FFprobe on PATH. Uses the current user's Pi credentials and frontend config; backs up frontend config and resume state, and restores them unless another writer changes them during recording. Do not use pie/dshe concurrently. No audio or subtitles. A failed run does not publish a video. Preflight makes no model calls.\n`;
}

function options(argv) {
  if (argv.includes('--help') || argv.includes('-h')) return null;
  const args = {};
  const allowed = new Set(['--pie', '--model', '--output', '--compare-model']);
  for (let i = 0; i < argv.length; i++) {
    const flag = argv[i];
    if (flag === '--preflight') { args.preflight = true; continue; }
    if (!allowed.has(flag) || !argv[i + 1] || argv[i + 1].startsWith('--') || args[flag]) {
      throw new Error(`Invalid argument ${flag}\n${usage()}`);
    }
    args[flag] = argv[++i];
  }
  for (const name of ['--pie', '--model', '--output']) {
    if (!args[name]) throw new Error(`Missing ${name}\n${usage()}`);
  }
  for (const name of ['--model', '--compare-model']) {
    if (args[name] && !/^[a-zA-Z0-9._-]+\/[a-zA-Z0-9._-]+$/.test(args[name])) {
      throw new Error(`${name} must be an exact provider/model identity`);
    }
  }
  if (path.extname(args['--output']).toLowerCase() !== '.mp4') throw new Error('--output must end in .mp4');
  return { pie: path.resolve(args['--pie']), model: args['--model'], output: path.resolve(args['--output']), compare: args['--compare-model'], preflight: !!args.preflight };
}

async function run(file, args, timeout = 30_000, extra = {}) {
  try {
    return (await exec(file, args, { timeout, maxBuffer: 8 * 1024 * 1024, windowsHide: true, ...extra })).stdout;
  } catch (error) {
    throw new Error(`${file} ${args.slice(0, 4).join(' ')}: ${String(error.stderr || error.message).slice(0, 700)}`);
  }
}

function rpcCatalog() {
  return new Promise((resolve, reject) => {
    // Only fixed arguments enter cmd.exe; model IDs never reach the shell.
    const child = spawn('cmd.exe', ['/d', '/s', '/c', 'pi.cmd --mode rpc --no-session --no-approve'], { stdio: ['pipe', 'pipe', 'ignore'], windowsHide: true });
    let buffer = '';
    let done = false;
    const timer = setTimeout(() => finish(new Error('Pi model catalog timed out')), 30_000);
    function finish(error, models) {
      if (done) return;
      done = true;
      clearTimeout(timer);
      child.kill();
      if (error) reject(error); else resolve(models);
    }
    child.on('error', finish);
    child.on('exit', () => { if (!done) finish(new Error('Pi exited before supplying a model catalog')); });
    child.stdout.on('data', chunk => {
      buffer += chunk;
      if (buffer.length > 10 * 1024 * 1024) return finish(new Error('Pi catalog output exceeded limit'));
      let index;
      while ((index = buffer.indexOf('\n')) >= 0) {
        const line = buffer.slice(0, index); buffer = buffer.slice(index + 1);
        try {
          const record = JSON.parse(line);
          if (record.type === 'response' && record.command === 'get_available_models') {
            return finish(record.success === false ? new Error('Pi rejected model catalog request') : null, record.data?.models);
          }
        } catch { /* Ignore non-JSON startup diagnostics. */ }
      }
    });
    child.stdin.end(JSON.stringify({ type: 'get_available_models', id: 'e-demo-catalog' }) + '\n');
  });
}

function exactModel(catalog, route) {
  const [provider, id] = route.split('/');
  const model = catalog.find(item => item.provider === provider && item.id === id);
  if (!model) throw new Error(`Model ${route} is not in Pi's live catalog; no alternative is selected automatically`);
  return model;
}

function supportedEfforts(model) {
  if (!model.reasoning) return [];
  const map = model.thinkingLevelMap || {};
  return ['off', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'].filter(level => map[level] !== null && (!['xhigh', 'max'].includes(level) || Object.hasOwn(map, level)));
}

class Terminal {
  constructor(session) { this.session = session; }
  async call(...args) {
    const text = await run('tui-test', ['--session', this.session, '--json', ...args], args[0] === 'record' ? 120_000 : 35_000);
    const result = JSON.parse(text);
    if (!result.ok) throw new Error(`tui-test ${args[0]}: ${result.message}`);
    return result.data;
  }
  async state() {
    const state = await this.call('state');
    if (state.exited !== null) throw new Error(`pie exited unexpectedly (code ${state.exited})`);
    return state.text;
  }
  async wait(check, description, ms = 15_000) {
    const deadline = Date.now() + ms;
    let last = '';
    while (Date.now() < deadline) {
      const text = await this.state();
      last = text;
      if (check(text)) return text;
      await sleep(350);
    }
    const route = last.split('\n').find(line => line.includes('e·pi '))?.trim() || '(no route header)';
    const composer = last.split('\n').find(line => line.includes('❯'))?.trim() || '(no composer)';
    throw new Error(`Timed out waiting for ${description}; current header: ${route}; composer: ${composer.slice(0, 100)}`);
  }
  async keys(...keys) {
    for (const key of keys) {
      await this.call('key', 'press', key);
      await sleep(instructionGapMs);
    }
  }
  async closePage() {
    for (let i = 0; i < 3; i++) {
      await this.keys('Escape');
      try { await this.wait(text => /❯\s*█/.test(text), 'closed input page', 2_000); return; }
      catch { /* Some pages are still processing an asynchronous update. */ }
    }
    throw new Error('Input page did not close');
  }
  async line(line, markedName = null) {
    if (line.includes('\n') || line.length > 60) throw new Error('Unsafe or overlong single-line input');
    const before = await this.state();
    if (!/❯\s*█/.test(before)) {
      const composer = before.split('\n').find(row => row.includes('❯'))?.trim() || '(composer hidden)';
      throw new Error(`Composer must be empty before typing; refusing to submit: ${composer.slice(0, 120)}`);
    }
    for (const character of line) {
      await this.call('type', '--', character);
      await sleep(characterDelayMs);
    }
    const visible = markedName ? line.replace(/^(\/\/[a-z] )/, `$1${markedName} `) : line;
    const draft = await this.wait(text => text.includes(`❯ ${visible}█`), `literal composer input ${line.slice(0, 40)}`);
    if (!draft.includes(`❯ ${visible}█`)) throw new Error('Input was modified by the terminal');
    await sleep(instructionGapMs);
    const maxSubmitPresses = line.startsWith('/') ? 6 : 1;
    for (let press = 0; press < maxSubmitPresses; press++) {
      await this.keys('Enter');
      try {
        await this.wait(text => !text.includes(`❯ ${visible}█`), 'prompt admission', 1_500);
        return;
      } catch (error) {
        const current = await this.state();
        if (!current.includes(`❯ ${visible}█`)) throw error;
        if (press + 1 === maxSubmitPresses) {
          throw new Error(`Input remained in the composer after ${maxSubmitPresses} submission keys`);
        }
      }
    }
  }
  async close() {
    try { await this.call('close'); } catch (error) { console.error(`Cleanup: ${error.message}`); }
  }
}

async function filesUnder(root) {
  try {
    const entries = await readdir(root, { recursive: true, withFileTypes: true });
    return entries.filter(entry => entry.isFile() && entry.name.endsWith('.jsonl')).map(entry => path.join(entry.parentPath, entry.name));
  } catch (error) {
    if (error.code === 'ENOENT') return [];
    throw error;
  }
}

async function sessionHeaders(root) {
  return await Promise.all((await filesUnder(root)).map(async file => {
    const header = JSON.parse((await readFile(file, 'utf8')).split('\n')[0]);
    return { file: path.basename(file), id: header.id, parentSession: header.parentSession };
  }));
}

async function sessionMessages(root) {
  const files = await filesUnder(root);
  const records = [];
  for (const file of files) {
    if ((await stat(file)).size > 8 * 1024 * 1024) throw new Error('Demo session exceeded read budget');
    for (const line of (await readFile(file, 'utf8')).split('\n')) {
      try {
        const record = JSON.parse(line);
        if (record.type === 'message') records.push(record.message);
      } catch { /* The last JSONL line can still be in progress. */ }
    }
  }
  return records;
}

async function waitForAnswer(terminal, root, before, timeout) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    await terminal.state();
    const messages = await sessionMessages(root);
    const answers = messages.filter(message => message?.role === 'assistant' && message.stopReason && message.stopReason !== 'toolUse');
    if (answers.length > before) {
      const latest = answers.at(-1);
      if (latest.stopReason !== 'stop') throw new Error(`Model finished with ${latest.stopReason}, not success`);
      await sleep(1_500);
      return messages;
    }
    await sleep(1_000);
  }
  throw new Error(`Model did not settle within ${timeout / 1_000}s`);
}

async function snapshot(file, dir, name) {
  let original = null;
  try { original = await readFile(file); } catch (error) { if (error.code !== 'ENOENT') throw error; }
  if (original) await writeFile(path.join(dir, `${name}.backup`), original, { flag: 'wx' });
  return { file, original, known: original ? digest(original) : null, changed: false };
}

async function currentHash(file) {
  try { return digest(await readFile(file)); } catch (error) { if (error.code === 'ENOENT') return null; throw error; }
}

async function noteWrite(saved) { saved.known = await currentHash(saved.file); saved.changed = true; }

async function restore(saved, dir) {
  if (!saved.changed) return true;
  if (await currentHash(saved.file) !== saved.known) {
    console.error(`Concurrent change detected in ${saved.file}; leaving it untouched. Backup: ${path.join(dir, `${path.basename(saved.file)}.backup`)}`);
    return false;
  }
  if (!saved.original) {
    await rm(saved.file, { force: true });
  } else {
    const temp = `${saved.file}.e-demo-${randomUUID()}.tmp`;
    await writeFile(temp, saved.original, { flag: 'wx' });
    await rename(temp, saved.file);
  }
  return true;
}

async function duration(file) {
  const output = await run('ffprobe', ['-v', 'error', '-show_entries', 'format=duration', '-of', 'default=noprint_wrappers=1:nokey=1', file]);
  const seconds = Number(output.trim());
  if (!Number.isFinite(seconds) || seconds <= 0) throw new Error(`No playable video in ${file}`);
  return seconds;
}

async function exportVideo(scenes, output, dir) {
  await Promise.all(scenes.map(scene => duration(scene.file)));
  // A spinner repaints while the model waits. Preserve every frame until a safe cut can be identified.
  const clips = [];
  const edits = [];
  for (const [index, scene] of scenes.entries()) {
    const clip = path.join(dir, `clip-${index}.mp4`);
    await run('ffmpeg', ['-hide_banner', '-loglevel', 'error', '-y', '-i', scene.file, '-an', '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-r', '30', clip], 120_000);
    clips.push(clip);
  }
  const list = path.join(dir, 'clips.txt');
  await writeFile(list, clips.map(file => `file '${file.replaceAll("'", "'\\''")}'`).join('\n'));
  const staged = path.join(dir, 'finished.mp4');
  await run('ffmpeg', ['-hide_banner', '-loglevel', 'error', '-y', '-f', 'concat', '-safe', '0', '-i', list, '-an', '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-r', '30', staged], 120_000);
  const finalDuration = await duration(staged);
  if (finalDuration > 90) console.warn(`Video is ${finalDuration.toFixed(1)}s; the 90-second target is deferred for this pacing review.`);
  await mkdir(path.dirname(output), { recursive: true });
  await rename(staged, output);
  await writeFile(`${output}.json`, JSON.stringify({ duration: finalDuration, instructionGapSeconds: instructionGapMs / 1_000, characterDelayMs, over90Seconds: finalDuration > 90, models: scenes[0].models, editedWaits: edits, timingIsNotLivePerformance: true }, null, 2) + '\n');
  return { duration: finalDuration, edits };
}

async function main() {
  const args = options(process.argv.slice(2));
  if (!args) { process.stdout.write(usage()); return; }
  if (process.platform !== 'win32') throw new Error('This first recording workflow is Windows-only');
  await stat(args.pie);
  await Promise.all(['tui-test', 'ffmpeg', 'ffprobe'].map(program => run(program, ['-version']).catch(async () => run(program, ['--version']))));
  const catalog = await rpcCatalog();
  if (!Array.isArray(catalog)) throw new Error('Pi returned no model catalog');
  const mainModel = exactModel(catalog, args.model);
  const other = catalog.find(model => model.provider === mainModel.provider && model.id === 'qwen3.8-max' && model.id !== mainModel.id)
    || catalog.find(model => model.reasoning && (model.provider !== mainModel.provider || model.id !== mainModel.id));
  const compare = args.compare || (other && `${other.provider}/${other.id}`);
  if (!compare || compare === args.model) throw new Error('Supply --compare-model with a distinct available route');
  const comparison = exactModel(catalog, compare);
  const effort = supportedEfforts(mainModel).includes('medium') ? 'medium' : supportedEfforts(mainModel).find(level => level !== 'off');
  if (!effort) throw new Error(`Model ${args.model} has no supported reasoning effort to demonstrate`);
  console.log(`Recording plan: ${args.model} (${mainModel.name}, effort ${effort}); resting comparison: ${compare} (${comparison.name}); max ${limits.calls} model calls, ${limits.generationMs / 1_000}s each.`);
  if (args.preflight) return;
  if (await currentHash(args.output)) throw new Error(`Refusing to overwrite ${args.output}`);

  if (!process.env.PUBLIC) throw new Error('Windows PUBLIC directory is required for an anonymized demo workspace');
  const dir = await mkdtemp(path.join(os.tmpdir(), 'e-record-demo-'));
  const workspace = await mkdtemp(path.join(process.env.PUBLIC, 'e-demo-work-'));
  const root = path.join(dir, 'sessions');
  await mkdir(root);
  await writeFile(path.join(workspace, 'sample.rs'), sample);
  const config = await snapshot(path.join(process.env.APPDATA, 'e', 'config.toml'), dir, 'config.toml');
  const state = await snapshot(path.join(process.env.APPDATA, 'pie', 'data', 'state.toml'), dir, 'state.toml');
  const terminal = new Terminal(`e-demo-${randomUUID().slice(0, 12)}`);
  let finished = false;
  let calls = 0;
  const scenes = [];
  const begin = Date.now();
  const deadline = () => { if (Date.now() - begin > limits.totalMs) throw new Error('Recording exceeded total 15-minute budget'); };
  const record = async (name, action) => {
    deadline();
    const file = path.join(dir, `${name}.mp4`);
    await terminal.call('record', 'start', file, '--fps', '30');
    try {
      await sleep(instructionGapMs);
      await action();
    } finally {
      await sleep(instructionGapMs);
      await terminal.call('record', 'stop');
    }
    scenes.push({ name, file, models: { main: args.model, compare } });
  };
  const modelCall = async (line, markedName = null) => {
    if (++calls > limits.calls) throw new Error('Model call budget exhausted');
    const before = (await sessionMessages(root)).filter(message => message?.role === 'assistant' && message.stopReason && message.stopReason !== 'toolUse').length;
    await terminal.line(line, markedName);
    return await waitForAnswer(terminal, root, before, limits.generationMs);
  };
  try {
    await terminal.call('run', '--cols', '120', '--rows', '36', '--cwd', workspace, '--env', `PI_CODING_AGENT_SESSION_DIR=${root}`, '--', args.pie, '--cwd', workspace, '--session', path.join(root, 'demo.jsonl'), '--approve');
    await terminal.wait(text => text.includes('e·pi') && text.includes('❯'), 'pie composer', 25_000);
    await terminal.keys('Ctrl+L');
    await terminal.wait(text => text.includes(comparison.name), 'model catalog', 25_000);
    await terminal.keys('Escape');
    await terminal.line(`/model ${compare}`);
    await terminal.wait(text => routeShown(text, comparison), 'comparison route');

    await record('working', async () => {
      await terminal.line(`/model ${args.model}`);
      await terminal.wait(text => routeShown(text, mainModel), 'main route');
      const messages = await modelCall('Use read on sample.rs. Explain double; quote code. No edits.');
      if (!messages.some(message => message?.role === 'toolResult')) throw new Error('No visible read-tool result was recorded');
      await sleep(1_800);
    });
    await record('reading', async () => {
      const before = await terminal.state();
      await terminal.keys('Ctrl+R');
      await terminal.wait(text => text !== before, 'Reading view');
      await terminal.keys('Down', 'Right', 'Down');
      await sleep(1_800);
      await terminal.keys('Escape');
      await terminal.wait(text => /❯\s*█/.test(text), 'composer after Reading exit');
      await sleep(500);
    });
    await record('effort', async () => {
      await terminal.line(`/model ${args.model} set-default-effort ${effort}`);
      await sleep(500);
      await noteWrite(config);
      await terminal.keys('Ctrl+L');
      await terminal.wait(text => text.split('\n').some(row => row.includes(mainModel.name) && row.toLowerCase().includes(effort)), 'default effort annotation');
      await sleep(1_500);
      await terminal.closePage();
    });
    await record('marks', async () => {
      const current = await readFile(config.file, 'utf8');
      const used = [...current.matchAll(/letter\s*=\s*["']([a-z])["']/g)].map(match => match[1]);
      const mark = [...'abcdefgimnoprstuvwxyz'].find(letter => !used.includes(letter));
      if (!mark) throw new Error('No free model mark letter');
      await terminal.keys('Ctrl+L');
      await terminal.wait(text => text.includes(mainModel.name), 'model menu');
      await terminal.keys(`Shift+${mark.toUpperCase()}`);
      await terminal.wait(text => text.split('\n').some(row => row.includes(mainModel.name) && row.includes(`[${mark}]`)), 'new model mark');
      await noteWrite(config);
      await sleep(1_000);
      await terminal.closePage();
      await terminal.line(`/model ${compare}`);
      await terminal.wait(text => routeShown(text, comparison), 'comparison route');
      await modelCall(`//${mark} 7 doubled?`, mainModel.name);
      await terminal.wait(text => routeShown(text, comparison), 'restored route', 20_000);
      await sleep(1_200);
    });
    await record('fork', async () => {
      await terminal.line('/fork');
      await terminal.wait(text => text.includes('Fork session'), 'fork selection');
      await sleep(900);
      await terminal.keys('Enter');
      const forkDeadline = Date.now() + 30_000;
      let ancestry = [];
      while (Date.now() < forkDeadline) {
        await terminal.state();
        ancestry = await sessionHeaders(root);
        if (ancestry.some(session => session.parentSession)) break;
        await sleep(700);
      }
      if (!ancestry.some(session => session.parentSession)) throw new Error(`Native fork did not create a child session: ${JSON.stringify(ancestry)}`);
      // The child file can appear before the frontend finishes restoring the forked draft.
      await terminal.wait(text => /❯\s*7 doubled\?/.test(text), 'restored fork draft', 30_000);
      await sleep(700);
      await terminal.keys('Ctrl+C');
      await terminal.wait(text => /❯\s*█/.test(text), 'cleared fork draft');
      await sleep(700);
      if (!/❯\s*█/.test(await terminal.state())) throw new Error('Fork draft reappeared; refusing to type resume');
      await terminal.line('/resume');
      try {
        await terminal.wait(text => /\(fork\)/i.test(text), 'resume ancestry', 30_000);
      } catch (error) {
        console.error(`Resume viewport (disposable session only):\n${(await terminal.state()).slice(0, 4000)}`);
        console.error(`Native sessions: ${JSON.stringify(await sessionHeaders(root))}`);
        throw error;
      }
      await sleep(2_000);
      await terminal.keys('Escape');
    });
    const result = await exportVideo(scenes, args.output, dir);
    finished = true;
    console.log(`Created ${args.output} (${result.duration.toFixed(1)}s). Edited waits: ${result.edits.length}; see ${args.output}.json. Inspect each scene before publishing.`);
  } finally {
    await terminal.close();
    const restoredConfig = await restore(config, dir);
    // Preserve changes from other pie processes; restore only a pointer to one of our own sessions.
    if (await currentHash(state.file) !== state.known) {
      const data = await readFile(state.file, 'utf8').catch(() => '');
      const headers = await sessionHeaders(root);
      if (data.replaceAll('\\\\', '\\').includes(root) || headers.some(header => data.includes(header.id))) {
        await noteWrite(state);
      } else {
        console.error(`Resume state was updated by another writer; preserving ${state.file}`);
      }
    }
    const restoredState = await restore(state, dir);
    await rm(workspace, { recursive: true, force: true });
    if (restoredConfig && restoredState) await rm(dir, { recursive: true, force: true });
    else console.error(`Manual restoration needed; keeping backup in ${dir}`);
    if (!finished) console.error('Recording failed; no finished video was published.');
  }
}

main().catch(error => { console.error(error.stack || String(error)); process.exitCode = 1; });
