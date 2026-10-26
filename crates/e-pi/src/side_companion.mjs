import { Worker } from "node:worker_threads";
import { mkdtemp, rm } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { getPackageDir } from "@earendil-works/pi-coding-agent";

const COMMAND = "__pie_side_v1";
const STATUS = "pie-side-v1";
const MAX_CHOICES = 512;

function text(message) {
  return typeof message.content === "string" ? message.content :
    (message.content ?? []).filter((part) => part.type === "text").map((part) => part.text).join("");
}

function prepare(request, workers) {
  return new Promise((resolve, reject) => {
    const worker = new Worker(new URL("./pie-side-helper.mjs", import.meta.url), {
      workerData: { request, packageRoot: getPackageDir() },
      resourceLimits: { maxOldGenerationSizeMb: 256 },
      stdout: true, stderr: true,
    });
    workers.add(worker);
    worker.stdout.resume();
    worker.stderr.resume();
    const timer = setTimeout(() => { void worker.terminate(); reject(new Error("Side session preparation timed out")); }, 15000);
    worker.on("message", (result) => {
      if (result.error) reject(new Error(result.error));
      else resolve(result);
    });
    worker.on("error", reject);
    worker.on("exit", () => {
      clearTimeout(timer);
      workers.delete(worker);
      reject(new Error("Side session worker stopped before returning a result"));
    });
  });
}

export default function registerSideControls(pi) {
  let pending = null;
  const workers = new Set();
  const artifacts = new Map();
  const discard = async (id) => {
    const directory = artifacts.get(id);
    if (directory) { await rm(directory, { recursive: true, force: true }); artifacts.delete(id); }
  };
  pi.on("session_shutdown", async () => {
    await Promise.all([...workers].map((worker) => worker.terminate()));
    workers.clear();
    await Promise.all([...artifacts.keys()].map(discard));
  });
  pi.registerCommand(COMMAND, {
    description: "Internal pie side session preparation",
    handler: async (args, ctx) => {
      let id;
      const publish = (value) => ctx.ui.setStatus(STATUS, JSON.stringify({ protocol: 1, id, ...value }));
      try {
        const request = JSON.parse(args);
        id = request.id;
        if (request.kind === "cancel") {
          if (pending?.id === request.target) {
            pending.controller.abort();
            await Promise.all([...workers].map((worker) => worker.terminate()));
          }
          await discard(request.target);
          return;
        }
        if (typeof id !== "string" || id.length > 128 || !["new", "clone", "fork"].includes(request.kind)) {
          throw new Error("Invalid side session request");
        }
        if (pending) throw new Error("A side session is already being prepared");
        const operation = { id, controller: new AbortController() };
        pending = operation;
        let offered = false;
        try {
          await Promise.all([...artifacts.keys()].map(discard));
          const manager = ctx.sessionManager;
          const sourceId = manager.getSessionId();
          const source = manager.getSessionFile();
          const cwd = ctx.cwd;
          const directory = manager.getSessionDir();
          const model = ctx.model ? { provider: ctx.model.provider, model: ctx.model.id, effort: pi.getThinkingLevel() } : undefined;
          let leaf = null;
          let editor = "";
          if (request.kind !== "new") {
            const branch = manager.getBranch();
            if (branch.length > 100000) throw new Error("Session is too large to prepare a side snapshot");
            // Only a completed assistant response or a completed compaction is a continuation checkpoint.
            for (const entry of branch) {
              if (entry.type === "compaction" || (entry.type === "message" && entry.message.role === "assistant" &&
                  entry.message.stopReason === "stop" && !entry.message.content.some((part) => part.type === "toolCall"))) leaf = entry.id;
            }
            if (request.kind === "fork") {
              const choices = branch.filter((entry) => entry.type === "message" && entry.message.role === "user").slice(-MAX_CHOICES).reverse();
              if (!choices.length) throw new Error("No user messages are available to fork");
              const labels = choices.map((entry) => `${entry.id}  ${text(entry.message).replace(/\s+/g, " ").slice(0, 120)}`);
              publish({ outcome: "selecting" });
              const selected = await ctx.ui.select("Fork into side session", labels, { signal: operation.controller.signal });
              if (selected === undefined) { publish({ outcome: "cancelled" }); return; }
              const entry = choices[labels.indexOf(selected)];
              if (!entry) throw new Error("Invalid fork selection");
              leaf = entry.parentId;
              editor = text(entry.message);
              if (Buffer.byteLength(editor) > 256 * 1024) throw new Error("Selected user message is too large for the side editor");
            }
          }
          if (manager.getSessionId() !== sourceId) throw new Error("The source session changed during side preparation");
          const parent = request.kind === "new" ? undefined : source;
          publish({ outcome: "preparing" });
          const scratch = await mkdtemp(join(dirname(fileURLToPath(import.meta.url)), "side-"));
          artifacts.set(id, scratch);
          if (operation.controller.signal.aborted) { publish({ outcome: "cancelled" }); return; }
          const result = await prepare({ source, sourceId, cwd, directory: scratch, leaf, parent }, workers);
          if (operation.controller.signal.aborted || manager.getSessionId() !== sourceId) { publish({ outcome: "cancelled" }); return; }
          if (!result.persisted) await discard(id);
          publish({ outcome: "ready", ...result, directory, cwd, model, editor, parent });
          offered = true;
        } finally {
          if (!offered) {
            await Promise.all([...workers].map((worker) => worker.terminate()));
            await discard(id);
          }
          if (pending === operation) pending = null;
        }
      } catch (error) { publish({ outcome: "error", message: error instanceof Error ? error.message : "Side session preparation failed" }); }
    },
  });
}
