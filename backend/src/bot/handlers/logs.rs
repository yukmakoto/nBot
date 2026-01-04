use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;

use crate::models::SharedState;

#[derive(Deserialize)]
pub struct LogsQuery {
    #[serde(default)]
    cursor: Option<u64>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(serde::Serialize)]
pub struct LogsResponse {
    pub cursor: u64,
    pub next_cursor: u64,
    pub truncated: bool,
    pub lines: Vec<crate::logs::LogLine>,
}

pub async fn get_system_logs_handler(
    State(state): State<SharedState>,
    Query(query): Query<LogsQuery>,
) -> Json<LogsResponse> {
    let limit = query.limit.unwrap_or(400).clamp(1, 2000);
    let (cursor, truncated, lines) = state.logs.snapshot(query.cursor, limit);
    let next_cursor = lines
        .last()
        .map(|l| l.id)
        .unwrap_or_else(|| query.cursor.unwrap_or(cursor));
    Json(LogsResponse {
        cursor,
        next_cursor,
        truncated,
        lines,
    })
}
