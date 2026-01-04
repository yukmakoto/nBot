use crate::models::SharedState;

use super::BotModule;

fn merge_json_value(base: &mut serde_json::Value, overlay: &serde_json::Value) {
    match (base, overlay) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(overlay_map)) => {
            for (k, v) in overlay_map {
                match base_map.get_mut(k) {
                    Some(existing) => merge_json_value(existing, v),
                    None => {
                        base_map.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (base_slot, overlay_value) => {
            *base_slot = overlay_value.clone();
        }
    }
}

pub fn get_effective_module(
    state: &SharedState,
    bot_id: &str,
    module_id: &str,
) -> Option<BotModule> {
    let mut module = state.modules.get(module_id)?;

    if let Some(bot) = state.bots.get(bot_id) {
        if let Some(bot_cfg) = bot.modules_config.get(module_id) {
            if let Some(enabled) = bot_cfg.enabled {
                module.enabled = enabled;
            }
            if !bot_cfg.config.is_null() {
                merge_json_value(&mut module.config, &bot_cfg.config);
            }
        }
    }

    Some(module)
}

pub fn is_module_enabled(state: &SharedState, bot_id: &str, module_id: &str) -> bool {
    get_effective_module(state, bot_id, module_id)
        .map(|m| m.enabled)
        .unwrap_or(false)
}
