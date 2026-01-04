use crate::command::CommandRegistry;
use crate::plugin::InstalledPlugin;

pub fn register_plugin_commands(commands: &CommandRegistry, plugin: &InstalledPlugin) {
    // Always clear previous registrations to avoid stale/duplicated commands across reloads.
    commands.unregister_plugin_commands(&plugin.manifest.id);

    // Preferred source: manifest.commands (persisted to manifest.json and parsed into PluginManifest).
    // Backward-compat: allow config.commands if manifest.commands is empty.
    let mut declared: Vec<String> = plugin
        .manifest
        .commands
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if declared.is_empty() {
        if let Some(cmds) = plugin
            .manifest
            .config
            .get("commands")
            .and_then(|v| v.as_array())
        {
            declared = cmds
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }

    if declared.is_empty() {
        return;
    }

    // One plugin => one command, with aliases.
    // `commands` is interpreted as: [primary, ...aliases]
    let primary = declared[0].clone();

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut aliases: Vec<String> = Vec::new();
    for name in declared.into_iter().skip(1) {
        if seen.insert(name.clone()) {
            aliases.push(name);
        }
    }

    commands.register_plugin_command(
        &plugin.manifest.id,
        &primary,
        aliases,
        &plugin.manifest.description,
    );
}
