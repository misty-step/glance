//! Fire-and-forget Canary self-reporter. No creds => silent no-op.
//!
//! `glance` is primarily a short-lived CLI/build tool: a [`check_in`] as
//! early as possible in `main`, a [`report_error`] on the top-level failure
//! path, and — via [`CanaryLayer`] — automatic capture of every
//! `tracing::error!` anywhere in the app or its libraries. Sends run on a
//! detached thread so a Canary outage never slows or blocks generation, but
//! the process must call [`flush`] before exit or the proof event may never
//! leave the machine before the thread is torn down.
//!
//! `glance serve-local` is the one standing-service mode in this binary: it
//! calls [`start_health_loop`] so its monitor stays `alive` for the life of
//! the server, not just at the one check-in `main` already sends.

use std::sync::{Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

const SERVICE: &str = "glance-next"; // overridable via CANARY_SERVICE
const MONITOR: &str = "glance-next"; // must already exist in Canary
const TTL_MS: u64 = 120_000;
const SEND_TIMEOUT: Duration = Duration::from_secs(3);
const CHECKIN_INTERVAL: Duration = Duration::from_secs(60);

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

/// Standing-service mode only (`serve-local`): fire a check-in immediately,
/// then every [`CHECKIN_INTERVAL`] from a named background thread for the
/// life of the process. Without this a long-running mode outlives the
/// [`TTL_MS`] window between `main`'s one-shot [`check_in`] and going
/// falsely `overdue` while perfectly healthy.
pub fn start_health_loop() {
    if config().is_none() {
        return;
    }
    check_in();
    let _ = std::thread::Builder::new()
        .name("canary-health".into())
        .spawn(|| {
            loop {
                std::thread::sleep(CHECKIN_INTERVAL);
                check_in();
                // Bound `pending()` growth across a long-running loop: a
                // standing service never reaches the short-lived `flush()`
                // call at the end of `main`, so each tick reaps its own
                // (and any other) finished send threads here instead of
                // letting join handles accumulate for the process lifetime.
                flush();
            }
        });
}

/// Install a process-wide panic hook that reports `<service>.panic` to
/// Canary before running the previous (default) hook. Safe to call
/// anywhere; a no-op without `CANARY_ENDPOINT`/`CANARY_API_KEY`.
pub fn install_panic_hook() {
    if config().is_none() {
        return;
    }
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|location| format!("{}:{}", location.file(), location.line()))
            .unwrap_or_default();
        let message = panic_payload_message(info.payload());
        report_error(
            &format!("{}.panic", service()),
            &format!("{message} @ {location}"),
        );
        flush(); // best-effort before the process dies
        default_hook(info);
    }));
}

/// Extract a human-readable message from a panic payload. Panics carry
/// either a `&str` (the common `panic!("literal")` case) or a `String` (the
/// `panic!("{}", ...)` / `.expect(msg)` case); anything else has no useful
/// `Display`, so it falls back to a generic label. Pulled out of
/// [`install_panic_hook`]'s closure so the formatting logic is unit
/// testable without installing a process-global hook.
fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|message| (*message).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic".to_owned())
}

/// A `tracing_subscriber::Layer` that forwards every `ERROR`-level event to
/// [`report_error`]. Registering this once at process start turns "app
/// logging" into "error capture": any `tracing::error!(...)` anywhere in
/// `glance` or the crates it depends on lands in Canary with zero per-site
/// wiring. A no-op per event without `CANARY_ENDPOINT`/`CANARY_API_KEY`.
pub struct CanaryLayer;

impl<S> tracing_subscriber::Layer<S> for CanaryLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if config().is_none() || *event.metadata().level() != tracing::Level::ERROR {
            return;
        }
        let mut message = String::new();
        event.record(&mut EventVisitor(&mut message));
        let error_class = format!("{}.{}", service(), event.metadata().target());
        report_error(&error_class, &message);
    }
}

struct EventVisitor<'a>(&'a mut String);

impl tracing::field::Visit for EventVisitor<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if !self.0.is_empty() {
            self.0.push(' ');
        }
        if field.name() == "message" {
            self.0.push_str(&format!("{value:?}"));
        } else {
            self.0.push_str(&format!("{}={:?}", field.name(), value));
        }
    }
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

/// Shared test infrastructure — a real one-shot mock HTTP server plus
/// serialized env-var mutation — used by this module's own tests and (via
/// `pub(crate)`) by `main.rs`'s tests, which need the same mock server to
/// prove `record_run_failure` and image-render failures actually reach
/// [`CanaryLayer`].
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc;

    pub(crate) fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    pub(crate) fn set_test_env(endpoint: &str) {
        // SAFETY: every test that mutates process env serializes on
        // `env_lock` for its whole env-touching window, so no other thread
        // observes a torn read/write of these vars.
        unsafe {
            std::env::set_var("CANARY_ENDPOINT", endpoint);
            std::env::set_var("CANARY_API_KEY", "test-key");
        }
    }

    pub(crate) fn clear_test_env() {
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
    pub(crate) fn one_shot_mock_server() -> (String, JoinHandle<()>, mpsc::Receiver<String>) {
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

    /// Install [`CanaryLayer`](super::CanaryLayer) as the **global** default
    /// tracing subscriber, exactly once for the life of the test binary.
    ///
    /// A per-test `tracing::subscriber::with_default` (thread-local scoping)
    /// looks like the natural fit for test isolation, but `cargo test` runs
    /// many unrelated tests concurrently on other threads that also emit
    /// `tracing::error!` (e.g. any test exercising `run_generation`'s
    /// failure paths). `tracing-core` caches each callsite's `Interest`
    /// process-wide and only invalidates it when a dispatch changes; two
    /// threads independently entering/leaving scoped subscribers around the
    /// same moment can race that cache and cause a real event to be
    /// silently dropped — reproduced empirically, not from a hunch: this
    /// module's forwarding test was flaky *only* when run alongside another
    /// `run_generation`-exercising test under the default parallel harness.
    /// A single global subscriber, installed once, removes the race
    /// entirely and matches production (`main()` also installs one
    /// subscriber, once). Gating stays on [`config`](super::config) — the
    /// same env-var check every other test already serializes on via
    /// [`env_lock`] — so tests that never touch `CANARY_*` env vars remain
    /// unaffected no matter when this fires relative to them.
    pub(crate) fn install_test_subscriber() {
        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::util::SubscriberInitExt;

        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            let _ = tracing_subscriber::registry()
                .with(super::CanaryLayer)
                .try_init();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{
        clear_test_env, env_lock, install_test_subscriber, one_shot_mock_server, set_test_env,
    };
    use super::*;
    use std::net::TcpListener;
    use std::time::Instant;

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

    #[test]
    fn canary_layer_forwards_tracing_error_to_mock_server() {
        let _guard = env_lock().lock().expect("env lock");
        install_test_subscriber();
        let (endpoint, server, rx) = one_shot_mock_server();
        set_test_env(&endpoint);

        tracing::error!(directory = "src/parser", "boom from a page failure");
        flush();
        clear_test_env();

        let request = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("mock server received the tracing::error! forwarded as a Canary error");
        server.join().expect("server thread joins");

        assert!(request.starts_with("POST /api/v1/errors HTTP/1.1"));
        assert!(request.contains("\"service\":\"glance-next\""));
        // error_class = "<service>.<tracing target>"; target defaults to
        // this test module's path, so the class carries provenance without
        // any per-call-site wiring.
        assert!(request.contains("\"error_class\":\"glance-next.glance::canary::tests\""));
        assert!(request.contains("boom from a page failure"));
        assert!(request.contains("directory=\\\"src/parser\\\""));
    }

    #[test]
    fn canary_layer_ignores_non_error_events() {
        let _guard = env_lock().lock().expect("env lock");
        install_test_subscriber();
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
        let addr = listener.local_addr().expect("local addr");
        set_test_env(&format!("http://{addr}"));

        tracing::warn!("not an error, must not be reported");
        tracing::info!("also not an error");
        flush();
        clear_test_env();

        // Nothing was sent: connecting with a short timeout must time out
        // rather than succeed, proving the layer emitted no request.
        listener
            .set_nonblocking(true)
            .expect("set listener nonblocking");
        assert!(
            listener.accept().is_err(),
            "CanaryLayer must not forward WARN/INFO events to Canary"
        );
    }

    #[test]
    fn panic_payload_message_extracts_str_and_string_payloads() {
        let str_payload: Box<dyn std::any::Any + Send> = Box::new("boom");
        assert_eq!(panic_payload_message(str_payload.as_ref()), "boom");

        let string_payload: Box<dyn std::any::Any + Send> = Box::new(String::from("kaboom"));
        assert_eq!(panic_payload_message(string_payload.as_ref()), "kaboom");

        let opaque_payload: Box<dyn std::any::Any + Send> = Box::new(42_i32);
        assert_eq!(panic_payload_message(opaque_payload.as_ref()), "panic");
    }
}
