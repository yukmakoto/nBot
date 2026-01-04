/**
 * nBot Cooldown Plugin
 * Prevents spam by setting command execution cooldown.
 *
 * Notes:
 * - This runs in `preCommand`; returning false blocks the command execution.
 * - `nbot.callApi` is async output; use `nbot.sendReply` for user feedback.
 *
 * Config:
 * - default_seconds: default cooldown time (seconds)
 * - per_command: per-command cooldown config, e.g. { "help": 5, "like": 2 }
 * - notify: whether to send a cooldown message when blocked
 * - notify_interval_seconds: throttle for cooldown notifications per key
 * - message_template: supports {command} {remaining} {cooldown}
 * - bypass_super_admin: allow super admins to bypass cooldown
 */

// Cooldown records: Map<"userId:commandName", lastExecuteTime>
const cooldowns = new Map();
const lastNotified = new Map();

function clampNumber(value, fallback, min, max) {
  const n = Number(value);
  if (!Number.isFinite(n)) return fallback;
  return Math.max(min, Math.min(max, n));
}

function getConfig() {
  const cfg = nbot.getConfig() || {};
  return {
    default_seconds: clampNumber(cfg.default_seconds, 3, 0, 3600),
    per_command: cfg.per_command && typeof cfg.per_command === "object" ? cfg.per_command : {},
    notify: cfg.notify !== false,
    notify_interval_seconds: clampNumber(cfg.notify_interval_seconds, 2, 0, 60),
    message_template:
      typeof cfg.message_template === "string" && cfg.message_template.trim()
        ? cfg.message_template
        : "指令冷却中：{command} 还需等待 {remaining} 秒（冷却 {cooldown} 秒）",
    bypass_super_admin: cfg.bypass_super_admin !== false,
  };
}

function formatTemplate(template, replacements) {
  let out = template;
  for (const [key, value] of Object.entries(replacements)) {
    out = out.replaceAll(`{${key}}`, String(value));
  }
  return out;
}

// Return plugin object
return {
  onEnable() {
    const config = getConfig();
    nbot.log.info(
      "Cooldown plugin enabled, default_seconds=" +
        config.default_seconds +
        ", notify=" +
        config.notify
    );
  },

  onDisable() {
    cooldowns.clear();
    lastNotified.clear();
    nbot.log.info("Cooldown plugin disabled");
  },

  preCommand(ctx) {
    const { user_id, group_id, command, is_super_admin } = ctx;
    const config = getConfig();

    if (config.bypass_super_admin && is_super_admin) {
      return true;
    }

    const key = user_id + ":" + command;
    const now = nbot.now();

    const cooldownSecs = clampNumber(
      config.per_command[command],
      config.default_seconds,
      0,
      3600
    );
    const cooldownMs = cooldownSecs * 1000;

    const lastTime = cooldowns.get(key);
    if (lastTime !== undefined) {
      const elapsed = now - lastTime;
      if (elapsed < cooldownMs) {
        const remaining = Math.ceil((cooldownMs - elapsed) / 1000);

        if (config.notify) {
          const notifyIntervalMs = config.notify_interval_seconds * 1000;
          const last = lastNotified.get(key) || 0;
          if (!notifyIntervalMs || now - last >= notifyIntervalMs) {
            lastNotified.set(key, now);
            const msg = formatTemplate(config.message_template, {
              command,
              remaining,
              cooldown: cooldownSecs,
            });
            nbot.sendReply(user_id, group_id || 0, msg);
          }
        }

        return false;
      }
    }

    cooldowns.set(key, now);
    return true;
  },

  updateConfig(newConfig) {
    // Backward-compat: config is always read from nbot.getConfig(); no local cache needed.
    nbot.log.info("Cooldown config updated");
  }
};
