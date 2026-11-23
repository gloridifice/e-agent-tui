import {
  getAgentDir,
  getPackageDir,
  VERSION,
} from "@earendil-works/pi-coding-agent";

import registerBtw from "./pie-btw-companion.mjs";
import registerMcpControls from "./pie-mcp-companion.mjs";
import registerSideControls from "./pie-side-companion.mjs";

const STATUS_KEY = "pie-native-auth-v1";
const CONTEXT_COMMAND = "__pie_native_auth_context_v1";
const REFRESH_COMMAND = "__pie_native_auth_refresh_v1";

function publish(ctx, value) {
  ctx.ui.setStatus(STATUS_KEY, JSON.stringify({ protocol: 1, ...value }));
}

// Only the switch is installed at startup. Do not register the tool or add
// model-facing instructions until the user explicitly enables delegation.
function registerDelegationSwitch(pi) {
  let runtime;
  let retired = false;
  let serial = Promise.resolve();
  const publish = (ctx) => ctx.ui.setStatus("pie-subagents-v1", JSON.stringify({
    sessionId: ctx.sessionManager.getSessionId(), enabled: !!runtime?.enabled,
    entries: runtime?.entries() ?? [],
  }));
  pi.on("session_start", (_event, ctx) => publish(ctx));
  pi.registerCommand("subagent", {
    description: "Toggle background agents, or set /subagent on|off",
    handler: (args, ctx) => {
      const change = async () => {
        if (retired) return;
        const arg = args.trim();
        if (!["", "on", "off"].includes(arg)) {
          ctx.ui.notify("Usage: /subagent [on|off]", "error");
          return;
        }
        const enable = arg === "on" || (!arg && !runtime?.enabled);
        if (enable && !runtime) {
          const { createDelegation } = await import("./pie-subagents.mjs");
          if (retired) return;
          runtime = createDelegation(pi, publish, getPackageDir());
        }
        if (enable) runtime.enable();
        else if (runtime?.enabled) await runtime.disable(ctx);
        if (!retired) publish(ctx);
      };
      serial = serial.then(change).catch((e) => ctx.ui.notify(`Subagent switch failed: ${e.message}`, "error"));
      return serial;
    },
  });
  pi.on("session_shutdown", async () => {
    retired = true;
    await runtime?.shutdown();
  });
  return { publish };
}

function refreshFailed(result) {
  return result.aborted || result.errors.size > 0;
}

export default function registerPieNativeAuth(pi) {
  registerBtw(pi);
  registerMcpControls(pi);
  registerSideControls(pi);
  const delegation = registerDelegationSwitch(pi);
  pi.registerCommand("__pie_reload_v1", {
    description: "Internal pie resource reload",
    handler: async (_args, ctx) => {
      if (!ctx.isIdle()) throw new Error("Wait for Pi to become idle before /reload");
      await ctx.reload();
    },
  });

  pi.registerCommand(CONTEXT_COMMAND, {
    description: "Internal pie authentication context",
    handler: async (_args, ctx) => {
      delegation.publish(ctx);
      publish(ctx, {
        kind: "context",
        packageRoot: getPackageDir(),
        agentDir: getAgentDir(),
        nodePath: process.execPath,
        cwd: ctx.cwd,
        sessionId: ctx.sessionManager.getSessionId(),
        projectTrusted: ctx.isProjectTrusted(),
        piVersion: VERSION,
      });
    },
  });

  pi.registerCommand(REFRESH_COMMAND, {
    description: "Internal pie authentication refresh",
    handler: async (args, ctx) => {
      let flowId = "";
      try {
        const request = JSON.parse(args);
        if (typeof request.flowId !== "string" || typeof request.provider !== "string") {
          throw new Error("invalid refresh request");
        }
        flowId = request.flowId;
        const local = await ctx.modelRegistry.refresh({
          allowNetwork: false,
          providers: [request.provider],
        });
        if (refreshFailed(local)) {
          publish(ctx, {
            kind: "refresh",
            flowId,
            success: false,
            error: "Running Pi provider refresh failed",
            remoteRefreshed: false,
          });
          return;
        }

        let remoteRefreshed = false;
        let remoteWarning;
        if (process.env.PI_OFFLINE !== undefined) {
          remoteWarning = "remote model catalog refresh skipped in offline mode";
        } else {
          try {
            const remote = await ctx.modelRegistry.refresh({
              allowNetwork: true,
              providers: [request.provider],
              signal: AbortSignal.timeout(10_000),
            });
            if (refreshFailed(remote)) {
              remoteWarning = "remote model catalog refresh failed";
            } else {
              remoteRefreshed = true;
            }
          } catch {
            remoteWarning = "remote model catalog refresh failed";
          }
        }
        publish(ctx, {
          kind: "refresh",
          flowId,
          success: true,
          remoteRefreshed,
          remoteWarning,
        });
      } catch {
        publish(ctx, {
          kind: "refresh",
          flowId,
          success: false,
          error: "Running Pi provider refresh failed",
          remoteRefreshed: false,
        });
      }
    },
  });
}
