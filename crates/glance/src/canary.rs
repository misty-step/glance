//! Fire-and-forget Canary self-reporter. No creds => silent no-op.
//!
//! `glance` is a short-lived CLI/build tool, not a standing service: there
//! is no background health loop, only a [`check_in`] as early as possible in
//! `main` and a [`report_error`] on the top-level failure path. Sends run on
//! a detached thread so a Canary outage never slows or blocks generation,
//! but the process must call [`flush`] before exit or the proof event may
//! never leave the machine before the thread is torn down.

use std::sync::{Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

const SERVICE: &str = "glance-next"; // overridable via CANARY_SERVICE
const MONITOR: &str = "glance-next"; // must already exist in Canary
const TTL_MS: u64 = 120_000;
const SEND_TIMEOUT: Duration = Duration::from_secs(3);

fn pending() -> &'static Mutex<Vec<JoinHandle<()>>> {
    static PENDING: OnceLock<Mutex<Vec<JoinHandle<()>>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(Vec::new()))
}

fn config() -> Option<(String, String)> {
    let endpoint = std::env::var("CANARY_ENDPOINT").ok()?;
    let key = std::env::var("CANARY_API_KEY")
        .or_else(|_| std::env::var("CANARY_INGEST_KEY"))
        .ok()?;
    (!endpoint.trim().is_empty() && !key.trim().is_empty())
        .then(|| (endpoint.trim_end_matches('/').to_owned(), key))
}

fn service() -> String {
    std::env::var("CANARY_SERVICE")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| SERVICE.to_owned())
}

/// Report a handled or unhandled error. Safe to call anywhere; a no-op
/// without `CANARY_ENDPOINT`/`CANARY_API_KEY`.
pub fn report_error(error_class: &str, message: &str) {
    let Some((endpoint, key)) = config() else {
        return;
    };
    let environment =
        std::env::var("CANARY_ENVIRONMENT").unwrap_or_else(|_| "production".to_owned());
    let body = serde_json::json!({
        "service": service(),
        "error_class": error_class,
        "message": message.chars().take(4096).collect::<String>(),
        "severity": "error",
        "environment": environment,
    });
    spawn_send(endpoint, key, "/api/v1/errors", body);
}

/// One check-in per CLI invocation. Call as early as possible in `main`.
pub fn check_in() {
    let Some((endpoint, key)) = config() else {
        return;
    };
    let body = serde_json::json!({
        "monitor": MONITOR,
        "status": "alive",
        "summary": concat!(env!("CARGO_PKG_NAME"), " run"),
        "ttl_ms": TTL_MS,
    });
    spawn_send(endpoint, key, "/api/v1/check-ins", body);
}

/// Wait for any in-flight Canary sends to finish. `glance` is a short-lived
/// process: without this, the detached send thread from [`check_in`] or
/// [`report_error`] can be killed mid-flight by process exit before the
/// event reaches the network. Bounded by [`SEND_TIMEOUT`] per attempt (two
/// attempts max), so this never blocks indefinitely.
pub fn flush() {
    let handles = match pending().lock() {
        Ok(mut guard) => std::mem::take(&mut *guard),
        Err(_) => return,
    };
    for handle in handles {
        let _ = handle.join();
    }
}

fn spawn_send(endpoint: String, key: String, path: &'static str, body: serde_json::Value) {
    let handle = std::thread::Builder::new()
        .name("canary-report".into())
        .spawn(move || {
            let url = format!("{endpoint}{path}");
            let auth = format!("Bearer {key}");
            for _ in 0..2 {
                // one retry, then give up silently
                let sent = ureq::post(&url)
                    .timeout(SEND_TIMEOUT)
                    .set("Authorization", &auth)
                    .set("Content-Type", "application/json")
                    .send_json(&body)
                    .is_ok();
                if sent {
                    break;
                }
            }
        });
    let Ok(handle) = handle else {
        return;
    };
    if let Ok(mut guard) = pending().lock() {
        guard.push(handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc;
    use std::time::Instant;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn set_test_env(endpoint: &str) {
        // SAFETY: every test that mutates process env serializes on
        // `env_lock` for its whole env-touching window, so no other thread
        // observes a torn read/write of these vars.
        unsafe {
            std::env::set_var("CANARY_ENDPOINT", endpoint);
            std::env::set_var("CANARY_API_KEY", "test-key");
        }
    }

    fn clear_test_env() {
        // SAFETY: see `set_test_env`.
        unsafe {
            std::env::remove_var("CANARY_ENDPOINT");
            std::env::remove_var("CANARY_API_KEY");
        }
    }

    /// Read a full HTTP/1.1 request off `stream`: headers, then exactly
    /// `Content-Length` body bytes. A single `read()` call is not enough —
    /// TCP may deliver the request across more than one segment, especially
    /// under load — so this loops until the declared body length is in
    /// hand (bounded by the caller's read timeout).
    fn read_full_request(stream: &mut TcpStream) -> String {
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            let read = match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => read,
            };
            buffer.extend_from_slice(&chunk[..read]);

            let Some(header_end) = buffer.windows(4).position(|window| window == b"\r\n\r\n")
            else {
                continue;
            };
            let headers = String::from_utf8_lossy(&buffer[..header_end]);
            let content_length: usize = headers
                .lines()
                .find_map(|line| {
                    line.split_once(':').and_then(|(name, value)| {
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse().ok())
                            .flatten()
                    })
                })
                .unwrap_or(0);
            let body_len = buffer.len() - (header_end + 4);
            if body_len >= content_length {
                break;
            }
        }
        String::from_utf8_lossy(&buffer).into_owned()
    }

    /// Spawn a one-shot mock server that captures the first request it
    /// receives, replies 200, and hands the raw request text back over the
    /// returned channel.
    fn one_shot_mock_server() -> (String, JoinHandle<()>, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
        let addr = listener.local_addr().expect("local addr");
        let (tx, rx) = mpsc::channel();

        let server = std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let request = read_full_request(&mut stream);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
            let _ = tx.send(request);
        });

        (format!("http://{addr}"), server, rx)
    }

    #[test]
    fn report_error_sends_expected_request_to_mock_server() {
        let _guard = env_lock().lock().expect("env lock");
        let (endpoint, server, rx) = one_shot_mock_server();

        set_test_env(&endpoint);
        report_error("glance-next.test.mock", "boom");
        flush();
        clear_test_env();

        let request = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("mock server received a request");
        server.join().expect("server thread joins");

        assert!(request.starts_with("POST /api/v1/errors HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer test-key"));
        assert!(request.contains("Content-Type: application/json"));
        assert!(request.contains("\"service\":\"glance-next\""));
        assert!(request.contains("\"error_class\":\"glance-next.test.mock\""));
        assert!(request.contains("\"message\":\"boom\""));
        assert!(request.contains("\"severity\":\"error\""));
    }

    #[test]
    fn check_in_sends_expected_monitor_to_mock_server() {
        let _guard = env_lock().lock().expect("env lock");
        let (endpoint, server, rx) = one_shot_mock_server();

        set_test_env(&endpoint);
        check_in();
        flush();
        clear_test_env();

        let request = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("mock server received a request");
        server.join().expect("server thread joins");

        assert!(request.starts_with("POST /api/v1/check-ins HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer test-key"));
        assert!(request.contains("\"monitor\":\"glance-next\""));
        assert!(request.contains("\"status\":\"alive\""));
        assert!(request.contains("\"ttl_ms\":120000"));
    }

    #[test]
    fn report_error_to_dead_port_returns_without_hanging_or_panicking() {
        let _guard = env_lock().lock().expect("env lock");
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let addr = listener.local_addr().expect("local addr");
        drop(listener); // free the port; nothing listens on it anymore

        set_test_env(&format!("http://{addr}"));
        let started = Instant::now();
        report_error("glance-next.test.dead_port", "unreachable");
        flush();
        let elapsed = started.elapsed();
        clear_test_env();

        assert!(
            elapsed < Duration::from_secs(15),
            "flush took {elapsed:?}, expected a bounded return even against a dead port"
        );
    }

    #[test]
    fn config_is_none_without_credentials() {
        let _guard = env_lock().lock().expect("env lock");
        clear_test_env();
        assert!(config().is_none());
    }
}
