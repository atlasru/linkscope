use anyhow::{bail, ensure, Context, Result};
use async_trait::async_trait;
use futures::StreamExt;
use linkscope_engine::model::{Entity, Patch};
use reqwest::{redirect::Policy, Client, Url};
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, sync::Arc, time::Duration};
use tokio::{
    sync::{Mutex, Semaphore},
    time::Instant,
};
use tokio_util::sync::CancellationToken;

pub(crate) const RESPONSE_LIMIT: usize = 2 * 1024 * 1024;
const CACHE_LIMIT: usize = 16 * 1024 * 1024;
const CACHE_ENTRIES: usize = 64;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub label: String,
    pub inputs: Vec<String>,
    pub destination: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Output {
    pub patch: Patch,
    pub warnings: Vec<String>,
    pub cached: bool,
}

#[derive(Clone)]
pub(crate) struct Response {
    pub body: Vec<u8>,
    pub uri: String,
    /// RFC 3339 UTC; the original retrieval time is retained on cache replay.
    pub retrieved_at: String,
    pub redirect: Option<Url>,
}

#[async_trait]
pub(crate) trait Transport: Send + Sync {
    async fn fetch(&self, url: Url) -> Result<Response>;
}

struct HttpTransport {
    client: Client,
}

/// Cross-host redirects are needed by RDAP bootstrap. Only public HTTPS names
/// on the default TLS port are followed, with a short redirect chain.
pub(crate) fn safe_redirect(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_end_matches('.');
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && host.contains('.')
        && host != "localhost"
        && host.parse::<std::net::IpAddr>().is_err()
        && !host.ends_with(".local")
        && !host.ends_with(".internal")
        && !host.ends_with(".localhost")
}

#[async_trait]
impl Transport for HttpTransport {
    async fn fetch(&self, url: Url) -> Result<Response> {
        let response = self
            .client
            .get(url)
            .header("accept", "application/rdap+json, application/json")
            .send()
            .await
            .context("Public-data service could not be reached")?;
        let status = response.status();
        if status.is_redirection() {
            let location = response
                .headers()
                .get("location")
                .and_then(|value| value.to_str().ok())
                .context("Registry redirect is missing a valid Location header")?;
            return Ok(Response {
                body: Vec::new(),
                uri: response.url().to_string(),
                retrieved_at: String::new(),
                redirect: Some(
                    response
                        .url()
                        .join(location)
                        .context("Registry redirect URL is invalid")?,
                ),
            });
        }
        if !status.is_success() {
            let retry = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok());
            if status.as_u16() == 429 {
                bail!(
                    "Service rate limit reached; retry later{}",
                    retry
                        .map(|v| format!(" (Retry-After: {v})"))
                        .unwrap_or_default()
                );
            }
            bail!("Public-data service returned HTTP {}", status.as_u16());
        }
        ensure!(
            response.content_length().unwrap_or(0) <= RESPONSE_LIMIT as u64,
            "Response exceeds 2 MiB limit"
        );
        let uri = response.url().to_string();
        let mut stream = response.bytes_stream();
        let mut body = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Service response was interrupted")?;
            ensure!(
                body.len() + chunk.len() <= RESPONSE_LIMIT,
                "Response exceeds 2 MiB limit"
            );
            body.extend_from_slice(&chunk);
        }
        Ok(Response {
            body,
            uri,
            retrieved_at: time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)?,
            redirect: None,
        })
    }
}

struct CacheEntry {
    key: String,
    response: Response,
    expires: Instant,
}

pub struct Service {
    transport: Arc<dyn Transport>,
    active: Semaphore,
    admitted: Semaphore,
    next_request: Mutex<Instant>,
    cache: Mutex<VecDeque<CacheEntry>>,
    timeout: Duration,
    interval: Duration,
    ttl: Duration,
}

impl Service {
    pub fn new() -> Result<Self> {
        // Select one TLS provider explicitly rather than bringing aws-lc's
        // additional native toolchain into the desktop dependency graph.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = Client::builder()
            .https_only(true)
            .no_proxy()
            // Redirects go through the same request-start gate as initial
            // requests; reqwest auto-follow would bypass rate limiting.
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(18))
            .user_agent("LinkScope/2.0 public-data research")
            .build()?;
        Ok(Self::with_transport(Arc::new(HttpTransport { client })))
    }

    pub fn manifests() -> Vec<Manifest> {
        vec![
            Manifest {
                id: "dns".into(),
                label: "Resolve DNS".into(),
                inputs: vec!["domain".into()],
                destination: "dns.google (A and AAAA)".into(),
            },
            Manifest {
                id: "rdap".into(),
                label: "Registration data".into(),
                inputs: vec!["domain".into(), "ip".into()],
                destination: "rdap.org → public HTTPS registry".into(),
            },
            Manifest {
                id: "certificates".into(),
                label: "Certificate records".into(),
                inputs: vec!["domain".into()],
                destination: "crt.sh".into(),
            },
        ]
    }

    pub(crate) fn with_transport(transport: Arc<dyn Transport>) -> Self {
        Self {
            transport,
            active: Semaphore::new(2),
            admitted: Semaphore::new(32),
            next_request: Mutex::new(Instant::now()),
            cache: Mutex::new(VecDeque::new()),
            timeout: Duration::from_secs(20),
            interval: Duration::from_millis(500),
            ttl: Duration::from_secs(300),
        }
    }

    /// Waiting, rate limiting, transport and parsing share one twenty-second
    /// deadline. A cancelled cache replay also returns cancellation, not data.
    pub async fn run(
        &self,
        kind: &str,
        input: &Entity,
        cancel: CancellationToken,
    ) -> Result<Output> {
        ensure!(!input.id.is_empty(), "Select a saved entity first");
        ensure!(
            input.x.is_finite() && input.y.is_finite(),
            "Input position is invalid"
        );
        let value = validate_input(kind, input)?;
        let _admission = self
            .admitted
            .try_acquire()
            .map_err(|_| anyhow::anyhow!("Research queue is full; retry after a job completes"))?;
        tokio::select! {
            biased;
            _ = cancel.cancelled() => bail!("Research cancelled"),
            result = tokio::time::timeout(self.timeout, async {
                let _permit = self.active.acquire().await.context("Research service stopped")?;
                self.run_inner(kind, input, &value).await
            }) => result.map_err(|_| anyhow::anyhow!("Research timed out after {} seconds", self.timeout.as_secs()))?,
        }
    }

    async fn run_inner(&self, kind: &str, input: &Entity, value: &str) -> Result<Output> {
        match kind {
            "dns" => {
                let mut output = Output {
                    cached: true,
                    ..Output::default()
                };
                for record_type in ["A", "AAAA"] {
                    let mut url = Url::parse("https://dns.google/resolve")?;
                    url.query_pairs_mut()
                        .append_pair("name", value)
                        .append_pair("type", record_type);
                    let (response, cached) = self.request(url).await?;
                    output.cached &= cached;
                    crate::parsers::dns(&response, input, record_type, &mut output)?;
                }
                if output.patch.entities.is_empty() && output.warnings.is_empty() {
                    output.warnings.push("No A or AAAA records returned".into());
                }
                Ok(output)
            }
            "rdap" => {
                let path = if input.kind == "ip" { "ip" } else { "domain" };
                let mut url = Url::parse("https://rdap.org/")?;
                url.path_segments_mut()
                    .map_err(|_| anyhow::anyhow!("Invalid RDAP endpoint"))?
                    .extend([path, value]);
                let (response, cached) = self.request(url).await?;
                crate::parsers::rdap(&response, input, cached)
            }
            "certificates" => {
                let mut url = Url::parse("https://crt.sh/")?;
                url.query_pairs_mut()
                    .append_pair("q", &format!("%.{value}"))
                    .append_pair("output", "json");
                let (response, cached) = self.request(url).await?;
                crate::parsers::certificates(&response, input, cached)
            }
            _ => bail!("Unknown research action"),
        }
    }

    async fn request(&self, url: Url) -> Result<(Response, bool)> {
        let key = url.as_str().to_owned();
        {
            let mut cache = self.cache.lock().await;
            cache.retain(|entry| entry.expires > Instant::now());
            if let Some(entry) = cache.iter().find(|entry| entry.key == key) {
                return Ok((entry.response.clone(), true));
            }
        }
        let from_rdap = url.host_str() == Some("rdap.org");
        let mut current = url;
        let mut redirects = 0;
        let response = loop {
            // Hold the gate through its wait, then advance from the actual
            // start time to avoid closely spaced starts after executor stalls.
            let mut next = self.next_request.lock().await;
            tokio::time::sleep_until(*next).await;
            *next = Instant::now() + self.interval;
            drop(next);
            let response = self.transport.fetch(current).await?;
            if let Some(target) = response.redirect {
                ensure!(
                    from_rdap && redirects < 3 && safe_redirect(&target),
                    "Service redirect is not a permitted public HTTPS registry URL"
                );
                current = target;
                redirects += 1;
            } else {
                break response;
            }
        };
        ensure!(
            response.body.len() <= RESPONSE_LIMIT,
            "Response exceeds 2 MiB limit"
        );
        // A transient HTML/error document returned with HTTP 200 must not
        // poison otherwise useful retries for the entire cache TTL.
        serde_json::from_slice::<serde_json::Value>(&response.body)
            .context("Public-data service returned invalid JSON")?;
        let mut cache = self.cache.lock().await;
        cache.retain(|entry| entry.key != key && entry.expires > Instant::now());
        while cache.len() >= CACHE_ENTRIES
            || cache.iter().map(|e| e.response.body.len()).sum::<usize>() + response.body.len()
                > CACHE_LIMIT
        {
            if cache.pop_front().is_none() {
                break;
            }
        }
        cache.push_back(CacheEntry {
            key,
            response: response.clone(),
            expires: Instant::now() + self.ttl,
        });
        Ok((response, false))
    }

    #[cfg(test)]
    pub(crate) fn limits(&mut self, timeout: Duration, interval: Duration, ttl: Duration) {
        self.timeout = timeout;
        self.interval = interval;
        self.ttl = ttl;
    }

    #[cfg(test)]
    pub(crate) async fn fill_queue(&self) -> tokio::sync::SemaphorePermit<'_> {
        self.admitted.acquire_many(32).await.unwrap()
    }

    #[cfg(test)]
    pub(crate) async fn occupy_workers(&self) -> tokio::sync::SemaphorePermit<'_> {
        self.active.acquire_many(2).await.unwrap()
    }
}

fn validate_input(kind: &str, input: &Entity) -> Result<String> {
    ensure!(
        Service::manifests()
            .iter()
            .any(|m| m.id == kind && m.inputs.contains(&input.kind)),
        "Research action does not support this entity type"
    );
    let label = input.label.trim().trim_end_matches('.');
    if input.kind == "ip" {
        return Ok(label
            .parse::<std::net::IpAddr>()
            .context("Enter a valid IPv4 or IPv6 address")?
            .to_string());
    }
    ensure!(
        (1..=253).contains(&label.len()) && label.is_ascii(),
        "Enter an ASCII domain name (use punycode for international domains)"
    );
    ensure!(
        label.contains('.')
            && label.split('.').all(|part| !part.is_empty()
                && part.len() <= 63
                && !part.starts_with('-')
                && !part.ends_with('-')
                && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')),
        "Enter a valid domain name"
    );
    ensure!(
        label.parse::<std::net::IpAddr>().is_err(),
        "Use an IP entity for an address"
    );
    Ok(label.to_ascii_lowercase())
}
