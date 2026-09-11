use crate::{engine::EngineHandle, model::now};
use anyhow::{bail, Result};
use futures::StreamExt;
use reqwest::{Client, Proxy, redirect::Policy};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, net::{IpAddr, SocketAddr}, sync::Arc, time::Duration};
use tokio::sync::{Mutex, Semaphore};
use tokio_util::sync::CancellationToken;
use url::Url;

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(tag="mode",rename_all="snake_case")]
pub enum Route { Offline, DirectDoh { resolver: String }, Proxy { url: String }, Tor, Chain }
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct NetworkConfig { pub anonymous: bool, pub default_route: Route, #[serde(default)] pub sources: HashMap<String,Route> }
impl Default for NetworkConfig { fn default()->Self{Self{anonymous:true,default_route:Route::Offline,sources:HashMap::new()}} }

pub fn validate_route(route:&Route,anonymous:bool)->Result<()> {
    match route {
        Route::Offline=>Ok(()),
        Route::DirectDoh{resolver}=>{if anonymous{bail!("anonymous mode requires a proxy")}if resolver!="cloudflare"&&resolver!="google"{bail!("unsupported resolver")}Ok(())},
        Route::Proxy{url}=>{let u=Url::parse(url)?;if !matches!(u.scheme(),"socks5h"|"http"|"https"){bail!("use socks5h, http or https; socks5 may resolve locally")}
            if u.host_str().and_then(|h|h.trim_matches(['[',']']).parse::<IpAddr>().ok()).is_none(){bail!("proxy must have a literal IP address to avoid bootstrap DNS")}
            if !u.username().is_empty()||u.password().is_some()||u.query().is_some()||u.fragment().is_some(){bail!("proxy credentials/parameters are not supported in config")};Ok(())},
        Route::Tor=>bail!("embedded Arti transport is not implemented; no direct fallback"),
        Route::Chain=>bail!("proxy chains are not implemented; no direct fallback"),
    }
}
struct DenyDns;
impl reqwest::dns::Resolve for DenyDns {
    fn resolve(&self,_:reqwest::dns::Name)->reqwest::dns::Resolving {
        Box::pin(async{Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied,"system DNS disabled").into())})
    }
}
struct Gate { limit:Semaphore, clock:Mutex<GateState> }
struct GateState { next:tokio::time::Instant, failures:u8, open_until:tokio::time::Instant }
pub struct Network { config:NetworkConfig, gates:Mutex<HashMap<String,Arc<Gate>>>, engine:EngineHandle }
impl Network {
    pub fn new(config:NetworkConfig,engine:EngineHandle)->Result<Self>{validate_route(&config.default_route,config.anonymous)?;for r in config.sources.values(){validate_route(r,config.anonymous)?;}Ok(Self{config,gates:Mutex::new(HashMap::new()),engine})}
    fn builder()->reqwest::ClientBuilder {Client::builder().no_proxy().dns_resolver(Arc::new(DenyDns)).redirect(Policy::none()).retry(reqwest::retry::never()).https_only(true).timeout(Duration::from_secs(25)).connect_timeout(Duration::from_secs(10)).pool_max_idle_per_host(2).user_agent("LinkScope/0.1")}
    async fn client(&self,route:&Route,target:&Url)->Result<Client>{
        match route{
            Route::Proxy{url}=>Ok(Self::builder().proxy(Proxy::all(url)?).build()?),
            Route::DirectDoh{resolver}=>{
                let host=target.host_str().ok_or_else(||anyhow::anyhow!("missing host"))?;
                if host.parse::<IpAddr>().is_ok(){return Ok(Self::builder().build()?)}
                let (doh,ip)=if resolver=="google"{("dns.google","8.8.8.8")}else{("cloudflare-dns.com","1.1.1.1")};
                let bootstrap=Self::builder().resolve(doh,SocketAddr::new(ip.parse()?,443)).build()?;
                let path=if resolver=="google"{"resolve"}else{"dns-query"};
                let r=bootstrap.get(format!("https://{doh}/{path}")).query(&[("name",host),("type","A")]).header("accept","application/dns-json").send().await?;
                if !r.status().is_success(){bail!("DoH resolution failed")}
                let bytes=read_bounded(r,64*1024).await?;let json:serde_json::Value=serde_json::from_slice(&bytes)?;
                let addrs:Vec<SocketAddr>=json["Answer"].as_array().into_iter().flatten().filter(|v|v["type"]==1).filter_map(|v|v["data"].as_str()?.parse::<IpAddr>().ok()).map(|ip|SocketAddr::new(ip,443)).collect();
                if addrs.is_empty(){bail!("DoH returned no addresses")};Ok(Self::builder().resolve_to_addrs(host,&addrs).build()?)
            },
            _=>bail!("network route unavailable")
        }
    }
    pub async fn get(&self,source:&str,url:Url,ttl:i64,cancel:&CancellationToken)->Result<Vec<u8>>{
        if url.scheme()!="https"||url.host_str().is_none()||!url.username().is_empty()||url.password().is_some(){bail!("only credential-free HTTPS URLs are accepted")}
        let route=self.config.sources.get(source).unwrap_or(&self.config.default_route);
        if matches!(route,Route::Offline){bail!("source is offline; configure a route")}
        validate_route(route,self.config.anonymous)?;
        let key=format!("{:x}",Sha256::digest(format!("{source}|{}|{}",url,serde_json::to_string(route)?)));
        let k=key.clone();let cached=self.engine.call(move|e|{let v=e.store.cache_get(&k)?;if v.is_some(){e.cache_hits+=1}else{e.cache_misses+=1};Ok(v)}).await?;
        if let Some(v)=cached{return Ok(v)}
        let gate={let mut gates=self.gates.lock().await;gates.entry(source.into()).or_insert_with(||Arc::new(Gate{limit:Semaphore::new(2),clock:Mutex::new(GateState{next:tokio::time::Instant::now(),failures:0,open_until:tokio::time::Instant::now()})})).clone()};
        tokio::select!{_=cancel.cancelled()=>bail!("cancelled"),result=async{
            let _permit=gate.limit.acquire().await?;
            let client=self.client(route,&url).await.map_err(|_|anyhow::anyhow!("transport or DoH failed"))?;
            for attempt in 0..3u32 {
                let when={let mut state=gate.clock.lock().await;let now=tokio::time::Instant::now();if state.open_until>now{bail!("source circuit is open")};let when=state.next.max(now);state.next=when+Duration::from_millis(1100);when};
                tokio::time::sleep_until(when).await;
                let accept=if url.host_str()==Some("cloudflare-dns.com"){"application/dns-json"}else{"application/json, text/plain;q=0.8"};
                let response=client.get(url.clone()).header("accept",accept).send().await;
                let mut delay=Duration::from_millis((1u64<<attempt)*500+(now() as u64%211));
                match response {
                    Ok(r) if r.status().is_success()=>{
                        let body=read_bounded(r,2*1024*1024).await?;gate.clock.lock().await.failures=0;
                        let k=key.clone();let b=body.clone();self.engine.call(move|e|e.store.cache_put(&k,&b,ttl)).await?;return Ok(body)
                    },
                    Ok(r) if r.status().as_u16()==429||r.status().is_server_error()=>{if let Some(secs)=r.headers().get("retry-after").and_then(|v|v.to_str().ok()).and_then(|s|s.parse::<u64>().ok()){delay=Duration::from_secs(secs.min(60));}},
                    Ok(r)=>bail!("source returned HTTP {} (redirects are disabled)",r.status().as_u16()),
                    Err(_)=>{}
                }
                {let mut s=gate.clock.lock().await;s.failures=s.failures.saturating_add(1);if s.failures>=5{s.open_until=tokio::time::Instant::now()+Duration::from_secs(30);s.failures=0;}}
                if attempt<2{tokio::time::sleep(delay).await;}
            }
            bail!("source unavailable after three attempts")
        }=>result}
    }
}
async fn read_bounded(r:reqwest::Response,limit:usize)->Result<Vec<u8>>{
    if r.content_length().unwrap_or(0)>limit as u64{bail!("response exceeds byte limit")}
    let mut stream=r.bytes_stream();let mut out=Vec::new();while let Some(chunk)=stream.next().await{let chunk=chunk?;if out.len()+chunk.len()>limit{bail!("response exceeds byte limit")};out.extend_from_slice(&chunk);}Ok(out)
}
