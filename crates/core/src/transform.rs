use crate::{engine::EngineHandle, model::*, network::Network};
use anyhow::{bail,Result};
use async_trait::async_trait;
use futures::{future::BoxFuture, stream, FutureExt, StreamExt};
use serde::{Deserialize,Serialize};
use std::{path::Path,sync::Arc};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use url::Url;

#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct Manifest {
    pub id:String, pub name:String, pub input:String, pub group:String,
    pub endpoint:String, pub host:String, pub parser:String,
    #[serde(default)]pub pointer:String, #[serde(default)]pub output_kind:String,
    #[serde(default="default_ttl")]pub ttl:i64,
}
fn default_ttl()->i64{3600}
#[async_trait]
pub trait Transform:Send+Sync {
    fn manifest(&self)->&Manifest;
    async fn run(&self,input:&Node,network:&Network,cancel:&CancellationToken)->Result<Batch>;
}
pub struct Registration(pub fn()->Manifest);
inventory::collect!(Registration);
pub struct HttpTransform(pub Manifest);
#[async_trait]
impl Transform for HttpTransform {
    fn manifest(&self)->&Manifest{&self.0}
    async fn run(&self,input:&Node,network:&Network,cancel:&CancellationToken)->Result<Batch>{
        let m=&self.0;if input.kind!=m.input{bail!("transform input kind mismatch")}
        let encoded:String=url::form_urlencoded::byte_serialize(input.value.as_bytes()).collect();
        let u=Url::parse(&m.endpoint.replace("{value}",&encoded))?;
        if u.host_str()!=Some(m.host.as_str()){bail!("plugin endpoint escaped its declared host")}
        let bytes=network.get(&m.id,u,m.ttl,cancel).await?;
        parse(m,input,&bytes)
    }
}
pub fn parse(m:&Manifest,input:&Node,bytes:&[u8])->Result<Batch>{
    let json:serde_json::Value=serde_json::from_slice(bytes)?;let mut values:Vec<(String,String)>=Vec::new();
    match m.parser.as_str(){
        "doh"=>{for a in json["Answer"].as_array().into_iter().flatten(){if a["type"]==1||a["type"]==28{if let Some(s)=a["data"].as_str(){if s.parse::<std::net::IpAddr>().is_ok(){values.push(("ip".into(),s.into()));}}}}},
        "crt"=>{for r in json.as_array().into_iter().flatten().take(10000){if let Some(names)=r["name_value"].as_str(){for n in names.lines().take(50){values.push(("domain".into(),n.trim_start_matches("*.").into()));}}if values.len()>1024{break}}},
        "certspotter"=>{for r in json.as_array().into_iter().flatten(){for n in r["dns_names"].as_array().into_iter().flatten(){if let Some(n)=n.as_str(){values.push(("domain".into(),n.trim_start_matches("*.").into()));}}if values.len()>1024{break}}},
        "internetdb"=>{for n in json["hostnames"].as_array().into_iter().flatten(){if let Some(n)=n.as_str(){values.push(("domain".into(),n.into()));}}},
        "urlscan"=>{for r in json["results"].as_array().into_iter().flatten(){if let Some(n)=r["page"]["url"].as_str(){values.push(("url".into(),n.into()));}}},
        "cdx"=>{for r in json.as_array().into_iter().flatten().skip(1){if let Some(n)=r.get(0).and_then(|n|n.as_str()){values.push(("url".into(),n.into()));}}},
        "pointer"=>{if let Some(v)=json.pointer(&m.pointer){if let Some(a)=v.as_array(){for v in a.iter().take(512){if let Some(s)=v.as_str(){values.push((m.output_kind.clone(),s.into()));}}}else if let Some(s)=v.as_str(){values.push((m.output_kind.clone(),s.into()));}}},
        "attributes"=>{},
        _=>bail!("unknown parser")
    }
    let mut batch=Batch::default();let mut seen=std::collections::HashSet::new();
    let mut root=input.clone();
    if m.parser=="attributes" {
        // Bound retained data independently from response size.
        if bytes.len()>48*1024{bail!("attribute response exceeds retained payload limit")}
        root.attributes.insert(m.id.clone(),json);
    }
    root.sources.push(m.id.clone());batch.nodes.push(root);
    for(kind,value)in values.into_iter().take(1024){
        let mut node=Node::new(&kind,&value,&m.id);if node.id==input.id||!seen.insert(node.id.clone()){continue}if node.validate().is_err(){continue}
        let angle=batch.nodes.len() as f32*2.399963;node.x=input.x+angle.cos()*90.;node.y=input.y+angle.sin()*90.;
        batch.edges.push(Edge::new(&input.id,&node.id,"observed",&m.id));batch.nodes.push(node);if batch.nodes.len()>=256{break}
    }
    batch.validate()?;Ok(batch)
}
pub fn load_plugins(path:Option<&Path>)->Result<Vec<Arc<dyn Transform>>>{
    let mut manifests:Vec<_>=inventory::iter::<Registration>.into_iter().map(|r|(r.0)()).collect();
    if let Some(path)=path {if path.exists(){for file in std::fs::read_dir(path)?.take(128){let p=file?.path();if p.extension().and_then(|s|s.to_str())==Some("json"){if p.metadata()?.len()>32768{bail!("plugin manifest too large")};manifests.push(serde_json::from_slice(&std::fs::read(p)?)?);}}}}
    let mut ids=std::collections::HashSet::new();let mut result=Vec::new();
    for m in manifests {
        if !ids.insert(m.id.clone())||m.id.len()>64||m.id.is_empty(){bail!("invalid or duplicate plugin id")}
        let u=Url::parse(&m.endpoint.replace("{value}","example.org"))?;
        if u.scheme()!="https"||u.host_str()!=Some(m.host.as_str())||!u.username().is_empty()||u.password().is_some(){bail!("invalid plugin host")}
        if !["doh","crt","certspotter","internetdb","urlscan","cdx","pointer","attributes"].contains(&m.parser.as_str()){bail!("unsupported parser")}
        result.push(Arc::new(HttpTransform(m))as Arc<dyn Transform>);
    }Ok(result)
}
macro_rules! builtin {($id:literal,$name:literal,$input:literal,$endpoint:literal,$host:literal,$parser:literal)=>{
    inventory::submit!{Registration(||Manifest{id:$id.into(),name:$name.into(),input:$input.into(),group:"data".into(),endpoint:$endpoint.into(),host:$host.into(),parser:$parser.into(),pointer:String::new(),output_kind:String::new(),ttl:3600})}
}}
builtin!("cloudflare","Cloudflare · DNS A","domain","https://cloudflare-dns.com/dns-query?name={value}&type=A","cloudflare-dns.com","doh");
builtin!("google","Google · DNS A","domain","https://dns.google/resolve?name={value}&type=A","dns.google","doh");
builtin!("crtsh","Certificate Transparency","domain","https://crt.sh/?q=%25.{value}&output=json","crt.sh","crt");
builtin!("certspotter","Cert Spotter","domain","https://api.certspotter.com/v1/issuances?domain={value}&include_subdomains=true&expand=dns_names","api.certspotter.com","certspotter");
builtin!("internetdb","Shodan InternetDB","ip","https://internetdb.shodan.io/{value}","internetdb.shodan.io","internetdb");
builtin!("urlscan","urlscan · passive search","domain","https://urlscan.io/api/v1/search/?q=domain:{value}&size=100","urlscan.io","urlscan");
builtin!("wayback","Wayback CDX","domain","https://web.archive.org/cdx/search/cdx?url={value}%2F*&output=json&fl=original&collapse=urlkey&limit=100","web.archive.org","cdx");
builtin!("rdap","RDAP · .com domains","domain","https://rdap.verisign.com/com/v1/domain/{value}","rdap.verisign.com","attributes");
builtin!("github","GitHub · public profile","username","https://api.github.com/users/{value}","api.github.com","attributes");
builtin!("hackernews","Hacker News · user","username","https://hacker-news.firebaseio.com/v0/user/{value}.json","hacker-news.firebaseio.com","attributes");
builtin!("keybase","Keybase · public lookup","username","https://keybase.io/_/api/1.0/user/lookup.json?usernames={value}","keybase.io","attributes");

#[derive(Clone,Serialize)]
pub struct Progress { pub source:String,pub status:String,pub done:usize,pub total:usize,pub batch:Option<Batch>,pub error:Option<String> }
pub async fn run_all(input:Node,transforms:Vec<Arc<dyn Transform>>,network:Arc<Network>,engine:EngineHandle,tx:mpsc::Sender<Progress>,cancel:CancellationToken){
    let total=transforms.len();let mut done=0;
    // Materialize owned, Send futures before constructing the buffered stream.
    // This avoids a higher-ranked trait-object lifetime in the iterator closure
    // escaping into the task passed to Tauri/Tokio spawn.
    let mut pending: Vec<BoxFuture<'static, (String, Result<Batch>)>> = Vec::new();
    for transform in transforms {
        let node = input.clone();
        let network = network.clone();
        let cancel = cancel.clone();
        pending.push(async move {
            let id = transform.manifest().id.clone();
            let result = transform.run(&node, &network, &cancel).await;
            (id, result)
        }.boxed());
    }
    let tasks = stream::iter(pending).buffer_unordered(10);
    tokio::pin!(tasks);
    loop {
        let next=tokio::select!{_=cancel.cancelled()=>break,next=tasks.next()=>next};let Some((source,result))=next else{break};done+=1;
        let result=match result{Ok(batch)=>engine.call(move|e|e.apply(batch)).await,Err(e)=>Err(e)};
        let p=match result{Ok(batch)=>Progress{source,status:"complete".into(),done,total,batch:Some(batch),error:None},Err(e)=>Progress{source,status:"error".into(),done,total,batch:None,error:Some(e.to_string())}};
        tokio::select!{_=cancel.cancelled()=>break,r=tx.send(p)=>if r.is_err(){break}}
    }
    let final_event=Progress{source:String::new(),status:if cancel.is_cancelled(){"cancelled"}else{"finished"}.into(),done,total,batch:None,error:None};
    let _=tokio::time::timeout(std::time::Duration::from_secs(2),tx.send(final_event)).await;
}
