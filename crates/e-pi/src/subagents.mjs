import { spawn, execFile } from "node:child_process";
import { randomUUID } from "node:crypto";
import { readFileSync } from "node:fs";
import { basename, join } from "node:path";

const TOOL = "spawn_agent";
const MAX_ACTIVE = 3;
const SLOTS = 6;
const MAX_BYTES = 32 * 1024;
const LIMIT_MS = 15 * 60 * 1000;
const parameters = {
  type: "object", additionalProperties: false, required: ["label", "task"], properties: {
    label: { type: "string", description: "One short work word, e.g. Search or Build", maxLength: 20 },
    task: { type: "string", description: "Self-contained task. The child does not share this conversation." },
  },
};

function launch(root, pi, ctx, task) {
  // RPC entry points are not necessarily CLI entry points. Use this exact
  // installation's declared binary rather than resolving a second pi on PATH.
  const args = [];
  if (/^(node|bun)(\.exe)?$/i.test(basename(process.execPath))) {
    const bin = JSON.parse(readFileSync(join(root, "package.json"), "utf8")).bin;
    args.push(join(root, typeof bin === "string" ? bin : bin.pi));
  }
  const builtins = new Set(["read", "edit", "write", "bash", "powershell", "ls", "find", "grep"]);
  const tools = pi.getActiveTools().filter((name) => builtins.has(name));
  args.push("--print", "--mode", "text", "--no-session", "--provider", ctx.model.provider,
    "--model", ctx.model.id, "--thinking", pi.getThinkingLevel(),
    ctx.isProjectTrusted() ? "--approve" : "--no-approve", ...(tools.length ? ["--tools", tools.join(",")] : ["--no-tools"]));
  const child = spawn(process.execPath, args, { cwd: ctx.cwd, windowsHide: true, stdio: ["pipe", "pipe", "ignore"] });
  const buffer = Buffer.alloc(MAX_BYTES);
  let used = 0, truncated = false, stopped = false, timedOut = false, inputError = false;
  child.stdout.on("data", (chunk) => {
    const copied = chunk.copy(buffer, used);
    used += copied;
    truncated ||= copied < chunk.length;
  });
  let force;
  const stop = () => {
    if (stopped || !child.pid || child.exitCode !== null || child.signalCode !== null) return;
    stopped = true;
    if (process.platform === "win32") {
      execFile("taskkill.exe", ["/PID", `${child.pid}`, "/T", "/F"], { windowsHide: true, timeout: 3000 }, () => {});
    } else child.kill("SIGTERM"); // Pi disposes its own tracked shell children.
    force = setTimeout(() => child.kill("SIGKILL"), 3500);
  };
  child.stdin.on("error", () => { inputError = true; stop(); });
  child.stdin.end(`Delegated task:\n${task}`);
  const deadline = setTimeout(() => { timedOut = true; stop(); }, LIMIT_MS);
  const result = new Promise((resolve) => {
    child.once("error", () => resolve({ status: "failed", text: "Could not launch Pi." }));
    child.once("close", (code) => {
      const text = buffer.toString("utf8", 0, used).trim();
      resolve(code === 0 && text && !inputError && !timedOut
        ? { status: "done", text: text + (truncated ? "\n[Result truncated]" : "") }
        : { status: "failed", text: timedOut ? "Child timed out." : `Child returned no usable answer (exit ${code ?? "signal"}).` });
    });
  }).finally(() => { clearTimeout(deadline); clearTimeout(force); });
  return { stop, result };
}

export function createDelegation(pi, publish, root) {
  let enabled = false, registered = false;
  const tasks = new Map();
  const running = () => [...tasks.values()].filter((t) => t.status === "running");
  const retire = async () => {
    const pending = running();
    tasks.clear(); // Fence completions before signalling processes.
    for (const t of pending) t.child.stop();
    await Promise.allSettled(pending.map((t) => t.child.result));
  };
  pi.on("message_start", (event, ctx) => {
    if (event.message.role !== "user") return;
    let changed = false;
    for (const [id, t] of tasks) if (t.status !== "running") { tasks.delete(id); changed = true; }
    if (changed) publish(ctx);
  });
  const definition = {
    name: TOOL, label: "Delegate", parameters,
    description: "Delegate an independent task to a background Pi agent in the same workspace. Returns an ID immediately. The result arrives automatically; do not poll or assign conflicting writes.",
    async execute(_call, { label, task }, signal, _update, ctx) {
      if (!enabled || signal?.aborted) throw new Error("Delegation is disabled or interrupted.");
      if (typeof label !== "string" || !/^[\p{L}][\p{L}\p{N}]{0,19}$/u.test(label) || typeof task !== "string" || !task.trim() || Buffer.byteLength(task) > MAX_BYTES)
        throw new Error("Supply a one-word label (up to 20 characters) and a nonempty task up to 32 KiB.");
      if (!ctx.model) throw new Error("No parent model is selected.");
      if (running().length >= MAX_ACTIVE) throw new Error("Three child agents are already working.");
      if (tasks.size === SLOTS) tasks.delete([...tasks.values()].find((t) => t.status !== "running").id);
      let slot = 0;
      while ([...tasks.values()].some((t) => t.slot === slot)) slot++;
      const owner = ctx.sessionManager.getSessionId();
      const id = randomUUID();
      const child = launch(root, pi, ctx, task);
      const entry = { id, label, slot, status: "running", child };
      tasks.set(id, entry);
      publish(ctx);
      void child.result.then(({ status, text }) => {
        if (!enabled || tasks.get(id) !== entry || ctx.sessionManager.getSessionId() !== owner) return;
        entry.status = status;
        delete entry.child;
        publish(ctx);
        pi.sendMessage({ customType: "pie-subagent-result", display: false,
          content: `Child ${label} (${id}) ${status}:\n${text}`, details: { id, label, status } },
        { deliverAs: "followUp", triggerTurn: true });
      }).catch(() => ctx.ui.notify("Could not deliver the child result.", "error"));
      return { content: [{ type: "text", text: `Started ${label}: ${id}. Its result will arrive automatically.` }], details: { id } };
    },
  };
  return {
    get enabled() { return enabled; },
    entries: () => [...tasks.values()].map(({ slot, label, status }) => ({ slot, label, status })),
    enable() {
      if (enabled) return;
      if (!registered && pi.getAllTools().some((t) => t.name === TOOL)) throw new Error(`${TOOL} is already registered.`);
      pi.registerTool({ ...definition, exposure: "direct" });
      if (!pi.getActiveTools().includes(TOOL)) pi.setActiveTools([...pi.getActiveTools(), TOOL]);
      registered = enabled = true;
    },
    async disable(ctx) {
      enabled = false;
      const cleaned = retire();
      try {
        pi.registerTool({ ...definition, exposure: "hidden" });
        pi.setActiveTools(pi.getActiveTools().filter((n) => n !== TOOL));
      } finally {
        publish(ctx);
        await cleaned;
      }
    },
    shutdown() { enabled = false; return retire(); },
  };
}
