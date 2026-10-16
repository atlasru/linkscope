use crate::{service::Response, Output};
use anyhow::{ensure, Context, Result};
use linkscope_engine::model::{Entity, Relationship, SourceRef};
use serde_json::{json, Value};
use std::collections::HashSet;
use uuid::Uuid;

fn source(response: &Response, label: &str, record: Value) -> SourceRef {
    SourceRef {
        id: Uuid::new_v4().to_string(),
        label: label.into(),
        uri: Some(response.uri.clone()),
        retrieved_at: Some(response.retrieved_at.clone()),
        record: Some(record),
    }
}

fn append(
    output: &mut Output,
    input: &Entity,
    kind: &str,
    label: String,
    relation: &str,
    provenance: SourceRef,
    record: Value,
) {
    let angle = output.patch.entities.len() as f64 * 2.399963;
    let radius = 110.0 + (output.patch.entities.len() / 15) as f64 * 45.0;
    let entity = Entity {
        id: Uuid::new_v4().to_string(),
        kind: kind.into(),
        label,
        x: input.x + angle.cos() * radius,
        y: input.y + angle.sin() * radius,
        properties: [("record".into(), record)].into(),
        sources: vec![provenance.clone()],
        ..Entity::default()
    };
    output.patch.relationships.push(Relationship {
        id: Uuid::new_v4().to_string(),
        source: input.id.clone(),
        target: entity.id.clone(),
        kind: relation.into(),
        label: relation.replace('-', " "),
        sources: vec![provenance],
        ..Relationship::default()
    });
    output.patch.entities.push(entity);
}

pub(crate) fn dns(
    response: &Response,
    input: &Entity,
    record_type: &str,
    output: &mut Output,
) -> Result<()> {
    let data: Value =
        serde_json::from_slice(&response.body).context("DNS service returned invalid JSON")?;
    let status = data
        .get("Status")
        .and_then(Value::as_u64)
        .context("DNS response is missing its status")?;
    if status != 0 {
        output.warnings.push(format!(
            "DNS {record_type}: {}",
            match status {
                3 => "domain not found".to_owned(),
                2 => "resolver could not complete the query".to_owned(),
                n => format!("resolver status {n}"),
            }
        ));
        return Ok(());
    }
    let expected = if record_type == "A" { 1 } else { 28 };
    let mut seen = HashSet::new();
    for answer in data["Answer"].as_array().into_iter().flatten() {
        if answer["type"].as_u64() != Some(expected) {
            continue;
        }
        let Some(address) = answer["data"]
            .as_str()
            .and_then(|value| value.parse::<std::net::IpAddr>().ok())
        else {
            output
                .warnings
                .push(format!("Ignored an invalid DNS {record_type} address"));
            continue;
        };
        if (record_type == "A") != address.is_ipv4() {
            output
                .warnings
                .push("Ignored DNS record with mismatched address family".into());
            continue;
        }
        if !seen.insert(address) {
            continue;
        }
        if output.patch.entities.len() >= 256 {
            output
                .warnings
                .push("DNS preview capped at 256 addresses".into());
            break;
        }
        let record = json!({"answer":answer,"status":status,"question":data["Question"],"authenticatedData":data["AD"]});
        append(
            output,
            input,
            "ip",
            address.to_string(),
            "resolves-to",
            source(response, "Google Public DNS", record.clone()),
            record,
        );
    }
    Ok(())
}

pub(crate) fn rdap(response: &Response, input: &Entity, cached: bool) -> Result<Output> {
    let data: Value =
        serde_json::from_slice(&response.body).context("Registry returned invalid JSON")?;
    ensure!(data.is_object(), "Registry response is not an object");
    ensure!(
        data.get("errorCode").is_none(),
        "Registry response reports error {}",
        data["errorCode"]
    );
    ensure!(
        data.get("objectClassName")
            .and_then(Value::as_str)
            .is_some(),
        "Registry response is missing its object class"
    );
    let mut output = Output {
        cached,
        ..Output::default()
    };
    append(
        &mut output,
        input,
        "registration-data",
        format!("Registration: {}", input.label),
        "registration-record",
        source(response, "RDAP registry", data.clone()),
        data,
    );
    output.warnings.push(
        "Registry records may be redacted or outdated; they do not prove current ownership".into(),
    );
    Ok(output)
}

pub(crate) fn certificates(response: &Response, input: &Entity, cached: bool) -> Result<Output> {
    let data: Value = serde_json::from_slice(&response.body)
        .context("Certificate service returned invalid JSON")?;
    let records = data
        .as_array()
        .context("Certificate response is not an array")?;
    let mut output = Output {
        cached,
        ..Output::default()
    };
    let mut seen = HashSet::new();
    for record in records {
        let Some(id) = record.get("id").and_then(|id| match id {
            Value::Number(n) => Some(n.to_string()),
            Value::String(s) if s.len() <= 128 => Some(s.clone()),
            _ => None,
        }) else {
            output
                .warnings
                .push("Ignored a certificate record without a usable record ID".into());
            continue;
        };
        if !seen.insert(id.clone()) {
            continue;
        }
        if output.patch.entities.len() == 100 {
            output
                .warnings
                .push("Certificate preview capped at 100 distinct records".into());
            break;
        }
        let name = record["common_name"]
            .as_str()
            .filter(|name| name.len() <= 512)
            .unwrap_or("Certificate");
        let mut provenance = source(response, "crt.sh certificate transparency", record.clone());
        // IDs originate in a service response, so encode them through URL APIs.
        let mut uri = reqwest::Url::parse("https://crt.sh/")?;
        uri.query_pairs_mut().append_pair("id", &id);
        provenance.uri = Some(uri.to_string());
        append(
            &mut output,
            input,
            "certificate",
            format!("{name} · {id}"),
            "certificate-observed",
            provenance,
            record.clone(),
        );
    }
    if output.patch.entities.is_empty() {
        output
            .warnings
            .push("No certificate records returned".into());
    } else {
        output.warnings.push(
            "A certificate record does not establish common ownership or a current live service"
                .into(),
        );
    }
    Ok(output)
}
