use serde::Serialize;

use crate::error::{LlmError, Result};

/// Build an HTTP agent with the given global timeout.
pub(crate) fn make_http_agent(timeout_secs: u64) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(timeout_secs)))
        .build()
        .new_agent()
}

/// POST a JSON body with retry logic and exponential backoff.
///
/// Complexity: O(max_retries) network calls in the worst case.
pub(crate) fn http_post_json(
    agent: &ureq::Agent,
    url: &str,
    headers: &[(&str, &str)],
    body: &impl Serialize,
    max_retries: usize,
    retry_backoff_ms: u64,
) -> Result<String> {
    let mut last_err = String::new();
    for attempt in 0..=max_retries {
        if attempt > 0 {
            let backoff = retry_backoff_ms * (1u64 << (attempt - 1).min(6));
            std::thread::sleep(std::time::Duration::from_millis(backoff));
        }
        let mut req = agent.post(url);
        for &(k, v) in headers {
            req = req.header(k, v);
        }
        req = req.header("content-type", "application/json");

        match req.send_json(body) {
            Ok(resp) => {
                let text = resp
                    .into_body()
                    .read_to_string()
                    .map_err(|e| LlmError::Provider {
                        message: format!("failed to read response body: {e}"),
                    })?;
                return Ok(text);
            }
            Err(e) => {
                last_err = format!("{e}");
                let retryable = last_err.contains("429")
                    || last_err.contains("500")
                    || last_err.contains("timeout")
                    || last_err.contains("connection");
                if !retryable {
                    return Err(LlmError::Provider { message: last_err });
                }
            }
        }
    }
    Err(LlmError::Provider {
        message: format!("exhausted retries: {last_err}"),
    })
}
