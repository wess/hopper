//! The low-level Docker Engine API client.
//!
//! Speaks HTTP/1.1 to the daemon over whichever transport `endpoint.rs`
//! resolves. Every domain module (containers, images, …) is built on the
//! `json` / `action` / `ndjson` / `stream` helpers here.
//!
//! Two deliberate choices:
//!
//! * **A fresh connection per request.** The endpoint can change at runtime
//!   (a provider bringing up its own socket) and the engine can restart under
//!   us, so a pool would mostly serve stale sockets. Local socket connects are
//!   cheap; correctness is worth more here than the handshake we save.
//! * **Cancellation by drop.** Streaming calls are plain futures — dropping
//!   one closes its connection, so a view that goes away cannot leak a stream.
//!   There is no separate abort registry to get out of sync.

use crate::endpoint::{self, Endpoint};
use crate::error::{DockerError, Result};
use crate::transport;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::{Method, Request, Response};
use hyper_util::rt::TokioIo;
use serde::de::DeserializeOwned;
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// The API version Hopper is written against. Newer daemons keep serving it;
/// [`Client::negotiate`] steps *down* when a daemon is older.
pub const TARGET_API: &str = "1.43";
/// The oldest version whose responses still decode into our shapes.
pub const MIN_API: &str = "1.24";

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Compare two dotted API versions ("1.43" < "1.9" is false — these are
/// numeric per component, not lexical).
fn version_le(a: &str, b: &str) -> bool {
    let parse = |v: &str| -> (u32, u32) {
        let mut it = v.trim_start_matches('v').split('.');
        (
            it.next().and_then(|s| s.parse().ok()).unwrap_or(0),
            it.next().and_then(|s| s.parse().ok()).unwrap_or(0),
        )
    };
    parse(a) <= parse(b)
}

/// Pick the version to speak: ours, unless the daemon is older.
pub fn negotiated_version(daemon_max: &str) -> String {
    if daemon_max.is_empty() || version_le(TARGET_API, daemon_max) {
        TARGET_API.to_string()
    } else if version_le(daemon_max, MIN_API) {
        MIN_API.to_string()
    } else {
        daemon_max.trim_start_matches('v').to_string()
    }
}

struct Inner {
    endpoint: RwLock<Endpoint>,
    api: RwLock<String>,
    timeout: RwLock<Duration>,
}

#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

impl Client {
    pub fn new(ep: Endpoint) -> Self {
        Self {
            inner: Arc::new(Inner {
                endpoint: RwLock::new(ep),
                api: RwLock::new(TARGET_API.to_string()),
                timeout: RwLock::new(DEFAULT_TIMEOUT),
            }),
        }
    }

    /// Resolve the endpoint from the process environment.
    pub fn from_env() -> Self {
        Self::new(endpoint::from_env())
    }

    pub fn endpoint(&self) -> Endpoint {
        self.inner.endpoint.read().unwrap().clone()
    }

    /// Point the client at a different daemon. Takes effect on the next
    /// request — nothing caches the target.
    pub fn set_endpoint(&self, ep: Endpoint) {
        *self.inner.endpoint.write().unwrap() = ep;
    }

    pub fn api_version(&self) -> String {
        self.inner.api.read().unwrap().clone()
    }

    pub fn set_timeout(&self, d: Duration) {
        *self.inner.timeout.write().unwrap() = d;
    }

    fn timeout(&self) -> Duration {
        *self.inner.timeout.read().unwrap()
    }

    /// Ask the daemon what it speaks and step down if it is older than our
    /// target. The Bun build pinned v1.43 unconditionally, which failed
    /// outright against engines predating it.
    pub async fn negotiate(&self) -> Result<String> {
        // `/version` is served unversioned, so this works before we know what
        // version to prefix with.
        let res: serde_json::Value = self.json(Req::get("/version").unversioned()).await?;
        let daemon = res
            .get("ApiVersion")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let chosen = negotiated_version(daemon);
        *self.inner.api.write().unwrap() = chosen.clone();
        Ok(chosen)
    }

    fn url_for(&self, req: &Req) -> String {
        let mut url = String::new();
        if req.versioned {
            url.push_str("/v");
            url.push_str(&self.api_version());
        }
        url.push_str(&req.path);
        if !req.query.is_empty() {
            url.push('?');
            let pairs: Vec<String> = req
                .query
                .iter()
                .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
                .collect();
            url.push_str(&pairs.join("&"));
        }
        url
    }

    fn build(&self, req: &Req) -> Result<Request<Full<Bytes>>> {
        let ep = self.endpoint();
        let mut builder = Request::builder()
            .method(req.method.clone())
            .uri(self.url_for(req))
            .header(hyper::header::HOST, ep.host_header());
        for (k, v) in &req.headers {
            builder = builder.header(k.as_str(), v.as_str());
        }
        let body = match &req.body {
            Some(ReqBody::Json(v)) => {
                builder = builder.header(hyper::header::CONTENT_TYPE, "application/json");
                Full::new(Bytes::from(serde_json::to_vec(v)?))
            }
            Some(ReqBody::Raw { bytes, content_type }) => {
                builder = builder.header(hyper::header::CONTENT_TYPE, content_type.as_str());
                Full::new(bytes.clone())
            }
            None => {
                // Docker requires a content-type on bodyless POSTs to some
                // endpoints; an explicit empty JSON body keeps them happy.
                if req.method == Method::POST {
                    builder = builder.header(hyper::header::CONTENT_TYPE, "application/json");
                }
                Full::new(Bytes::new())
            }
        };
        builder
            .body(body)
            .map_err(|e| DockerError::transport(format!("Malformed request: {e}")))
    }

    /// Send a request and hand back the raw response. Non-2xx becomes a
    /// [`DockerError`] carrying the daemon's own message.
    pub async fn send(&self, req: Req) -> Result<Response<Incoming>> {
        let ep = self.endpoint();
        let deadline = req.timeout.unwrap_or_else(|| self.timeout());
        let http = self.build(&req)?;

        let fut = async {
            let io = TokioIo::new(transport::connect(&ep).await?);
            let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
                .await
                .map_err(DockerError::from)?;
            // The connection task must outlive this call: a streaming body is
            // read after `send_request` returns.
            tokio::spawn(async move {
                let _ = conn.await;
            });
            sender.send_request(http).await.map_err(DockerError::from)
        };

        let res = match tokio::time::timeout(deadline, fut).await {
            Ok(r) => r?,
            Err(_) => {
                return Err(DockerError::timeout(format!(
                    "The Docker daemon did not respond within {}s ({}).",
                    deadline.as_secs(),
                    ep.describe()
                )))
            }
        };

        if res.status().is_success() {
            return Ok(res);
        }
        Err(Self::error_from(res).await)
    }

    /// Turn a failed response into an error carrying the daemon's message.
    async fn error_from(res: Response<Incoming>) -> DockerError {
        let status = res.status();
        let fallback = || {
            status
                .canonical_reason()
                .map(|r| format!("{} {}", status.as_u16(), r))
                .unwrap_or_else(|| format!("HTTP {}", status.as_u16()))
        };
        let body = match res.into_body().collect().await {
            Ok(b) => b.to_bytes(),
            Err(_) => return DockerError::api(status.as_u16(), fallback()),
        };
        let message = serde_json::from_slice::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| {
                v.get("message")
                    .and_then(|m| m.as_str())
                    .map(str::to_string)
            })
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(fallback);
        DockerError::api(status.as_u16(), message)
    }

    /// Read a whole response body.
    pub async fn bytes(&self, req: Req) -> Result<Bytes> {
        let res = self.send(req).await?;
        Ok(res.into_body().collect().await?.to_bytes())
    }

    /// JSON request — the common case.
    pub async fn json<T: DeserializeOwned + Default>(&self, req: Req) -> Result<T> {
        let res = self.send(req).await?;
        if res.status() == hyper::StatusCode::NO_CONTENT {
            return Ok(T::default());
        }
        let body = res.into_body().collect().await?.to_bytes();
        if body.is_empty() {
            return Ok(T::default());
        }
        serde_json::from_slice(&body).map_err(|e| {
            DockerError::decode(format!(
                "The daemon sent a response Hopper could not read: {e}"
            ))
        })
    }

    /// POST/DELETE with no meaningful body (start, stop, remove, …).
    pub async fn action(&self, req: Req) -> Result<()> {
        let res = self.send(req).await?;
        // Drain so the connection closes cleanly rather than being reset.
        let _ = res.into_body().collect().await;
        Ok(())
    }

    /// Like [`Client::action`], but Docker's 304 ("already in that state")
    /// counts as success — starting a running container is not an error the
    /// user needs to see.
    pub async fn idempotent(&self, req: Req) -> Result<()> {
        match self.action(req).await {
            Err(e) if e.is_not_modified() => Ok(()),
            other => other,
        }
    }

    /// Stream a response body, handing each chunk to `on_chunk`.
    ///
    /// Returning `false` from the callback stops the stream and closes the
    /// connection.
    pub async fn stream<F>(&self, req: Req, mut on_chunk: F) -> Result<()>
    where
        F: FnMut(Bytes) -> bool,
    {
        let res = self.send(req).await?;
        let mut body = res.into_body();
        while let Some(frame) = body.frame().await {
            let frame = frame?;
            if let Ok(data) = frame.into_data() {
                if !data.is_empty() && !on_chunk(data) {
                    break;
                }
            }
        }
        Ok(())
    }

    /// Stream a newline-delimited JSON body (events, stats, pull, build),
    /// yielding one decoded value per line.
    ///
    /// Lines are reassembled across chunk boundaries; an undecodable line is
    /// skipped rather than ending the stream, because the daemon interleaves
    /// frames we do not model.
    pub async fn ndjson<T, F>(&self, req: Req, mut on_item: F) -> Result<()>
    where
        T: DeserializeOwned,
        F: FnMut(T) -> bool,
    {
        let mut buf: Vec<u8> = Vec::new();
        let mut stop = false;
        self.stream(req, |chunk| {
            buf.extend_from_slice(&chunk);
            while let Some(nl) = buf.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = buf.drain(..=nl).collect();
                let line = &line[..line.len() - 1];
                if line.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                if let Ok(item) = serde_json::from_slice::<T>(line) {
                    if !on_item(item) {
                        stop = true;
                        return false;
                    }
                }
            }
            true
        })
        .await?;

        // A final line with no trailing newline still counts.
        if !stop && !buf.is_empty() {
            if let Ok(item) = serde_json::from_slice::<T>(&buf) {
                on_item(item);
            }
        }
        Ok(())
    }

    /// Perform a request that upgrades the connection, handing back the raw
    /// duplex stream. This is how interactive exec hijacks the socket for a
    /// real TTY.
    pub async fn upgrade(&self, req: Req) -> Result<TokioIo<hyper::upgrade::Upgraded>> {
        let ep = self.endpoint();
        let http = self.build(&req)?;
        let io = TokioIo::new(transport::connect(&ep).await?);
        let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
            .await
            .map_err(DockerError::from)?;
        tokio::spawn(async move {
            let _ = conn.with_upgrades().await;
        });
        let res = sender.send_request(http).await.map_err(DockerError::from)?;
        let status = res.status();
        if !status.is_success() && status != hyper::StatusCode::SWITCHING_PROTOCOLS {
            return Err(Self::error_from(res).await);
        }
        let upgraded = hyper::upgrade::on(res)
            .await
            .map_err(|e| DockerError::transport(format!("Could not hijack the connection: {e}")))?;
        Ok(TokioIo::new(upgraded))
    }

    /// `GET /_ping` — the liveness probe the status poll runs.
    pub async fn ping(&self) -> Result<()> {
        self.action(
            Req::get("/_ping")
                .unversioned()
                .timeout(Duration::from_secs(5)),
        )
        .await
    }
}

/// Render a request's URL. Sibling modules build query strings that are worth
/// asserting on without standing up a daemon.
#[cfg(test)]
pub(crate) fn render_for_test(client: &Client, req: &Req) -> String {
    client.url_for(req)
}

/// A request body.
enum ReqBody {
    Json(serde_json::Value),
    Raw {
        bytes: Bytes,
        content_type: String,
    },
}

/// A request under construction.
pub struct Req {
    method: Method,
    path: String,
    query: Vec<(String, String)>,
    headers: Vec<(String, String)>,
    body: Option<ReqBody>,
    versioned: bool,
    timeout: Option<Duration>,
}

impl Req {
    fn new(method: Method, path: impl Into<String>) -> Self {
        Self {
            method,
            path: path.into(),
            query: Vec::new(),
            headers: Vec::new(),
            body: None,
            versioned: true,
            timeout: None,
        }
    }

    pub fn get(path: impl Into<String>) -> Self {
        Self::new(Method::GET, path)
    }

    pub fn post(path: impl Into<String>) -> Self {
        Self::new(Method::POST, path)
    }

    pub fn put(path: impl Into<String>) -> Self {
        Self::new(Method::PUT, path)
    }

    pub fn delete(path: impl Into<String>) -> Self {
        Self::new(Method::DELETE, path)
    }

    pub fn head(path: impl Into<String>) -> Self {
        Self::new(Method::HEAD, path)
    }

    /// Add a query parameter.
    pub fn query(mut self, key: &str, value: impl ToString) -> Self {
        self.query.push((key.to_string(), value.to_string()));
        self
    }

    /// Add a query parameter only when `value` is `Some`.
    pub fn query_opt(self, key: &str, value: Option<impl ToString>) -> Self {
        match value {
            Some(v) => self.query(key, v),
            None => self,
        }
    }

    /// Add a boolean query parameter only when it is true — Docker treats a
    /// present `false` differently from absent on several endpoints.
    pub fn flag(self, key: &str, value: bool) -> Self {
        if value {
            self.query(key, "1")
        } else {
            self
        }
    }

    pub fn header(mut self, key: &str, value: impl Into<String>) -> Self {
        self.headers.push((key.to_string(), value.into()));
        self
    }

    pub fn json_body(mut self, body: impl serde::Serialize) -> Self {
        self.body = Some(ReqBody::Json(
            serde_json::to_value(body).unwrap_or(serde_json::Value::Null),
        ));
        self
    }

    pub fn raw_body(mut self, bytes: impl Into<Bytes>, content_type: &str) -> Self {
        self.body = Some(ReqBody::Raw {
            bytes: bytes.into(),
            content_type: content_type.to_string(),
        });
        self
    }

    /// Send without the `/v1.43` prefix (`/_ping`, `/version`).
    pub fn unversioned(mut self) -> Self {
        self.versioned = false;
        self
    }

    pub fn timeout(mut self, d: Duration) -> Self {
        self.timeout = Some(d);
        self
    }

    /// Streaming endpoints have no meaningful deadline — the whole point is to
    /// stay open — so they opt out of the request timeout.
    pub fn no_timeout(mut self) -> Self {
        self.timeout = Some(Duration::from_secs(60 * 60 * 24));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_comparison_is_numeric_not_lexical() {
        assert!(version_le("1.9", "1.43"));
        assert!(!version_le("1.43", "1.9"));
        assert!(version_le("1.43", "1.43"));
    }

    #[test]
    fn negotiation_keeps_our_target_against_a_newer_daemon() {
        assert_eq!(negotiated_version("1.51"), TARGET_API);
        assert_eq!(negotiated_version("1.43"), TARGET_API);
    }

    #[test]
    fn negotiation_steps_down_for_an_older_daemon() {
        assert_eq!(negotiated_version("1.41"), "1.41");
        assert_eq!(negotiated_version("v1.40"), "1.40");
    }

    #[test]
    fn negotiation_floors_at_the_oldest_version_we_can_decode() {
        assert_eq!(negotiated_version("1.20"), MIN_API);
    }

    #[test]
    fn negotiation_falls_back_when_the_daemon_reports_nothing() {
        assert_eq!(negotiated_version(""), TARGET_API);
    }

    #[test]
    fn urls_carry_the_version_prefix_unless_opted_out() {
        let c = Client::new(Endpoint::Unix {
            path: "/tmp/x.sock".into(),
        });
        assert_eq!(
            c.url_for(&Req::get("/containers/json")),
            "/v1.43/containers/json"
        );
        assert_eq!(c.url_for(&Req::get("/_ping").unversioned()), "/_ping");
    }

    #[test]
    fn query_parameters_are_percent_encoded() {
        let c = Client::new(Endpoint::default());
        let req = Req::get("/containers/json")
            .query("filters", r#"{"status":["running"]}"#)
            .flag("all", true);
        let url = c.url_for(&req);
        assert!(url.contains("filters=%7B%22status%22%3A%5B%22running%22%5D%7D"));
        assert!(url.ends_with("&all=1"));
    }

    #[test]
    fn false_flags_are_omitted_entirely() {
        let c = Client::new(Endpoint::default());
        assert_eq!(
            c.url_for(&Req::get("/x").flag("all", false)),
            "/v1.43/x"
        );
    }

    #[test]
    fn optional_query_parameters_skip_when_absent() {
        let c = Client::new(Endpoint::default());
        let none: Option<String> = None;
        assert_eq!(c.url_for(&Req::get("/x").query_opt("tag", none)), "/v1.43/x");
        assert_eq!(
            c.url_for(&Req::get("/x").query_opt("tag", Some("v1"))),
            "/v1.43/x?tag=v1"
        );
    }

    #[test]
    fn the_endpoint_can_change_at_runtime() {
        let c = Client::new(Endpoint::Unix {
            path: "/a.sock".into(),
        });
        c.set_endpoint(Endpoint::Unix {
            path: "/b.sock".into(),
        });
        assert_eq!(c.endpoint().path(), Some("/b.sock"));
    }
}
