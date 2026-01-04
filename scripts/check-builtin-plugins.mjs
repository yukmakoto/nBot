import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { isDeepStrictEqual } from "node:util";
import vm from "node:vm";

function createNbotStub(pluginId, config = {}) {
  const storage = new Map();
  const outputs = [];

  const push = (type, args) => outputs.push({ type, args });

  const nbot = {
    sendMessage: (...args) => push("sendMessage", args),
    sendReply: (...args) => push("sendReply", args),
    callApi: (...args) => push("callApi", args),

    callLlmForward: (...args) => push("callLlmForward", args),
    callLlmForwardFromUrl: (...args) => push("callLlmForwardFromUrl", args),
    callLlmForwardImageFromUrl: (...args) => push("callLlmForwardImageFromUrl", args),
    callLlmForwardVideoFromUrl: (...args) => push("callLlmForwardVideoFromUrl", args),
    callLlmForwardAudioFromUrl: (...args) => push("callLlmForwardAudioFromUrl", args),
    callLlmForwardMediaBundle: (...args) => push("callLlmForwardMediaBundle", args),
    callLlmChat: (...args) => push("callLlmChat", args),
    callLlmChatWithSearch: (...args) => push("callLlmChatWithSearch", args),
    sendForwardMessage: (...args) => push("sendForwardMessage", args),

    httpFetch: async (...args) => {
      push("httpFetch", args);
      return { ok: false, status: 0, body: "" };
    },
    renderMarkdownImage: (..._args) => null,

    fetchGroupNotice: (...args) => push("fetchGroupNotice", args),
    fetchGroupMsgHistory: (...args) => push("fetchGroupMsgHistory", args),
    fetchGroupFiles: (...args) => push("fetchGroupFiles", args),
    fetchGroupFileUrl: (...args) => push("fetchGroupFileUrl", args),
    downloadFile: (...args) => push("downloadFile", args),

    log: {
      info: (_msg) => {},
      warn: (_msg) => {},
      error: (_msg) => {},
    },

    now: () => Date.now(),
    getPluginId: () => pluginId,

    getConfig: () => config,
    setConfig: (_cfg) => true,

    storage: {
      get: (key) => (storage.has(key) ? storage.get(key) : null),
      set: (key, value) => {
        storage.set(key, value);
        return true;
      },
      delete: (key) => {
        storage.delete(key);
        return true;
      },
    },
  };

  return { nbot, outputs };
}

async function main() {
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
  const pluginsDir = path.join(root, "data", "plugins", "bot");

  const entries = await fs.readdir(pluginsDir, { withFileTypes: true });
  const pluginDirs = entries.filter((e) => e.isDirectory()).map((e) => e.name).sort();

  let okCount = 0;
  const failures = [];
  const warnings = [];

  for (const dirName of pluginDirs) {
    const pluginRoot = path.join(pluginsDir, dirName);
    const manifestPath = path.join(pluginRoot, "manifest.json");
    const indexPath = path.join(pluginRoot, "index.js");

    let manifest;
    let code;
    try {
      manifest = JSON.parse(await fs.readFile(manifestPath, "utf8"));
      code = await fs.readFile(indexPath, "utf8");
    } catch (e) {
      failures.push({ plugin: dirName, error: `Read failed: ${e?.message || e}` });
      continue;
    }

    const pluginId = String(manifest.id || dirName);
    const pluginWarnings = [];

    const schemaKeys = new Set(
      Array.isArray(manifest.configSchema)
        ? manifest.configSchema
            .map((i) => (i && typeof i === "object" ? String(i.key || "") : ""))
            .filter((k) => k)
        : [],
    );

    if (schemaKeys.has("enabled")) {
      pluginWarnings.push("manifest.configSchema 包含 key=enabled（容易形成“双开关”）");
    }

    if (manifest.config && typeof manifest.config === "object" && !Array.isArray(manifest.config)) {
      if (Object.prototype.hasOwnProperty.call(manifest.config, "enabled")) {
        pluginWarnings.push("manifest.config 包含 enabled（容易形成“双开关”）");
      }

      const configKeys = Object.keys(manifest.config);
      const extra = configKeys.filter((k) => k !== "commands" && !schemaKeys.has(k));
      if (extra.length > 0) {
        pluginWarnings.push(`manifest.config 存在未声明到 configSchema 的字段: ${extra.join(", ")}`);
      }

      if (Array.isArray(manifest.configSchema)) {
        for (const item of manifest.configSchema) {
          const key = item && typeof item === "object" ? String(item.key || "") : "";
          if (!key) continue;
          if (!Object.prototype.hasOwnProperty.call(manifest.config, key)) continue;
          if (!Object.prototype.hasOwnProperty.call(item, "default")) continue;
          if (!isDeepStrictEqual(manifest.config[key], item.default)) {
            pluginWarnings.push(`configSchema.default 与 config 不一致: ${key}`);
          }
        }
      }
    }

    const { nbot } = createNbotStub(pluginId, manifest.config || {});
    const context = vm.createContext({
      console,
      globalThis: null,
      nbot,
      setTimeout,
      clearTimeout,
      setInterval,
      clearInterval,
    });
    context.globalThis = context;

    const wrapped = `
      const plugin = (function() {
${code}
      })();
      globalThis.__plugin = (plugin && (plugin.default || plugin)) || plugin;
    `;

    try {
      const script = new vm.Script(wrapped, { filename: indexPath });
      script.runInContext(context, { timeout: 1000 });

      const plugin = context.__plugin;
      if (!plugin || typeof plugin !== "object") {
        throw new Error("Plugin did not export an object (return {...})");
      }

      const declaredCommands = Array.isArray(manifest.commands)
        ? manifest.commands.map((s) => String(s).trim()).filter(Boolean)
        : [];
      const fallbackCommands =
        !declaredCommands.length && Array.isArray(manifest?.config?.commands)
          ? manifest.config.commands.map((s) => String(s).trim()).filter(Boolean)
          : [];
      const effectiveDeclaredCommands = declaredCommands.length ? declaredCommands : fallbackCommands;
      if (typeof plugin.onCommand === "function" && effectiveDeclaredCommands.length === 0) {
        pluginWarnings.push("插件实现了 onCommand，但 manifest.commands 为空（指令将无法路由到插件）");
      }

      if (typeof plugin.onEnable === "function") {
        await plugin.onEnable();
      }

      okCount += 1;
      process.stdout.write(`OK  ${pluginId}\n`);
      for (const w of pluginWarnings) {
        warnings.push({ plugin: pluginId, warning: w });
        process.stdout.write(`WARN ${pluginId}  ${w}\n`);
      }
    } catch (e) {
      failures.push({ plugin: pluginId, error: e?.stack || String(e) });
      process.stdout.write(`ERR ${pluginId}\n`);
    }
  }

  process.stdout.write(`\nChecked ${pluginDirs.length} plugins: ${okCount} OK, ${failures.length} failed\n`);
  if (warnings.length > 0) {
    process.stdout.write(`Warnings: ${warnings.length}\n`);
  }
  if (failures.length > 0) {
    for (const f of failures) {
      process.stdout.write(`\n[${f.plugin}]\n${f.error}\n`);
    }
    process.exitCode = 1;
  }
}

await main();
