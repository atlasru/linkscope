use crate::{model::*,storage::Store};
use anyhow::{bail,Result};
use serde_json::{json,Value};
use std::io::Write;

fn timestamp(t:i64)->String{time::OffsetDateTime::from_unix_timestamp(t).unwrap_or(time::OffsetDateTime::UNIX_EPOCH).format(&time::format_description::well_known::Rfc3339).unwrap()}
fn xml(s:&str)->String{s.chars().filter(|c|*c=='\n'||*c=='\r'||*c=='\t'||*c>=' ').collect::<String>().replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;").replace('"',"&quot;").replace('\'',"&apos;")}
fn stix_type(n:&Node)->&'static str{match n.kind.as_str(){"domain"=>"domain-name","ip"=>if n.value.contains(':'){"ipv6-addr"}else{"ipv4-addr"},"url"=>"url","email"=>"email-addr",_=>"x-linkscope-entity"}}
fn each<T:serde::de::DeserializeOwned>(store:&Store,table:&str,mut f:impl FnMut(T)->Result<()>)->Result<()>{let mut cursor=0;loop{let rows=store.page(table,cursor,256)?;if rows.is_empty(){break}for(row,s)in rows{cursor=row;f(serde_json::from_str(&s)?)?;}}Ok(())}
pub fn write(store:&Store,format:&str,mut out:impl Write)->Result<()> {
    match format{
        "json"=>{
            write!(out,"{{\"version\":1,\"nodes\":[")?;let mut first=true;
            each::<Node>(store,"nodes",|n|{if !first{write!(out,",")?}first=false;serde_json::to_writer(&mut out,&n)?;Ok(())})?;
            write!(out,"],\"edges\":[")?;first=true;
            each::<Edge>(store,"edges",|e|{if !first{write!(out,",")?}first=false;serde_json::to_writer(&mut out,&e)?;Ok(())})?;write!(out,"]}}")?;
        },
        "graphml"=>{
            write!(out,"<?xml version=\"1.0\" encoding=\"UTF-8\"?><graphml xmlns=\"http://graphml.graphdrawing.org/xmlns\"><key id=\"payload\" for=\"all\" attr.name=\"payload\" attr.type=\"string\"/><graph id=\"investigation\" edgedefault=\"directed\">")?;
            each::<Node>(store,"nodes",|n|{write!(out,"<node id=\"{}\"><data key=\"payload\">{}</data></node>",xml(&n.id),xml(&serde_json::to_string(&n)?))?;Ok(())})?;
            each::<Edge>(store,"edges",|e|{write!(out,"<edge id=\"{}\" source=\"{}\" target=\"{}\"><data key=\"payload\">{}</data></edge>",xml(&e.id),xml(&e.source),xml(&e.target),xml(&serde_json::to_string(&e)?))?;Ok(())})?;write!(out,"</graph></graphml>")?;
        },
        "stix"=>{
            write!(out,"{{\"type\":\"bundle\",\"id\":\"bundle--{}\",\"objects\":[",uuid::Uuid::new_v4())?;let mut first=true;
            each::<Node>(store,"nodes",|n|{if !first{write!(out,",")?}first=false;let t=stix_type(&n);let mut obj=json!({"type":t,"spec_version":"2.1","id":format!("{t}--{}",n.id),"value":n.value,"x_linkscope_kind":n.kind,"x_linkscope_attributes":n.attributes,"x_linkscope_sources":n.sources,"x_linkscope_confidence":n.confidence});
                if t.starts_with("x-"){obj["created"]=json!(timestamp(n.timestamp));obj["modified"]=json!(timestamp(n.timestamp));}
                serde_json::to_writer(&mut out,&obj)?;Ok(())})?;
            each::<Edge>(store,"edges",|e|{let source=store.node(&e.source)?.ok_or_else(||anyhow::anyhow!("missing source"))?;let target=store.node(&e.target)?.ok_or_else(||anyhow::anyhow!("missing target"))?;
                if !first{write!(out,",")?}first=false;serde_json::to_writer(&mut out,&json!({"type":"relationship","spec_version":"2.1","id":format!("relationship--{}",e.id),"created":timestamp(e.timestamp),"modified":timestamp(e.timestamp),"relationship_type":"related-to","source_ref":format!("{}--{}",stix_type(&source),source.id),"target_ref":format!("{}--{}",stix_type(&target),target.id),"x_linkscope_relation":e.relation,"x_linkscope_provenance":e.provenance}))?;Ok(())})?;write!(out,"]}}")?;
        },
        "misp"=>{
            write!(out,"{{\"Event\":{{\"uuid\":\"{}\",\"info\":\"LinkScope local export\",\"distribution\":\"0\",\"analysis\":\"0\",\"threat_level_id\":\"4\",\"published\":false,\"Attribute\":[",uuid::Uuid::new_v4())?;let mut first=true;
            each::<Node>(store,"nodes",|n|{if !first{write!(out,",")?}first=false;let(t,cat)=match n.kind.as_str(){"domain"=>("domain","Network activity"),"ip"=>("ip-dst","Network activity"),"url"=>("url","Network activity"),"email"=>("email-src","Payload delivery"),_=>("text","Other")};
                serde_json::to_writer(&mut out,&json!({"uuid":n.id,"type":t,"category":cat,"value":n.value,"to_ids":false,"timestamp":n.timestamp.to_string(),"comment":serde_json::to_string(&n.attributes)?}))?;Ok(())})?;
            // Preserve graph links as event-level local extension; MISP may drop it.
            write!(out,"],\"x_linkscope_edges\":[")?;first=true;
            each::<Edge>(store,"edges",|e|{if !first{write!(out,",")?}first=false;serde_json::to_writer(&mut out,&e)?;Ok(())})?;write!(out,"]}}}}")?;
        },
        _=>bail!("unsupported export format")
    }Ok(())
}
pub fn import_json(bytes:&[u8])->Result<Batch>{if bytes.len()>2*1024*1024{bail!("import batch exceeds 2 MiB")};let v:Value=serde_json::from_slice(bytes)?;let b:Batch=serde_json::from_value(v)?;b.validate()?;Ok(b)}

