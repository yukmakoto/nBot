use crate::models::SharedState;
use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct LlmAbuseConfig {
    pub enabled: bool,
    pub max_concurrent_global: usize,
    pub max_concurrent_per_user: usize,
    pub max_concurrent_per_group: usize,
    pub min_interval_per_user: Duration,
}

impl LlmAbuseConfig {
    pub fn from_state(state: &SharedState, bot_id: &str) -> Self {
        let module = crate::module::get_effective_module(state, bot_id, "llm");
        let limits = module
            .as_ref()
            .and_then(|m| m.config.get("limits"))
            .unwrap_or(&serde_json::Value::Null);

        let enabled = limits
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let max_concurrent_global = limits
            .get("max_concurrent_global")
            .and_then(|v| v.as_u64())
            .unwrap_or(2)
            .clamp(1, 64) as usize;

        let max_concurrent_per_user = limits
            .get("max_concurrent_per_user")
            .and_then(|v| v.as_u64())
            .unwrap_or(1)
            .clamp(1, 16) as usize;

        let max_concurrent_per_group = limits
            .get("max_concurrent_per_group")
            .and_then(|v| v.as_u64())
            .unwrap_or(1)
            .clamp(1, 16) as usize;

        let min_interval_secs = limits
            .get("min_interval_seconds_per_user")
            .and_then(|v| v.as_u64())
            .unwrap_or(10)
            .clamp(0, 3600);

        Self {
            enabled,
            max_concurrent_global,
            max_concurrent_per_user,
            max_concurrent_per_group,
            min_interval_per_user: Duration::from_secs(min_interval_secs),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LlmAbuseBlock {
    pub message: String,
}

pub struct LlmTaskGuard {
    active: bool,
    user_id: u64,
    group_id: u64,
}

impl Drop for LlmTaskGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        GLOBAL_INFLIGHT.fetch_sub(1, Ordering::Relaxed);
        dec_inflight(&USER_INFLIGHT, self.user_id);
        if self.group_id != 0 {
            dec_inflight(&GROUP_INFLIGHT, self.group_id);
        }
    }
}

static GLOBAL_INFLIGHT: AtomicUsize = AtomicUsize::new(0);
static USER_INFLIGHT: Lazy<DashMap<u64, AtomicUsize>> = Lazy::new(DashMap::new);
static GROUP_INFLIGHT: Lazy<DashMap<u64, AtomicUsize>> = Lazy::new(DashMap::new);
static USER_LAST_START: Lazy<DashMap<u64, Instant>> = Lazy::new(DashMap::new);

fn try_inc_global(max: usize) -> bool {
    let mut cur = GLOBAL_INFLIGHT.load(Ordering::Relaxed);
    loop {
        if cur >= max {
            return false;
        }
        match GLOBAL_INFLIGHT.compare_exchange_weak(
            cur,
            cur + 1,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return true,
            Err(v) => cur = v,
        }
    }
}

fn try_inc_inflight(map: &DashMap<u64, AtomicUsize>, key: u64, max: usize) -> bool {
    let entry = map.entry(key).or_insert_with(|| AtomicUsize::new(0));
    let counter = entry.value();

    let mut cur = counter.load(Ordering::Relaxed);
    loop {
        if cur >= max {
            return false;
        }
        match counter.compare_exchange_weak(cur, cur + 1, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return true,
            Err(v) => cur = v,
        }
    }
}

fn dec_inflight(map: &DashMap<u64, AtomicUsize>, key: u64) {
    if let Some(entry) = map.get(&key) {
        entry.value().fetch_sub(1, Ordering::Relaxed);
    }
}

pub fn try_begin_llm_task(
    cfg: LlmAbuseConfig,
    user_id: u64,
    group_id: u64,
) -> Result<LlmTaskGuard, LlmAbuseBlock> {
    if !cfg.enabled {
        return Ok(LlmTaskGuard {
            active: false,
            user_id,
            group_id,
        });
    }

    if !try_inc_global(cfg.max_concurrent_global) {
        return Err(LlmAbuseBlock {
            message: "当前分析请求过多，请稍后再试".to_string(),
        });
    }

    if !try_inc_inflight(&USER_INFLIGHT, user_id, cfg.max_concurrent_per_user) {
        GLOBAL_INFLIGHT.fetch_sub(1, Ordering::Relaxed);
        return Err(LlmAbuseBlock {
            message: "你有一个正在进行的分析任务，请等待完成后再试".to_string(),
        });
    }

    if group_id != 0 && !try_inc_inflight(&GROUP_INFLIGHT, group_id, cfg.max_concurrent_per_group) {
        dec_inflight(&USER_INFLIGHT, user_id);
        GLOBAL_INFLIGHT.fetch_sub(1, Ordering::Relaxed);
        return Err(LlmAbuseBlock {
            message: "当前群内已有分析任务在进行，请稍后再试".to_string(),
        });
    }

    if cfg.min_interval_per_user != Duration::ZERO {
        let now = Instant::now();
        let mut block = None;
        match USER_LAST_START.entry(user_id) {
            dashmap::mapref::entry::Entry::Occupied(mut e) => {
                let last = *e.get();
                let elapsed = now.saturating_duration_since(last);
                if elapsed < cfg.min_interval_per_user {
                    let remain = cfg.min_interval_per_user - elapsed;
                    let remain_secs = (remain.as_secs_f64().ceil() as u64).max(1);
                    block = Some(format!("请求过于频繁，请 {} 秒后再试", remain_secs));
                } else {
                    e.insert(now);
                }
            }
            dashmap::mapref::entry::Entry::Vacant(v) => {
                v.insert(now);
            }
        }

        if let Some(message) = block {
            if group_id != 0 {
                dec_inflight(&GROUP_INFLIGHT, group_id);
            }
            dec_inflight(&USER_INFLIGHT, user_id);
            GLOBAL_INFLIGHT.fetch_sub(1, Ordering::Relaxed);
            return Err(LlmAbuseBlock { message });
        }
    }

    Ok(LlmTaskGuard {
        active: true,
        user_id,
        group_id,
    })
}
