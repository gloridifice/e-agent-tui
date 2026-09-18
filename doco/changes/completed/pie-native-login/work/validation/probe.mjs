import assert from "node:assert/strict";
import { mkdtemp, mkdir, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";

const packageRoot = resolve(process.argv[2] ?? "");
if (!packageRoot) throw new Error("usage: node probe.mjs <pi-package-root>");
const cli = join(packageRoot, "dist", "bundle", "cli.js");
const sdkUrl = pathToFileURL(join(packageRoot, "dist", "index.js")).href;
const extension = resolve(dirname(fileURLToPath(import.meta.url)), "fixture-extension.mjs");
const root = await mkdtemp(join(tmpdir(), "pie-native-login-"));
const agentDir = join(root, "agent");
const cwd = join(root, "workspace");
await mkdir(agentDir, { recursive: true });
await mkdir(cwd, { recursive: true });

const child = spawn(process.execPath, [
  cli,
  "--mode", "rpc",
  "--no-session",
  "--no-context-files",
  "--no-skills",
  "--no-prompt-templates",
  "--no-themes",
  "--extension", extension,
  "--approve",
], {
  cwd,
  env: { ...process.env, PI_CODING_AGENT_DIR: agentDir, PI_OFFLINE: "1" },
  stdio: ["pipe", "pipe", "pipe"],
});
const lines = createInterface({ input: child.stdout, crlfDelay: Infinity });
const waiting = [];
const records = [];
lines.on("line", line => {
  const value = JSON.parse(line);
  records.push(value);
  for (let i = waiting.length - 1; i >= 0; i--) {
    if (waiting[i].predicate(value)) waiting.splice(i, 1)[0].resolve(value);
  }
});
let stderr = "";
child.stderr.on("data", chunk => { stderr += chunk; });
function waitFor(predicate, timeoutMs = 10_000) {
  const existing = records.find(predicate);
  if (existing) return Promise.resolve(existing);
  return new Promise((resolvePromise, reject) => {
    const entry = { predicate, resolve: value => { clearTimeout(timer); resolvePromise(value); } };
    waiting.push(entry);
    const timer = setTimeout(() => {
      const index = waiting.indexOf(entry);
      if (index >= 0) waiting.splice(index, 1);
      reject(new Error(`RPC timeout; stderr=${stderr}`));
    }, timeoutMs);
  });
}
let sequence = 0;
async function rpc(command) {
  const id = `p-${++sequence}`;
  child.stdin.write(`${JSON.stringify({ id, ...command })}\n`);
  return waitFor(value => value.type === "response" && value.id === id);
}

try {
  const initial = await rpc({ type: "get_available_models" });
  assert.equal(initial.success, true);
  assert.equal(initial.data.models.some(model => model.provider === "probe-factory"), false);

  assert.equal((await rpc({ type: "prompt", message: "/probe-register-late" })).success, true);
  assert.ok(records.some(value => value.type === "extension_ui_request" && value.method === "notify" && value.message === "probe-late registered"));

  const { createAgentSessionServices } = await import(sdkUrl);
  const helper = await createAgentSessionServices({
    cwd,
    agentDir,
    resourceLoaderOptions: { additionalExtensionPaths: [extension] },
  });
  const helperProviders = helper.modelRuntime.getProviders().map(provider => provider.id);
  assert.ok(helperProviders.includes("probe-factory"));
  assert.equal(helperProviders.includes("probe-late"), false,
    "a separate helper cannot observe provider registrations made later in the live RPC extension instance");

  const prompts = [];
  await helper.modelRuntime.login("probe-factory", "api_key", {
    async prompt(prompt) {
      prompts.push(prompt.type);
      if (prompt.type === "select") return "multi";
      if (prompt.type === "secret") return "probe-secret";
      return "probe-account";
    },
    notify() {},
  });
  assert.deepEqual(prompts, ["select", "secret", "text"]);

  const authText = await readFile(join(agentDir, "auth.json"), "utf8");
  assert.ok(authText.includes('"probe-factory"'));
  assert.ok(authText.includes('"PROBE_ACCOUNT"'));

  const beforeRefresh = await rpc({ type: "get_available_models" });
  assert.equal(beforeRefresh.data.models.some(model => model.provider === "probe-factory"), false);
  assert.equal((await rpc({ type: "prompt", message: "/probe-refresh" })).success, true);
  const afterRefresh = await rpc({ type: "get_available_models" });
  assert.ok(afterRefresh.data.models.some(model => model.provider === "probe-factory"),
    "the public extension ModelRegistry.refresh path updates the original RPC runtime");

  console.log(JSON.stringify({
    packageRoot,
    isolatedAgentDir: true,
    factoryProviderVisibleToHelper: true,
    lateProviderVisibleToHelper: false,
    nativePromptKinds: prompts,
    liveRpcRefreshViaPublicExtensionApi: true,
  }, null, 2));
} finally {
  child.stdin.end();
  child.kill();
}
