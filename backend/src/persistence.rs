use crate::models::{BotInstance, DatabaseInstance};
use dashmap::DashMap;
use std::fs;
use std::path::Path;
use tracing::{info, warn};

const STATE_DIR: &str = "data/state";
const BOTS_FILE: &str = "data/state/bots.json";
const DATABASES_FILE: &str = "data/state/databases.json";

fn ensure_dir() {
    let _ = fs::create_dir_all(STATE_DIR);
}

pub fn save_bots(bots: &DashMap<String, BotInstance>) {
    ensure_dir();
    let list: Vec<BotInstance> = bots.iter().map(|r| r.value().clone()).collect();
    match serde_json::to_string_pretty(&list) {
        Ok(json) => {
            if let Err(e) = fs::write(BOTS_FILE, json) {
                warn!("Failed to save bots: {:?}", e);
            } else {
                info!("Saved {} bots to {}", list.len(), BOTS_FILE);
            }
        }
        Err(e) => warn!("Failed to serialize bots: {:?}", e),
    }
}

pub fn load_bots() -> DashMap<String, BotInstance> {
    let map = DashMap::new();

    if Path::new(BOTS_FILE).exists() {
        match fs::read_to_string(BOTS_FILE) {
            Ok(json) => match serde_json::from_str::<Vec<BotInstance>>(&json) {
                Ok(list) => {
                    info!("Loaded {} bots from {}", list.len(), BOTS_FILE);
                    for bot in list {
                        map.insert(bot.id.clone(), bot);
                    }
                }
                Err(e) => warn!("Failed to parse bots.json: {:?}", e),
            },
            Err(e) => warn!("Failed to read bots.json: {:?}", e),
        }
    }

    map
}

pub fn save_databases(dbs: &DashMap<String, DatabaseInstance>) {
    ensure_dir();
    let list: Vec<DatabaseInstance> = dbs.iter().map(|r| r.value().clone()).collect();
    match serde_json::to_string_pretty(&list) {
        Ok(json) => {
            if let Err(e) = fs::write(DATABASES_FILE, json) {
                warn!("Failed to save databases: {:?}", e);
            } else {
                info!("Saved {} databases to {}", list.len(), DATABASES_FILE);
            }
        }
        Err(e) => warn!("Failed to serialize databases: {:?}", e),
    }
}

pub fn load_databases() -> DashMap<String, DatabaseInstance> {
    let map = DashMap::new();

    if Path::new(DATABASES_FILE).exists() {
        match fs::read_to_string(DATABASES_FILE) {
            Ok(json) => match serde_json::from_str::<Vec<DatabaseInstance>>(&json) {
                Ok(list) => {
                    info!("Loaded {} databases from {}", list.len(), DATABASES_FILE);
                    for db in list {
                        map.insert(db.id.clone(), db);
                    }
                }
                Err(e) => warn!("Failed to parse databases.json: {:?}", e),
            },
            Err(e) => warn!("Failed to read databases.json: {:?}", e),
        }
    }

    map
}
