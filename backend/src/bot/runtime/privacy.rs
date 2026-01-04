use std::collections::HashSet;
use tokio::task_local;

task_local! {
    static SENSITIVE_IDS: HashSet<String>;
}

pub(super) async fn with_sensitive_ids<T>(
    ids: HashSet<String>,
    fut: impl std::future::Future<Output = T>,
) -> T {
    SENSITIVE_IDS.scope(ids, fut).await
}

pub(super) fn get_sensitive_ids() -> Vec<String> {
    SENSITIVE_IDS
        .try_with(|s| s.iter().cloned().collect::<Vec<_>>())
        .unwrap_or_default()
}

