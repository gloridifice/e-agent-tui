import { createProvider, openAICompletionsApi } from "@earendil-works/pi-ai";

function provider(id, includeModel = true) {
  return createProvider({
    id,
    name: id === "probe-factory"
      ? "Probe Factory"
      : id === "probe-model-less"
        ? "Probe Model-less"
        : "Probe Late",
    baseUrl: "https://example.invalid/v1",
    auth: {
      apiKey: {
        name: "Probe API key",
        async login(interaction) {
          const method = await interaction.prompt({
            type: "select",
            message: "Select probe authentication method",
            options: [
              { id: "plain", label: "Plain key" },
              { id: "multi", label: "Multi-field key" },
              { id: "fail", label: "Fail safely" },
              { id: "sync_fail", label: "Commit then fail synchronization" },
              { id: "manual", label: "Manual callback" },
            ],
          });
          if (method === "manual") {
            const code = await interaction.prompt({
              type: "manual_code",
              message: "Paste callback URL or code",
            });
            return { type: "api_key", key: code };
          }
          const key = await interaction.prompt({
            type: "secret",
            message: "Enter probe API key",
          });
          if (method === "fail") {
            throw new Error(`fixture rejected ${key}`);
          }
          if (method === "sync_fail") {
            return { type: "api_key", key, env: { PROBE_SYNC_FAIL: "1" } };
          }
          if (method === "multi") {
            const account = await interaction.prompt({
              type: "text",
              message: "Enter probe account",
            });
            return { type: "api_key", key, env: { PROBE_ACCOUNT: account } };
          }
          return { type: "api_key", key };
        },
        async resolve({ credential, ctx }) {
          if (credential?.env?.PROBE_SYNC_FAIL === "1") {
            throw new Error("fixture synchronization failed");
          }
          const key = credential?.key ?? await ctx.env("PROBE_API_KEY");
          if (!key) return undefined;
          return {
            auth: { apiKey: key },
            env: credential?.env,
            source: credential?.key ? "stored credential" : "PROBE_API_KEY",
          };
        },
      },
    },
    models: includeModel
      ? [{
          id: `${id}-model`,
          name: `${id} model`,
          api: "openai-completions",
          provider: id,
          baseUrl: "https://example.invalid/v1",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 4096,
          maxTokens: 1024,
        }]
      : [],
    api: openAICompletionsApi(),
  });
}

export default function (pi) {
  pi.registerProvider(provider("probe-factory"));
  pi.registerProvider(provider("probe-model-less", false));
  pi.registerCommand("probe-register-late", {
    description: "Register a provider after session startup",
    handler: async (_args, ctx) => {
      pi.registerProvider(provider("probe-late"));
      ctx.ui.notify("probe-late registered", "info");
    },
  });
  pi.registerCommand("probe-refresh", {
    description: "Refresh the active RPC model registry",
    handler: async (_args, ctx) => {
      await ctx.modelRegistry.refresh({ allowNetwork: false });
      ctx.ui.notify("probe model registry refreshed", "info");
    },
  });
}
