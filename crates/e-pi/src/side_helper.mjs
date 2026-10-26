import { closeSync, existsSync, openSync, readSync, statSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { parentPort, workerData } from "node:worker_threads";

const MAX_SOURCE_BYTES = 32 * 1024 * 1024;
try {
  const { request, packageRoot } = workerData;
  const { SessionManager, CURRENT_SESSION_VERSION } = await import(pathToFileURL(join(packageRoot, "dist", "index.js")).href);
  let session;
  if (request.leaf) {
    if (statSync(request.source).size > MAX_SOURCE_BYTES) throw new Error("Session is too large for side snapshot preparation (32 MiB maximum)");
    const fd = openSync(request.source, "r");
    const headerBuffer = Buffer.alloc(65536);
    let bytes;
    try { bytes = readSync(fd, headerBuffer, 0, headerBuffer.length, 0); }
    finally { closeSync(fd); }
    const newline = headerBuffer.subarray(0, bytes).indexOf(10);
    if (newline < 0) throw new Error("Session header exceeds the side snapshot limit");
    const header = JSON.parse(headerBuffer.subarray(0, newline).toString("utf8"));
    // Native open may migrate legacy files. Never let a detached reader migrate the live source.
    if (header.type !== "session" || header.version !== CURRENT_SESSION_VERSION || header.id !== request.sourceId) {
      throw new Error("Side snapshots require the current native session format and identity");
    }
    const manager = SessionManager.open(request.source, request.directory, request.cwd);
    if (statSync(request.source).size > MAX_SOURCE_BYTES) throw new Error("Session grew beyond the side snapshot limit");
    if (manager.getSessionId() !== request.sourceId || !manager.getEntry(request.leaf)) {
      throw new Error("The source checkpoint is no longer available");
    }
    const branch = manager.getBranch(request.leaf);
    const pending = new Set();
    for (const entry of branch) {
      if (entry.type !== "message") continue;
      if (entry.message.role === "assistant") {
        for (const part of entry.message.content ?? []) if (part.type === "toolCall") pending.add(part.id);
      } else if (entry.message.role === "toolResult") pending.delete(entry.message.toolCallId);
    }
    if (pending.size) throw new Error("The selected checkpoint contains unfinished tool calls");
    session = manager.createBranchedSession(request.leaf);
  } else {
    session = SessionManager.create(request.cwd, request.directory, {
      parentSession: request.parent,
    }).getSessionFile();
  }
  if (!session || session === request.source) throw new Error("Pi did not create an independent side session");
  parentPort.postMessage({ session, persisted: existsSync(session) });
} catch (error) {
  parentPort.postMessage({ error: (error instanceof Error ? error.message : "Side session preparation failed").slice(0, 4096) });
} finally {
  parentPort.close();
}
