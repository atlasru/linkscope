use super::*;
use crate::service::{Response, Transport, RESPONSE_LIMIT};
use anyhow::Result;
use async_trait::async_trait;
use linkscope_engine::{model::Entity, Graph};
use reqwest::Url;
use serde_json::json;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{sync::Mutex, time::Instant};
use tokio_util::sync::CancellationToken;

struct Mock {
    calls: Mutex<Vec<(Instant, String)>>,
    delay: Duration,
    body: Option<Vec<u8>>,
    active: AtomicUsize,
    peak: AtomicUsize,
}

impl Mock {
    fn new(delay: Duration, body: Option<Vec<u8>>) -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(Vec::new()),
            delay,
            body,
            active: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
        })
    }
}

struct Active<'a>(&'a AtomicUsize);
impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

#[async_trait]
impl Transport for Mock {
    async fn fetch(&self, url: Url) -> Result<Response> {
        self.calls
            .lock()
            .await
            .push((Instant::now(), url.to_string()));
        let count = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(count, Ordering::SeqCst);
        let _active = Active(&self.active);
        tokio::time::sleep(self.delay).await;
        let data = if url.host_str() == Some("dns.google") {
            let ipv4 = url
                .query_pairs()
                .any(|(key, value)| key == "type" && value == "A");
            json!({"Status":0,"AD":true,"Question":[{"name":"example.com.","type":if ipv4 {1} else {28}}],
                "Answer":[{"name":"example.com.","type":if ipv4 {1} else {28},"TTL":300,"data":if ipv4 {"192.0.2.1"} else {"2001:db8::1"}}]})
        } else if url.host_str() == Some("crt.sh") {
            json!([{"id":12,"common_name":"example.com","name_value":"example.com\nwww.example.com"}])
        } else {
            json!({"objectClassName":"domain","ldhName":"EXAMPLE.COM","status":["active"]})
        };
        Ok(Response {
            body: self
                .body
                .clone()
                .unwrap_or_else(|| serde_json::to_vec(&data).unwrap()),
            uri: url.to_string(),
            retrieved_at: "2026-10-16T10:00:00Z".into(),
            redirect: None,
        })
    }
}

fn input() -> Entity {
    Entity {
        id: "original".into(),
        kind: "domain".into(),
        label: "Example.COM.".into(),
        ..Entity::default()
    }
}

fn service(mock: Arc<Mock>) -> Service {
    let mut service = Service::with_transport(mock);
    service.limits(
        Duration::from_secs(2),
        Duration::from_millis(1),
        Duration::from_secs(60),
    );
    service
}

#[tokio::test]
async fn dns_outputs_are_attributed_and_apply_as_one_undoable_patch() {
    let mock = Mock::new(Duration::ZERO, None);
    let service = service(mock.clone());
    let output = service
        .run("dns", &input(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(output.patch.entities.len(), 2);
    assert_eq!(output.patch.relationships.len(), 2);
    assert!(!output.cached);
    for entity in &output.patch.entities {
        assert_eq!(entity.kind, "ip");
        assert_eq!(
            entity.sources[0].retrieved_at.as_deref(),
            Some("2026-10-16T10:00:00Z")
        );
        assert!(entity.sources[0].record.is_some());
    }
    assert!(output
        .patch
        .relationships
        .iter()
        .all(|edge| edge.source == "original" && edge.directed && edge.kind == "resolves-to"));
    let calls = mock.calls.lock().await;
    assert_eq!(calls.len(), 2);
    assert!(calls[0].1.contains("name=example.com&type=A"));
    assert!(calls[1].1.contains("type=AAAA"));
    let mut graph = Graph::empty();
    graph
        .apply(linkscope_engine::model::Patch {
            entities: vec![input()],
            ..Default::default()
        })
        .unwrap();
    graph.apply(output.patch).unwrap();
    assert_eq!(graph.entity_count(), 3);
    graph.undo().unwrap();
    assert_eq!(graph.entity_count(), 1);
    assert_eq!(graph.relationship_count(), 0);
}

#[tokio::test]
async fn cache_replay_preserves_time_and_creates_separate_observations() {
    let mock = Mock::new(Duration::ZERO, None);
    let service = service(mock.clone());
    let first = service
        .run("dns", &input(), CancellationToken::new())
        .await
        .unwrap();
    let second = service
        .run("dns", &input(), CancellationToken::new())
        .await
        .unwrap();
    assert!(second.cached);
    assert_eq!(mock.calls.lock().await.len(), 2);
    assert_ne!(first.patch.entities[0].id, second.patch.entities[0].id);
    assert_eq!(
        first.patch.entities[0].sources[0].retrieved_at,
        second.patch.entities[0].sources[0].retrieved_at
    );
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(service
        .run("dns", &input(), cancel)
        .await
        .unwrap_err()
        .to_string()
        .contains("cancelled"));
}

#[tokio::test]
async fn cancellation_interrupts_transport_and_releases_concurrency() {
    let mock = Mock::new(Duration::from_secs(10), None);
    let service = service(mock.clone());
    let cancel = CancellationToken::new();
    let start = Instant::now();
    let entity = input();
    let future = service.run("rdap", &entity, cancel.clone());
    let cancellation = async {
        tokio::time::sleep(Duration::from_millis(20)).await;
        cancel.cancel();
    };
    let (result, _) = tokio::join!(future, cancellation);
    assert!(result.unwrap_err().to_string().contains("cancelled"));
    assert!(start.elapsed() < Duration::from_millis(500));
    assert_eq!(mock.active.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn cancellation_interrupts_rate_limit_wait_without_another_request() {
    let mock = Mock::new(Duration::ZERO, None);
    let mut service = service(mock.clone());
    service.limits(
        Duration::from_secs(2),
        Duration::from_secs(1),
        Duration::from_secs(60),
    );
    let cancel = CancellationToken::new();
    let entity = input();
    let future = service.run("dns", &entity, cancel.clone());
    let cancellation = async {
        tokio::time::sleep(Duration::from_millis(40)).await;
        cancel.cancel();
    };
    let (result, _) = tokio::join!(future, cancellation);
    assert!(result.unwrap_err().to_string().contains("cancelled"));
    assert_eq!(mock.calls.lock().await.len(), 1);
}

#[tokio::test]
async fn whole_job_deadline_interrupts_http() {
    let mock = Mock::new(Duration::from_secs(10), None);
    let mut service = service(mock.clone());
    service.limits(
        Duration::from_millis(30),
        Duration::ZERO,
        Duration::from_secs(60),
    );
    assert!(service
        .run("rdap", &input(), CancellationToken::new())
        .await
        .unwrap_err()
        .to_string()
        .contains("timed out"));
    assert_eq!(mock.active.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn deadline_and_cancellation_also_cover_concurrency_queue_waits() {
    let mock = Mock::new(Duration::ZERO, None);
    let mut service = service(mock.clone());
    service.limits(
        Duration::from_millis(40),
        Duration::ZERO,
        Duration::from_secs(60),
    );
    let _workers = service.occupy_workers().await;
    assert!(service
        .run("rdap", &input(), CancellationToken::new())
        .await
        .unwrap_err()
        .to_string()
        .contains("timed out"));
    let cancel = CancellationToken::new();
    let entity = input();
    let future = service.run("rdap", &entity, cancel.clone());
    let cancellation = async {
        tokio::time::sleep(Duration::from_millis(10)).await;
        cancel.cancel();
    };
    let (result, _) = tokio::join!(future, cancellation);
    assert!(result.unwrap_err().to_string().contains("cancelled"));
    assert!(mock.calls.lock().await.is_empty());
}

#[tokio::test]
async fn request_starts_respect_rate_limit() {
    let mock = Mock::new(Duration::ZERO, None);
    let mut service = service(mock.clone());
    service.limits(
        Duration::from_secs(2),
        Duration::from_millis(40),
        Duration::from_secs(60),
    );
    service
        .run("dns", &input(), CancellationToken::new())
        .await
        .unwrap();
    let calls = mock.calls.lock().await;
    assert!(calls[1].0.duration_since(calls[0].0) >= Duration::from_millis(39));
}

#[tokio::test]
async fn active_jobs_are_limited_to_two() {
    let mock = Mock::new(Duration::from_millis(30), None);
    let service = service(mock.clone());
    let a = Entity {
        label: "one.example".into(),
        ..input()
    };
    let b = Entity {
        label: "two.example".into(),
        ..input()
    };
    let c = Entity {
        label: "three.example".into(),
        ..input()
    };
    let (a, b, c) = tokio::join!(
        service.run("rdap", &a, CancellationToken::new()),
        service.run("rdap", &b, CancellationToken::new()),
        service.run("rdap", &c, CancellationToken::new())
    );
    assert!(a.is_ok() && b.is_ok() && c.is_ok());
    assert_eq!(mock.peak.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn queue_is_bounded_and_cache_expiry_refetches() {
    let mock = Mock::new(Duration::ZERO, None);
    let mut service = service(mock.clone());
    service.limits(
        Duration::from_secs(2),
        Duration::ZERO,
        Duration::from_millis(10),
    );
    let permit = service.fill_queue().await;
    assert!(service
        .run("rdap", &input(), CancellationToken::new())
        .await
        .unwrap_err()
        .to_string()
        .contains("queue is full"));
    drop(permit);
    service
        .run("rdap", &input(), CancellationToken::new())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(
        !service
            .run("rdap", &input(), CancellationToken::new())
            .await
            .unwrap()
            .cached
    );
    assert_eq!(mock.calls.lock().await.len(), 2);
}

#[tokio::test]
async fn invalid_types_and_url_injection_are_rejected_before_network() {
    let mock = Mock::new(Duration::ZERO, None);
    let service = service(mock.clone());
    for label in [
        "https://example.com/",
        "example.com?name=secret",
        "user@example.com",
        "-bad.example",
        "example..com",
        "192.0.2.1",
        "localhost",
    ] {
        assert!(
            service
                .run(
                    "dns",
                    &Entity {
                        label: label.into(),
                        ..input()
                    },
                    CancellationToken::new()
                )
                .await
                .is_err(),
            "{label}"
        );
    }
    assert!(service
        .run(
            "dns",
            &Entity {
                kind: "generic".into(),
                ..input()
            },
            CancellationToken::new()
        )
        .await
        .is_err());
    assert!(mock.calls.lock().await.is_empty());
}

#[tokio::test]
async fn oversized_and_malformed_responses_fail_actionably() {
    for body in [
        vec![b' '; RESPONSE_LIMIT + 1],
        b"not json".to_vec(),
        b"[]".to_vec(),
    ] {
        let service = service(Mock::new(Duration::ZERO, Some(body)));
        assert!(service
            .run("rdap", &input(), CancellationToken::new())
            .await
            .is_err());
    }
}

#[tokio::test]
async fn certificate_records_are_deduplicated_capped_and_do_not_infer_domain_identity() {
    let mut records = (0..110).map(|id| json!({"id":id,"common_name":"example.com","name_value":"example.com\nwww.example.com"})).collect::<Vec<_>>();
    records.insert(0, records[0].clone());
    let service = service(Mock::new(
        Duration::ZERO,
        Some(serde_json::to_vec(&records).unwrap()),
    ));
    let output = service
        .run("certificates", &input(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(output.patch.entities.len(), 100);
    assert!(output
        .patch
        .entities
        .iter()
        .all(|entity| entity.kind == "certificate"));
    assert!(output
        .patch
        .relationships
        .iter()
        .all(|edge| edge.kind == "certificate-observed"));
    assert!(output
        .warnings
        .iter()
        .any(|warning| warning.contains("capped")));
    assert!(output
        .warnings
        .iter()
        .any(|warning| warning.contains("ownership")));
}

#[tokio::test]
async fn dns_error_status_and_bad_answers_never_invent_records() {
    let body = br#"{"Status":3,"Answer":[{"type":1,"data":"192.0.2.1"}]}"#.to_vec();
    let output = service(Mock::new(Duration::ZERO, Some(body)))
        .run("dns", &input(), CancellationToken::new())
        .await
        .unwrap();
    assert!(output.patch.is_empty());
    assert!(output
        .warnings
        .iter()
        .any(|warning| warning.contains("not found")));
    let body =
        br#"{"Status":0,"Answer":[{"type":1,"data":"not-ip"},{"type":28,"data":"192.0.2.1"}]}"#
            .to_vec();
    let output = service(Mock::new(Duration::ZERO, Some(body)))
        .run("dns", &input(), CancellationToken::new())
        .await
        .unwrap();
    assert!(output.patch.is_empty());
}

#[tokio::test]
async fn cache_has_a_bounded_number_of_responses() {
    let mock = Mock::new(Duration::ZERO, None);
    let mut service = service(mock.clone());
    service.limits(
        Duration::from_secs(2),
        Duration::ZERO,
        Duration::from_secs(60),
    );
    for number in 0..65 {
        let entity = Entity {
            label: format!("record-{number}.example"),
            ..input()
        };
        service
            .run("rdap", &entity, CancellationToken::new())
            .await
            .unwrap();
    }
    let first = Entity {
        label: "record-0.example".into(),
        ..input()
    };
    assert!(
        !service
            .run("rdap", &first, CancellationToken::new())
            .await
            .unwrap()
            .cached
    );
    assert_eq!(mock.calls.lock().await.len(), 66);
}

struct RedirectMock {
    target: String,
    calls: Mutex<Vec<Instant>>,
    repeat: bool,
}

#[async_trait]
impl Transport for RedirectMock {
    async fn fetch(&self, url: Url) -> Result<Response> {
        self.calls.lock().await.push(Instant::now());
        let redirect = if url.host_str() == Some("rdap.org") || self.repeat {
            Some(Url::parse(&self.target)?)
        } else {
            None
        };
        Ok(Response {
            body: br#"{"objectClassName":"domain"}"#.to_vec(),
            uri: url.to_string(),
            retrieved_at: "2026-10-16T10:00:00Z".into(),
            redirect,
        })
    }
}

#[tokio::test]
async fn registry_redirects_are_rate_limited_and_attribute_final_registry() {
    let mock = Arc::new(RedirectMock {
        target: "https://registry.example/domain/example.com".into(),
        calls: Mutex::new(Vec::new()),
        repeat: false,
    });
    let mut service = Service::with_transport(mock.clone());
    service.limits(
        Duration::from_secs(2),
        Duration::from_millis(40),
        Duration::from_secs(60),
    );
    let output = service
        .run("rdap", &input(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        output.patch.entities[0].sources[0].uri.as_deref(),
        Some("https://registry.example/domain/example.com")
    );
    let calls = mock.calls.lock().await;
    assert_eq!(calls.len(), 2);
    assert!(calls[1].duration_since(calls[0]) >= Duration::from_millis(39));
}

#[tokio::test]
async fn insecure_registry_redirects_and_redirect_loops_fail_closed() {
    for target in [
        "http://registry.example/domain",
        "https://127.0.0.1/domain",
        "https://localhost/domain",
        "https://localhost./domain",
        "https://evil.local/domain",
        "https://user:password@registry.example/domain",
        "https://registry.example:8443/domain",
    ] {
        assert!(!crate::service::safe_redirect(&Url::parse(target).unwrap()));
        let mock = Arc::new(RedirectMock {
            target: target.into(),
            calls: Mutex::new(Vec::new()),
            repeat: false,
        });
        let service = Service::with_transport(mock.clone());
        assert!(service
            .run("rdap", &input(), CancellationToken::new())
            .await
            .is_err());
        assert_eq!(mock.calls.lock().await.len(), 1);
    }
    let mock = Arc::new(RedirectMock {
        target: "https://registry.example/domain/example.com".into(),
        calls: Mutex::new(Vec::new()),
        repeat: true,
    });
    let mut service = Service::with_transport(mock.clone());
    service.limits(
        Duration::from_secs(2),
        Duration::ZERO,
        Duration::from_secs(60),
    );
    assert!(service
        .run("rdap", &input(), CancellationToken::new())
        .await
        .is_err());
    assert_eq!(mock.calls.lock().await.len(), 4);
}
