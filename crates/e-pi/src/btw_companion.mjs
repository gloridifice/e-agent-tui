import { randomUUID } from "node:crypto";
import { buildSessionContext, convertToLlm } from "@earendil-works/pi-coding-agent";

// Use Pi's finalized, compaction-aware projection, never its streaming buffer.
// Describe past tool interactions as evidence, not executable calls. This also
// works with providers that reject tool history when no tools are declared.
function snapshot(manager) {
  const context = manager.buildSessionProjection?.() ?? buildSessionContext(manager.getBranch());
  const history = convertToLlm(context.messages);
  const messages = [];
  const calls = new Map();
  for (const msg of history) {
    if (msg.role === "system") continue; // May carry the parent's tool declarations.
    if (msg.role === "assistant") {
      calls.clear();
      if (["aborted", "error"].includes(msg.stopReason)) continue;
      for (const part of msg.content) if (part.type === "toolCall") calls.set(part.id, part);
      const content = msg.content.filter(p => p.type !== "toolCall");
      if (content.length) messages.push({ ...msg, content, stopReason: "stop" });
    } else if (msg.role === "toolResult") {
      const call = calls.get(msg.toolCallId);
      const args = call ? ` ${JSON.stringify(call.arguments)}` : "";
      messages.push({ role: "user", timestamp: msg.timestamp, content: [
        { type: "text", text: `Historical tool result (${msg.toolName}${args}):` }, ...msg.content,
      ] });
      calls.delete(msg.toolCallId);
    } else {
      messages.push(msg);
    }
  }
  return structuredClone(messages); // Providers must not receive live session-owned objects.
}

export default function registerBtw(pi) {
  const active = new Set();
  pi.on("session_shutdown", () => {
    for (const controller of active) controller.abort();
    active.clear();
  });
  pi.registerCommand("__pie_btw_v1", {
    description: "Internal pie one-shot question",
    handler: async (args, ctx) => {
      const { id, sessionId, question } = JSON.parse(args);
      if (typeof id !== "string" || !/^pie-btw-\d+$/.test(id) || sessionId !== ctx.sessionManager.getSessionId()) return;
      const publish = (ok, text) => ctx.ui.setStatus("pie-btw-v1", JSON.stringify({ id, sessionId, ok, text }));
      if (!ctx.model || typeof ctx.modelRegistry.streamSimple !== "function") return publish(false, "Side questions require a selected model and Pi model registry streaming support.");
      if (typeof question !== "string" || !question.trim() || Buffer.byteLength(question) > 65536 || active.size >= 3) return publish(false, "Invalid or busy side question request.");
      const controller = new AbortController();
      active.add(controller);
      const timer = setTimeout(() => controller.abort(), 5 * 60_000);
      try {
        const messages = snapshot(ctx.sessionManager);
        messages.push({ role: "user", content: question, timestamp: Date.now() });
        const thinking = pi.getThinkingLevel();
        const stream = ctx.modelRegistry.streamSimple(ctx.model, {
          systemPrompt: `${ctx.getSystemPrompt()}\n\nAnswer only the final side question using the preceding conversation as context. Do not continue its task. You have no tools.`,
          messages, tools: [],
        }, { reasoning: thinking === "off" ? undefined : thinking, sessionId: randomUUID(), cacheRetention: "none", signal: controller.signal });
        // A custom provider may ignore cancellation. Do not hold the command
        // (or a concurrency slot) waiting for it after timeout or shutdown.
        const result = await Promise.race([stream.result(), new Promise((_, reject) => {
          controller.signal.throwIfAborted();
          controller.signal.addEventListener("abort", () => reject(controller.signal.reason), { once: true });
        })]);
        if (!active.has(controller)) return;
        if (controller.signal.aborted) return publish(false, "The side question timed out.");
        if (result.stopReason === "error" || result.stopReason === "aborted") return publish(false, "The model could not answer the side question.");
        const text = result.content.filter(part => part.type === "text").map(part => part.text).join("\n");
        if (!text.trim()) return publish(false, "The model returned no answer text.");
        if (Buffer.byteLength(text) > 256 * 1024) return publish(false, "The side answer exceeded the display limit (256 KiB).");
        publish(true, text);
      } catch {
        if (active.has(controller)) publish(false, controller.signal.aborted ? "The side question timed out." : "Could not submit the side question to the selected model.");
      } finally {
        clearTimeout(timer);
        active.delete(controller);
      }
    },
  });
}
