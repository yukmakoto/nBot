use deno_core::op2;

// Op: HTTP fetch (async)
#[op2(async)]
#[string]
pub(in super::super) async fn op_http_fetch(
    #[string] url: String,
    #[bigint] timeout_ms: i64,
) -> Result<String, deno_core::error::AnyError> {
    let client = reqwest::Client::new();
    let timeout = std::time::Duration::from_millis(timeout_ms.clamp(1000, 60000) as u64);

    let resp = client
        .get(&url)
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| deno_core::error::generic_error(format!("HTTP request failed: {}", e)))?;

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| deno_core::error::generic_error(format!("Failed to read response: {}", e)))?;

    Ok(String::from_utf8_lossy(&bytes).to_string())
}
