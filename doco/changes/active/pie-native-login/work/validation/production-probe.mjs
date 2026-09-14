import assert from "node:assert/strict";
import { copyFile, mkdtemp, mkdir, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { createInterface } from "node:readline";

const packageRoot = resolve(process.argv[2] ?? "");
if (!packageRoot) throw new Error("usage: node production-probe.mjs <pi-package-root>");
const here = dirname(fileURLToPath(import.meta.url));
const repository = resolve(here, "../../../../../..");
const helperPath = join(repository, "crates", "e-pi", "src", "auth_helper.mjs");
const companionPath = join(repository, "crates", "e-pi", "src", "auth_companion.mjs");
const fixturePath = join(here, "fixture-extension.mjs");
const root = await mkdtemp(join(tmpdir(), "pie-native-auth-production-"));
const agentDir = join(root, "agent");
const cwd = join(root, "workspace");
await mkdir(agentDir, { recursive: true });
await mkdir(cwd, { recursive: true });
await writeFile(join(agentDir, "settings.json"), JSON.stringify({ extensions: [fixturePath] }));
const untrustedAgentDir = join(root, "untrusted-agent");
const untrustedCwd = join(root, "untrusted-workspace");
const untrustedExtensionDir = join(untrustedCwd, ".pi", "extensions");
await mkdir(untrustedAgentDir, { recursive: true });
await mkdir(untrustedExtensionDir, { recursive: true });
await writeFile(join(untrustedAgentDir, "settings.json"), JSON.stringify({ extensions: [] }));
await copyFile(fixturePath, join(untrustedExtensionDir, "fixture-extension.mjs"));

function harness(child) {
  const records = [];
  const waiting = [];
  let stderr = "";
  createInterface({ input: child.stdout, crlfDelay: Infinity }).on("line", line => {
    const record = JSON.parse(line);
    records.push(record);
    for (let index = waiting.length - 1; index >= 0; index--) {
      if (waiting[index].predicate(record)) waiting.splice(index, 1)[0].resolve(record);
    }
  });
  child.stderr.on("data", chunk => { stderr = `${stderr}${chunk}`.slice(-32_768); });
  const wait = (predicate, timeoutMs = 15_000) => {
    const found = records.find(predicate);
    if (found) return Promise.resolve(found);
    return new Promise((resolvePromise, reject) => {
      const entry = {
        predicate,
        resolve(value) {
          clearTimeout(timer);
          resolvePromise(value);
        },
      };
      waiting.push(entry);
      const timer = setTimeout(() => {
        const index = waiting.indexOf(entry);
        if (index >= 0) waiting.splice(index, 1);
        reject(new Error(`process timeout; records=${JSON.stringify(records.map(record => ({ type: record.type, flowId: record.flowId, requestId: record.requestId })))}; stderr=${stderr}`));
      }, timeoutMs);
    });
  };
  return { records, wait, write: value => child.stdin.write(`${JSON.stringify(value)}\n`) };
}

const rpcChild = spawn(process.execPath, [
  join(packageRoot, "dist", "bundle", "cli.js"),
  "--mode", "rpc",
  "--no-session",
  "--no-context-files",
  "--no-skills",
  "--no-prompt-templates",
  "--no-themes",
  "--extension", companionPath,
  "--extension", fixturePath,
  "--approve",
], {
  cwd,
  env: { ...process.env, PI_CODING_AGENT_DIR: agentDir, PI_OFFLINE: "1" },
  stdio: ["pipe", "pipe", "pipe"],
});
const rpc = harness(rpcChild);
let rpcSequence = 0;
async function rpcCall(command) {
  const id = `rpc-${++rpcSequence}`;
  rpc.write({ id, ...command });
  return rpc.wait(record => record.type === "response" && record.id === id);
}

const helperChild = spawn(process.execPath, [
  helperPath,
  packageRoot,
  agentDir,
  cwd,
  "true",
], {
  cwd,
  env: { ...process.env, PI_CODING_AGENT_DIR: agentDir, PI_OFFLINE: "1" },
  stdio: ["pipe", "pipe", "pipe"],
});
const helper = harness(helperChild);

try {
  const commands = await rpcCall({ type: "get_commands" });
  assert.equal(commands.success, true);
  assert.ok(commands.data.commands.some(command => command.name === "__pie_native_auth_context_v1"));
  assert.ok(commands.data.commands.some(command => command.name === "__pie_native_auth_refresh_v1"));
  const messagesBeforeControls = await rpcCall({ type: "get_messages" });
  assert.equal(messagesBeforeControls.success, true);
  await rpcCall({ type: "prompt", message: "/__pie_native_auth_context_v1" });
  const contextRequest = await rpc.wait(record =>
    record.type === "extension_ui_request"
      && record.method === "setStatus"
      && record.statusKey === "pie-native-auth-v1"
      && JSON.parse(record.statusText).kind === "context");
  const context = JSON.parse(contextRequest.statusText);
  assert.equal(resolve(context.packageRoot), packageRoot);
  assert.equal(resolve(context.agentDir), agentDir);
  assert.equal(resolve(context.cwd), cwd);
  assert.equal(context.projectTrusted, true);
  const stateBeforeAuth = await rpcCall({ type: "get_state" });
  assert.equal(stateBeforeAuth.success, true);
  assert.ok(stateBeforeAuth.data.sessionId);
  assert.equal(context.sessionId, stateBeforeAuth.data.sessionId);

  await helper.wait(record => record.type === "ready");
  helper.write({ type: "catalog", requestId: "catalog-1", providerRef: "probe-factory", logout: false });
  const catalog = await helper.wait(record => record.type === "catalog" && record.requestId === "catalog-1");
  const provider = catalog.providers.find(value => value.id === "probe-factory");
  assert.ok(provider);
  assert.deepEqual(provider.methods.map(value => value.id), ["api_key"]);
  assert.equal(provider.removable, false);
  const modelLessProvider = catalog.providers.find(value => value.id === "probe-model-less");
  assert.ok(modelLessProvider);
  assert.deepEqual(modelLessProvider.methods.map(value => value.id), ["api_key"]);

  helper.write({
    type: "start",
    flowId: "flow-login",
    provider: "probe-factory",
    method: "api_key",
    logout: false,
  });
  const answers = { select: "multi", secret: "probe-secret", text: "probe-account" };
  for (const kind of ["select", "secret", "text"]) {
    const prompt = await helper.wait(record =>
      record.type === "prompt" && record.flowId === "flow-login" && record.kind === kind);
    helper.write({
      type: "reply",
      flowId: "flow-login",
      promptId: prompt.promptId,
      value: answers[kind],
    });
  }
  const login = await helper.wait(record => record.type === "outcome" && record.flowId === "flow-login");
  assert.equal(login.committed, true);
  assert.equal(login.outcome, "succeeded");

  await rpcCall({
    type: "prompt",
    message: `/__pie_native_auth_refresh_v1 ${JSON.stringify({
      flowId: "flow-login",
      provider: "probe-factory",
    })}`,
  });
  const refreshRequest = await rpc.wait(record =>
    record.type === "extension_ui_request"
      && record.method === "setStatus"
      && record.statusKey === "pie-native-auth-v1"
      && JSON.parse(record.statusText).kind === "refresh"
      && JSON.parse(record.statusText).flowId === "flow-login");
  assert.equal(JSON.parse(refreshRequest.statusText).success, true);
  assert.equal(JSON.parse(refreshRequest.statusText).remoteRefreshed, false);
  assert.match(JSON.parse(refreshRequest.statusText).remoteWarning, /offline/);
  const models = await rpcCall({ type: "get_available_models" });
  assert.ok(models.data.models.some(model => model.provider === "probe-factory"));
  const stateAfterAuth = await rpcCall({ type: "get_state" });
  assert.equal(stateAfterAuth.success, true);
  assert.equal(stateAfterAuth.data.sessionId, stateBeforeAuth.data.sessionId);
  const messagesAfterControls = await rpcCall({ type: "get_messages" });
  assert.deepEqual(messagesAfterControls.data.messages, messagesBeforeControls.data.messages);

  helper.write({ type: "catalog", requestId: "catalog-2", logout: true });
  const afterLogin = await helper.wait(record => record.type === "catalog" && record.requestId === "catalog-2");
  assert.equal(afterLogin.providers.find(value => value.id === "probe-factory").removable, true);
  helper.write({ type: "start", flowId: "flow-logout", provider: "probe-factory", logout: true });
  const logout = await helper.wait(record => record.type === "outcome" && record.flowId === "flow-logout");
  assert.equal(logout.committed, true);
  assert.equal(logout.outcome, "succeeded");

  helper.write({
    type: "start",
    flowId: "flow-manual",
    provider: "probe-factory",
    method: "api_key",
    logout: false,
  });
  const manualSelect = await helper.wait(record =>
    record.type === "prompt" && record.flowId === "flow-manual" && record.kind === "select");
  helper.write({
    type: "reply",
    flowId: "flow-manual",
    promptId: manualSelect.promptId,
    value: "manual",
  });
  const manualPrompt = await helper.wait(record =>
    record.type === "prompt" && record.flowId === "flow-manual" && record.kind === "manual_code");
  helper.write({
    type: "reply",
    flowId: "flow-manual",
    promptId: manualPrompt.promptId,
    value: "manual-sensitive-value",
  });
  const manualOutcome = await helper.wait(record =>
    record.type === "outcome" && record.flowId === "flow-manual");
  assert.equal(manualOutcome.outcome, "succeeded");
  helper.write({
    type: "start",
    flowId: "flow-manual-cleanup",
    provider: "probe-factory",
    logout: true,
  });
  const manualCleanup = await helper.wait(record =>
    record.type === "outcome" && record.flowId === "flow-manual-cleanup");
  assert.equal(manualCleanup.outcome, "succeeded");

  helper.write({
    type: "start",
    flowId: "flow-failure",
    provider: "probe-factory",
    method: "api_key",
    logout: false,
  });
  const failSelect = await helper.wait(record =>
    record.type === "prompt" && record.flowId === "flow-failure" && record.kind === "select");
  helper.write({ type: "reply", flowId: "flow-failure", promptId: failSelect.promptId, value: "fail" });
  const failSecret = await helper.wait(record =>
    record.type === "prompt" && record.flowId === "flow-failure" && record.kind === "secret");
  helper.write({
    type: "reply",
    flowId: "flow-failure",
    promptId: failSecret.promptId,
    value: "fixture-sensitive-value",
  });
  const failure = await helper.wait(record => record.type === "outcome" && record.flowId === "flow-failure");
  assert.equal(failure.committed, false);
  assert.equal(failure.outcome, "failed");
  assert.equal(JSON.stringify(failure).includes("fixture-sensitive-value"), false);
  assert.match(failure.message, /^Authentication failed;/);

  helper.write({
    type: "start",
    flowId: "flow-cancel",
    provider: "probe-factory",
    method: "api_key",
    logout: false,
  });
  const cancelSelect = await helper.wait(record =>
    record.type === "prompt" && record.flowId === "flow-cancel" && record.kind === "select");
  helper.write({ type: "reply", flowId: "flow-cancel", promptId: cancelSelect.promptId, value: "plain" });
  const cancelSecret = await helper.wait(record =>
    record.type === "prompt" && record.flowId === "flow-cancel" && record.kind === "secret");
  helper.write({ type: "cancel" });
  await helper.wait(record =>
    record.type === "prompt_withdrawn"
      && record.flowId === "flow-cancel"
      && record.promptId === cancelSecret.promptId);
  const cancelled = await helper.wait(record => record.type === "outcome" && record.flowId === "flow-cancel");
  assert.equal(cancelled.committed, false);
  assert.equal(cancelled.outcome, "cancelled");
  await new Promise(resolve => setTimeout(resolve, 50));
  const cancelRecords = helper.records.filter(record => record.flowId === "flow-cancel");
  assert.equal(cancelRecords.at(-1).type, "outcome");
  assert.equal(cancelRecords.filter(record => record.type === "prompt_withdrawn").length, 1);

  helper.write({
    type: "start",
    flowId: "flow-builtin-cancel",
    provider: "anthropic",
    method: "api_key",
    logout: false,
  });
  const builtinSecret = await helper.wait(record =>
    record.type === "prompt" && record.flowId === "flow-builtin-cancel" && record.kind === "secret");
  helper.write({ type: "cancel" });
  await helper.wait(record =>
    record.type === "prompt_withdrawn"
      && record.flowId === "flow-builtin-cancel"
      && record.promptId === builtinSecret.promptId);
  const builtinCancelled = await helper.wait(record =>
    record.type === "outcome" && record.flowId === "flow-builtin-cancel", 10_000);
  assert.equal(builtinCancelled.outcome, "cancelled");

  helper.write({
    type: "start",
    flowId: "flow-sync-failure",
    provider: "probe-factory",
    method: "api_key",
    logout: false,
  });
  const syncSelect = await helper.wait(record =>
    record.type === "prompt" && record.flowId === "flow-sync-failure" && record.kind === "select");
  helper.write({
    type: "reply",
    flowId: "flow-sync-failure",
    promptId: syncSelect.promptId,
    value: "sync_fail",
  });
  const syncSecret = await helper.wait(record =>
    record.type === "prompt" && record.flowId === "flow-sync-failure" && record.kind === "secret");
  helper.write({
    type: "reply",
    flowId: "flow-sync-failure",
    promptId: syncSecret.promptId,
    value: "sync-sensitive-value",
  });
  const syncFailure = await helper.wait(record =>
    record.type === "outcome" && record.flowId === "flow-sync-failure");
  assert.equal(syncFailure.committed, true);
  assert.equal(syncFailure.synchronized, false);
  assert.equal(syncFailure.outcome, "failed");
  assert.equal(JSON.stringify(syncFailure).includes("sync-sensitive-value"), false);
  helper.write({
    type: "start",
    flowId: "flow-sync-cleanup",
    provider: "probe-factory",
    logout: true,
  });
  const syncCleanup = await helper.wait(record =>
    record.type === "outcome" && record.flowId === "flow-sync-cleanup");
  assert.equal(syncCleanup.outcome, "succeeded");

  const ambientChild = spawn(process.execPath, [
    helperPath,
    packageRoot,
    agentDir,
    cwd,
    "true",
  ], {
    cwd,
    env: {
      ...process.env,
      PI_CODING_AGENT_DIR: agentDir,
      PI_OFFLINE: "1",
      PROBE_API_KEY: "ambient-fallback-key",
    },
    stdio: ["pipe", "pipe", "pipe"],
  });
  const ambient = harness(ambientChild);
  try {
    await ambient.wait(record => record.type === "ready");
    ambient.write({
      type: "catalog",
      requestId: "catalog-ambient",
      providerRef: "probe-factory",
      logout: false,
    });
    const ambientCatalog = await ambient.wait(record =>
      record.type === "catalog" && record.requestId === "catalog-ambient");
    const ambientProvider = ambientCatalog.providers.find(value => value.id === "probe-factory");
    assert.equal(ambientProvider.configured, true);
    assert.equal(ambientProvider.removable, false);
  } finally {
    ambient.write({ type: "shutdown" });
    await once(ambientChild, "exit");
  }

  const untrustedChild = spawn(process.execPath, [
    helperPath,
    packageRoot,
    untrustedAgentDir,
    untrustedCwd,
    "false",
  ], {
    cwd: untrustedCwd,
    env: {
      ...process.env,
      PI_CODING_AGENT_DIR: untrustedAgentDir,
      PI_OFFLINE: "1",
    },
    stdio: ["pipe", "pipe", "pipe"],
  });
  const untrusted = harness(untrustedChild);
  try {
    await untrusted.wait(record => record.type === "ready");
    untrusted.write({
      type: "catalog",
      requestId: "catalog-untrusted",
      providerRef: "probe-factory",
      logout: false,
    });
    const untrustedCatalog = await untrusted.wait(record =>
      record.type === "catalog" && record.requestId === "catalog-untrusted");
    assert.equal(untrustedCatalog.providers.some(value => value.id === "probe-factory"), false);
  } finally {
    untrusted.write({ type: "shutdown" });
    await once(untrustedChild, "exit");
  }

  console.log(JSON.stringify({
    companionContext: true,
    factoryProviderCatalog: true,
    modelLessProviderCatalog: true,
    nativePromptKinds: ["select", "secret", "text", "manual_code"],
    liveRuntimeRefresh: true,
    remoteCatalogReporting: true,
    storedCredentialRemoval: true,
    cancellation: true,
    diagnosticRedaction: true,
    environmentFallbackAfterLogout: true,
    untrustedProjectIsolation: true,
    committedSynchronizationFailure: true,
    liveSessionPreserved: true,
    controlTranscriptExcluded: true,
  }, null, 2));
} finally {
  helper.write({ type: "shutdown" });
  rpcChild.stdin.end();
  rpcChild.kill();
  setTimeout(() => helperChild.kill(), 1_000).unref();
}
