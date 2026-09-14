import {
  getAgentDir,
  getPackageDir,
  VERSION,
} from "@earendil-works/pi-coding-agent";

const STATUS_KEY = "pie-native-auth-v1";
const CONTEXT_COMMAND = "__pie_native_auth_context_v1";
const REFRESH_COMMAND = "__pie_native_auth_refresh_v1";

function publish(ctx, value) {
  ctx.ui.setStatus(STATUS_KEY, JSON.stringify({ protocol: 1, ...value }));
}

function refreshFailed(result) {
  return result.aborted || result.errors.size > 0;
}

export default function registerPieNativeAuth(pi) {
  pi.registerCommand(CONTEXT_COMMAND, {
    description: "Internal pie authentication context",
    handler: async (_args, ctx) => {
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
