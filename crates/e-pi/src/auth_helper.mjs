import { createInterface } from "node:readline";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const [packageRoot, agentDir, cwd, trustedText] = process.argv.slice(2);
if (!packageRoot || !agentDir || !cwd || !trustedText) {
  throw new Error("missing authentication helper context");
}

const MAX_TEXT = 16_384;
const MAX_RECORD_BYTES = 256 * 1024;
const MAX_BUFFERED_BYTES = MAX_RECORD_BYTES * 64;
function write(value) {
  const line = `${JSON.stringify(value)}\n`;
  const bytes = Buffer.byteLength(line, "utf8");
  if (bytes > MAX_RECORD_BYTES) {
    throw new Error("authentication helper record exceeds 256 KiB");
  }
  if (process.stdout.writableLength + bytes > MAX_BUFFERED_BYTES) {
    throw new Error("authentication helper output capacity exceeded");
  }
  process.stdout.write(line);
}
const diagnostic = () => {
  process.stderr.write("Pi authentication helper diagnostic\n");
};
console.log = diagnostic;
console.info = diagnostic;
console.warn = diagnostic;
console.error = diagnostic;

const sdkUrl = pathToFileURL(join(packageRoot, "dist", "index.js")).href;
const { createAgentSessionServices, CredentialSynchronizationError } = await import(sdkUrl);
const services = await createAgentSessionServices({
  cwd,
  agentDir,
  resourceLoaderOptions: {
    noSkills: true,
    noPromptTemplates: true,
    noThemes: true,
    noContextFiles: true,
  },
  resourceLoaderReloadOptions: {
    resolveProjectTrust: async () => trustedText === "true",
  },
});
const runtime = services.modelRuntime;
const active = new Map();
const replies = new Map();
let promptSequence = 0;

function bounded(value, fallback = "") {
  return String(value ?? fallback).slice(0, MAX_TEXT);
}

async function catalog(request) {
  const credentials = await runtime.listCredentials();
  const stored = new Set(credentials.map(value => value.providerId));
  const providers = runtime.getProviders().slice(0, 256).map(provider => {
    const methods = [];
    if (provider.auth?.apiKey?.login) {
      methods.push({
        id: "api_key",
        name: bounded(provider.auth.apiKey.name, "API key"),
      });
    }
    if (provider.auth?.oauth?.login) {
      methods.push({
        id: "oauth",
        name: bounded(provider.auth.oauth.loginLabel ?? provider.auth.oauth.name, "OAuth"),
      });
    }
    const status = runtime.getProviderAuthStatus(provider.id);
    return {
      id: bounded(provider.id),
      name: bounded(provider.name, provider.id),
      methods,
      configured: Boolean(status.configured),
      removable: stored.has(provider.id),
      source: status.label ? bounded(status.label) : undefined,
    };
  });
  write({
    type: "catalog",
    requestId: request.requestId,
    providerRef: request.providerRef,
    logout: Boolean(request.logout),
    providers,
  });
}

function prompt(flowId, flow, value) {
  const promptId = `${flowId}-prompt-${++promptSequence}`;
  const kind = value.type;
  write({
    type: "prompt",
    flowId,
    promptId,
    kind,
    message: bounded(value.message),
    placeholder: value.placeholder ? bounded(value.placeholder) : undefined,
    options: value.type === "select"
      ? value.options.slice(0, 128).map(option => ({
          value: bounded(option.id),
          label: bounded(option.label, option.id),
          description: option.description ? bounded(option.description) : undefined,
        }))
      : [],
  });
  return new Promise((resolve, reject) => {
    let settled = false;
    const finish = (fn, result) => {
      if (settled) return;
      settled = true;
      replies.delete(promptId);
      value.signal?.removeEventListener("abort", abort);
      flow.controller.signal.removeEventListener("abort", abort);
      fn(result);
    };
    const abort = () => {
      if (settled) return;
      write({ type: "prompt_withdrawn", flowId, promptId });
      finish(reject, new Error("Login cancelled"));
    };
    replies.set(promptId, {
      flowId,
      resolve(answer) {
        finish(resolve, answer);
      },
    });
    value.signal?.addEventListener("abort", abort, { once: true });
    flow.controller.signal.addEventListener("abort", abort, { once: true });
    if (value.signal?.aborted || flow.controller.signal.aborted) abort();
  });
}

function notify(flowId, event) {
  switch (event.type) {
    case "auth_url":
      write({
        type: "notice",
        flowId,
        kind: "authorization_url",
        message: bounded(event.instructions, "Open this URL to continue"),
        url: bounded(event.url),
      });
      break;
    case "device_code":
      write({
        type: "notice",
        flowId,
        kind: "device_code",
        message: "Open the verification URL and enter the device code",
        url: bounded(event.verificationUri),
        code: bounded(event.userCode),
      });
      break;
    case "info": {
      const links = event.links?.slice(0, 16) ?? [];
      write({
        type: "notice",
        flowId,
        kind: "information",
        message: bounded(event.message),
        url: links[0]?.url ? bounded(links[0].url) : undefined,
      });
      for (const link of links.slice(1)) {
        write({
          type: "notice",
          flowId,
          kind: "information",
          message: bounded(link.label, "Provider link"),
          url: bounded(link.url),
        });
      }
      break;
    }
    case "progress":
      write({ type: "notice", flowId, kind: "progress", message: bounded(event.message) });
      break;
  }
}

async function mutate(request) {
  if (active.size > 0) {
    write({
      type: "outcome",
      flowId: request.flowId,
      provider: request.provider,
      committed: false,
      synchronized: false,
      outcome: "failed",
      message: "Another authentication operation is already active",
    });
    return;
  }
  const controller = new AbortController();
  const flow = { controller };
  active.set(request.flowId, flow);
  try {
    if (request.logout) {
      await runtime.logout(request.provider, { signal: controller.signal });
    } else {
      await runtime.login(request.provider, request.method, {
        signal: controller.signal,
        prompt: value => prompt(request.flowId, flow, value),
        notify: event => notify(request.flowId, event),
      });
    }
    write({
      type: "outcome",
      flowId: request.flowId,
      provider: request.provider,
      committed: true,
      synchronized: true,
      outcome: "succeeded",
      message: request.logout ? "Credential removed" : "Authentication completed",
    });
  } catch (error) {
    const cancelled = controller.signal.aborted || error?.message === "Login cancelled";
    const committed = error instanceof CredentialSynchronizationError;
    write({
      type: "outcome",
      flowId: request.flowId,
      provider: request.provider,
      committed,
      synchronized: false,
      outcome: cancelled ? "cancelled" : "failed",
      message: cancelled
        ? "Authentication cancelled"
        : committed
          ? "Credential changed, but Pi could not synchronize provider state"
          : "Authentication failed; retry or use native pi /login for provider diagnostics",
    });
  } finally {
    active.delete(request.flowId);
  }
}

write({ type: "ready", protocol: 1 });
const lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
for await (const line of lines) {
  if (Buffer.byteLength(line, "utf8") > MAX_RECORD_BYTES) {
    throw new Error("authentication helper record exceeds 256 KiB");
  }
  let request;
  try {
    request = JSON.parse(line);
  } catch {
    throw new Error("invalid authentication helper JSON");
  }
  switch (request.type) {
    case "catalog":
      void catalog(request).catch(() => write({
        type: "catalog",
        requestId: request.requestId,
        providerRef: request.providerRef,
        logout: Boolean(request.logout),
        providers: [],
        error: "Could not load Pi authentication providers; restart pie and retry",
      }));
      break;
    case "start":
      void mutate(request);
      break;
    case "reply": {
      const reply = replies.get(request.promptId);
      if (reply?.flowId === request.flowId) reply.resolve(bounded(request.value));
      break;
    }
    case "cancel":
      for (const flow of active.values()) flow.controller.abort();
      break;
    case "shutdown":
      for (const flow of active.values()) flow.controller.abort();
      lines.close();
      break;
  }
}
