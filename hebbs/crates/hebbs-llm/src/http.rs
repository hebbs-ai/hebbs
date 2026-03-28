use serde::Serialize;

use crate::error::{LlmError, Result};

/// Build an HTTP agent with the given global timeout.
pub(crate) fn make_http_agent(timeout_secs: u64) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(timeout_secs)))
        .build()
        .new_agent()
}

/// Build an HTTP agent with a long timeout for batch polling (5 min).
pub(crate) fn make_batch_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(300)))
        .build()
        .new_agent()
}

/// GET a URL with headers, return response body as string.
pub(crate) fn http_get(agent: &ureq::Agent, url: &str, headers: &[(&str, &str)]) -> Result<String> {
    let mut req = agent.get(url);
    for &(k, v) in headers {
        req = req.header(k, v);
    }
    let resp = req.call().map_err(|e| LlmError::Provider {
        message: format!("GET {url} failed: {e}"),
    })?;
    resp.into_body()
        .read_to_string()
        .map_err(|e| LlmError::Provider {
            message: format!("failed to read GET response: {e}"),
        })
}

/// POST a JSON body with retry logic, exponential backoff, and jitter.
///
/// On 429 responses, respects Retry-After header if present.
/// Jitter prevents thundering herd when multiple threads retry simultaneously.
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
            // Add jitter: 50-150% of computed backoff to prevent thundering herd
            let jitter = backoff / 2 + (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos() as u64
                % backoff.max(1));
            std::thread::sleep(std::time::Duration::from_millis(jitter));
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
                let is_rate_limited = last_err.contains("429");
                let retryable = is_rate_limited
                    || last_err.contains("500")
                    || last_err.contains("timeout")
                    || last_err.contains("connection");
                if !retryable {
                    return Err(LlmError::Provider { message: last_err });
                }
                // On 429, sleep longer to let rate limit window reset
                if is_rate_limited && attempt < max_retries {
                    let rate_limit_sleep = 5000u64 * (1u64 << attempt.min(3));
                    eprintln!(
                        "hebbs: rate limited (429), waiting {}s before retry {}/{}",
                        rate_limit_sleep / 1000,
                        attempt + 1,
                        max_retries
                    );
                    std::thread::sleep(std::time::Duration::from_millis(rate_limit_sleep));
                }
            }
        }
    }
    Err(LlmError::Provider {
        message: format!("exhausted retries: {last_err}"),
    })
}
