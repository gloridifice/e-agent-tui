import { createHash, randomUUID } from "node:crypto";
import {
  closeSync, constants, fsyncSync, lstatSync, openSync, readFileSync,
  renameSync, unlinkSync, writeFileSync,
} from "node:fs";
import { join } from "node:path";
import * as piSdk from "@earendil-works/pi-coding-agent";
const { getAgentDir, VERSION } = piSdk;

const KEY = "pie-mcp-v1";
const LIMIT = 256 * 1024;
const EXPOSURES = ["codemode", "deferred", "direct", "hidden"];
const record = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
const exposure = (value) => value === "codemode-deferred" ? "codemode" : (value === undefined ? "codemode" : value);
const namespace = (name) => `mcp__${name.replaceAll("-", "_")}`;
const segmenter = new Intl.Segmenter(undefined, { granularity: "grapheme" });
function text(value, max = 2048) {
  if (typeof value !== "string") return "";
  value = value.toWellFormed();
  if (value.length <= max) return value;
  let result = "";
  for (const { segment } of segmenter.segment(value)) {
    if (result.length + segment.length > max) break;
    result += segment;
  }
  return `${result}…`;
}

function readFile(path) {
  try {
    const stat = lstatSync(path);
    if (!stat.isFile() || stat.size > LIMIT) throw new Error();
    const raw = readFileSync(path, "utf8");
    if (Buffer.byteLength(raw) > LIMIT) throw new Error();
    const json = JSON.parse(raw);
    if (!record(json) || (json.mcpServers !== undefined && !record(json.mcpServers))) throw new Error();
    return { path, raw, json, mode: stat.mode & 0o777 };
  } catch (error) {
    if (error.code === "ENOENT") return { path, raw: "", json: {} };
    return { path, raw: "", json: {}, error: "Cannot read MCP configuration: invalid, oversized or non-regular file." };
  }
}

const strings = (value) => record(value) && Object.values(value).every((item) => typeof item === "string");
const loopback = (url) => ["localhost", "127.0.0.1", "[::1]"].includes(url.hostname);
const secure = (url) => url.protocol === "https:" || (url.protocol === "http:" && loopback(url));
function validOAuth(value) {
  if (value === undefined) return true;
  if (!record(value) || ["clientId", "clientSecret", "scope", "clientName"].some((key) => value[key] !== undefined && typeof value[key] !== "string")) return false;
  if (value.clientName !== undefined && !value.clientName.trim()) return false;
  if (value.callbackPort !== undefined && (!Number.isInteger(value.callbackPort) || value.callbackPort < 1 || value.callbackPort > 65535)) return false;
  let callback;
  if (value.callbackUrl !== undefined) {
    if (typeof value.callbackUrl !== "string" || !URL.canParse(value.callbackUrl)) return false;
    callback = new URL(value.callbackUrl);
    if (callback.protocol !== "http:" || !loopback(callback) || callback.search || callback.hash
      || (callback.port && value.callbackPort !== undefined && Number(callback.port) !== value.callbackPort)) return false;
  }
  if (value.clientRegistration !== undefined && !["dcr", "cimd"].includes(value.clientRegistration)) return false;
  if (value.clientRegistration === "cimd" && (value.clientId !== undefined || value.clientName !== undefined
    || (callback && (callback.hostname === "[::1]" || callback.pathname !== "/callback")))) return false;
  return value.authServerMetadataUrl === undefined || (typeof value.authServerMetadataUrl === "string"
    && URL.canParse(value.authServerMetadataUrl) && secure(new URL(value.authServerMetadataUrl)));
}
function valid(name, config) {
  if (!/^[A-Za-z0-9_-]+$/.test(name) || !record(config)
    || (config.description !== undefined && typeof config.description !== "string")
    || (config.timeout !== undefined && !(typeof config.timeout === "number" && config.timeout > 0))
    || (config.enabled !== undefined && typeof config.enabled !== "boolean")
    || !EXPOSURES.includes(exposure(config.exposure))
    || (config.toolExposure !== undefined && (!record(config.toolExposure)
      || !Object.values(config.toolExposure).every((mode) => EXPOSURES.includes(exposure(mode)))))) return false;
  if (typeof config.url === "string" && [undefined, "http", "streamable-http"].includes(config.type)) {
    if (!URL.canParse(config.url) || !["https:", "http:"].includes(new URL(config.url).protocol)
      || (config.headers !== undefined && !strings(config.headers)) || !validOAuth(config.oauth)) return false;
    return config.auth === undefined || (record(config.auth) && typeof config.auth.provider === "string"
      && config.auth.provider.length > 0 && secure(new URL(config.url)));
  }
  return typeof config.command === "string" && [undefined, "stdio"].includes(config.type)
    && (config.args === undefined || (Array.isArray(config.args) && config.args.every((value) => typeof value === "string")))
    && (config.env === undefined || strings(config.env))
    && (config.cwd === undefined || typeof config.cwd === "string");
}

function configView(pi, ctx) {
  const files = [readFile(join(getAgentDir(), "mcp.json"))];
  if (ctx.isProjectTrusted()) files.push(readFile(join(ctx.cwd, ".pi", "mcp.json")));
  const hash = createHash("sha256");
  const errors = [];
  const servers = new Map();
  // Partial project overrides were introduced after Pi 1.0.
  const parts = VERSION.split(".").map(Number);
  const overrides = parts[0] > 1 || (parts[0] === 1 && parts[1] >= 1);
  for (const [index, file] of files.entries()) {
    hash.update(file.path).update("\0").update(file.raw).update("\0");
    if (file.error) errors.push(file.error);
    if (file.json.autoEnableCodemode !== undefined && typeof file.json.autoEnableCodemode !== "boolean") errors.push("MCP autoEnableCodemode must be a boolean; fix configuration before editing.");
    for (const [name, raw] of Object.entries(file.json.mcpServers ?? {})) {
      const base = servers.get(name);
      const partial = overrides && index === 1 && record(raw)
        && raw.command === undefined && raw.url === undefined && raw.type === undefined;
      const config = partial && base ? { ...base.config, ...raw } : raw;
      if (!valid(name, config) || (partial && (!base
        || Object.keys(raw).some((key) => !["enabled", "exposure", "toolExposure"].includes(key))))
        || (index === 1 && config.auth && !partial)
        || [...servers.keys()].some((other) => other !== name && namespace(other) === namespace(name))) {
        errors.push("An invalid or conflicting MCP server entry was skipped; fix configuration before editing.");
        continue;
      }
      servers.set(name, { name, config, file, source: partial ? "project override" : index === 1 ? "project" : "user" });
    }
  }
  for (const item of pi.getMcpServers()) {
    if (!servers.has(item.name) && ![...servers.keys()].some((other) => namespace(other) === namespace(item.name))) {
      servers.set(item.name, { name: item.name, config: item.config, source: "extension" });
    }
  }
  if (servers.size > 256) throw new Error("Too many MCP servers to inspect safely.");
  return { files, servers, revision: hash.digest("hex"), errors: [...new Set(errors)] };
}

function snapshot(pi, ctx, view) {
  const tools = pi.getAllTools().filter((tool) => tool.sourceInfo?.path === "builtin:mcp");
  let toolBudget = 128;
  const result = {
    revision: view.revision,
    trusted: ctx.isProjectTrusted(),
    errors: view.errors,
    exposures: [
      { id: "codemode", name: "Codemode", description: "Discover and call from scripts" },
      { id: "deferred", name: "Deferred", description: "Discover through tool search" },
      { id: "direct", name: "Direct", description: "Declare tools to the model" },
      { id: "hidden", name: "Hidden", description: "Keep tools unreachable" },
    ],
    servers: [...view.servers.values()].map((server) => {
      const own = tools.filter((tool) => tool.namespace?.name === namespace(server.name));
      const shown = own.slice(0, toolBudget);
      toolBudget -= shown.length;
      return {
        name: server.name,
        enabled: server.config.enabled !== false,
        exposure: exposure(server.config.exposure),
        source: server.source,
        writable: Boolean(server.file) && view.errors.length === 0,
        transport: server.config.command ? "stdio" : "HTTP",
        description: text(server.config.description),
        overrides: Object.keys(server.config.toolExposure ?? {}).length,
        toolCount: own.length,
        tools: shown.map((tool) => ({
          name: text(tool.name, 512),
          description: text(tool.description),
          exposure: text(tool.exposure, 32),
          schema: text(JSON.stringify(tool.parameters), 8192),
          annotations: Object.entries(tool.annotations ?? {})
            .filter(([key, value]) => ["readOnlyHint", "destructiveHint", "idempotentHint", "openWorldHint"].includes(key)
              && typeof value === "boolean")
            .map(([key, value]) => `${key}: ${value}`).join(" · "),
        })),
      };
    }),
  };
  // Keep the control record bounded without silently hiding its tool count.
  while (Buffer.byteLength(JSON.stringify(result)) > LIMIT - 2048) {
    const largest = result.servers.reduce((a, b) => b.tools.length > (a?.tools.length ?? 0) ? b : a, undefined);
    if (!largest?.tools.length) throw new Error("MCP metadata exceeds the display limit.");
    largest.tools.pop();
  }
  return result;
}

function save(view, request) {
  if (view.errors.length || request.revision !== view.revision) {
    throw new Error("MCP configuration changed or is invalid. Refresh the menu before saving.");
  }
  const server = view.servers.get(request.server);
  const patch = request.patch;
  if (!server?.file || !record(patch) || Object.keys(patch).length === 0
    || Object.keys(patch).some((key) => !["enabled", "exposure"].includes(key))
    || (patch.enabled !== undefined && typeof patch.enabled !== "boolean")
    || (patch.exposure !== undefined && !EXPOSURES.includes(patch.exposure))) {
    throw new Error("This MCP server or setting cannot be changed here.");
  }
  const file = server.file;
  const raw = file.json.mcpServers[request.server];
  const keepDefaults = raw.command === undefined && raw.url === undefined && raw.type === undefined;
  if (patch.enabled !== undefined) {
    if (patch.enabled && !keepDefaults) delete raw.enabled;
    else raw.enabled = patch.enabled;
  }
  if (patch.exposure !== undefined) {
    if (patch.exposure === "codemode" && !keepDefaults) delete raw.exposure;
    else raw.exposure = patch.exposure;
  }
  const indent = /^([ \t]+)\S/m.exec(file.raw)?.[1] ?? "  ";
  const encoded = `${JSON.stringify(file.json, null, indent)}\n`;
  if (Buffer.byteLength(encoded) > LIMIT) throw new Error("MCP configuration exceeds the save limit.");
  const temp = `${file.path}.pie-${randomUUID()}.tmp`;
  let fd;
  try {
    fd = openSync(temp, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL, file.mode ?? 0o600);
    writeFileSync(fd, encoded);
    fsyncSync(fd);
    closeSync(fd);
    fd = undefined;
    if (readFileSync(file.path, "utf8") !== file.raw) {
      throw new Error("MCP configuration changed during save. Refresh and try again.");
    }
    renameSync(temp, file.path);
  } catch (error) {
    if (error.message.startsWith("MCP configuration changed")) throw error;
    throw new Error("Could not save MCP configuration; the change was not confirmed.");
  } finally {
    if (fd !== undefined) closeSync(fd);
    try { unlinkSync(temp); } catch { /* Already replaced or never created. */ }
  }
}

export default function registerMcpControls(pi) {
  const publish = (ctx, value) => ctx.ui.setStatus(KEY, JSON.stringify({
    protocol: 1, sessionId: ctx.sessionManager.getSessionId(), ...value,
  }));
  for (const operation of ["get", "save", "open"]) {
    pi.registerCommand(`__pie_mcp_${operation}_v1`, {
      description: `Internal pie MCP ${operation}`,
      handler: async (args, ctx) => {
        let id = "";
        let saved = false;
        try {
          const request = JSON.parse(args);
          if (typeof request.id !== "string" || request.id.length > 128
            || request.sessionId !== ctx.sessionManager.getSessionId()) throw new Error("Stale MCP request.");
          id = request.id;
          if (pi.getCommands().find((command) => command.name === "mcp")?.sourceInfo?.path !== "builtin:mcp") {
            throw new Error("Native MCP support is unavailable or replaced by an extension.");
          }
          if (operation === "open") {
            const url = new URL(request.url);
            if (!["https:", "http:"].includes(url.protocol) || url.username || url.password) throw new Error("MCP authorization link rejected.");
            const [command, argv] = process.platform === "win32"
              ? ["rundll32.exe", ["url.dll,FileProtocolHandler", url.href]]
              : process.platform === "darwin" ? ["open", [url.href]] : ["xdg-open", [url.href]];
            const result = await pi.exec(command, argv, { timeout: 10000 });
            publish(ctx, { id, kind: operation, success: result.code === 0 });
            return;
          }
          const view = configView(pi, ctx);
          if (operation === "save") {
            const commit = async () => {
              if (request.sessionId !== ctx.sessionManager.getSessionId()) throw new Error("Stale MCP request.");
              save(configView(pi, ctx), request);
              saved = true;
            };
            const path = view.servers.get(request.server)?.file?.path;
            if (path && typeof piSdk.withFileMutationQueue === "function") await piSdk.withFileMutationQueue(path, commit);
            else await commit();
          }
          publish(ctx, {
            id, kind: operation, success: true, saved,
            busy: request.deferReload === true || !ctx.isIdle(),
            snapshot: snapshot(pi, ctx, saved ? configView(pi, ctx) : view),
          });
        } catch (error) {
          publish(ctx, {
            id, kind: operation, success: false, saved,
            error: saved ? "MCP configuration saved, but its display could not refresh. Run /reload."
              : error.message.startsWith("MCP") || error.message.startsWith("Native MCP")
                || error.message.startsWith("This MCP") || error.message === "Stale MCP request."
                ? error.message : "Could not inspect MCP configuration.",
          });
        }
      },
    });
  }
}
