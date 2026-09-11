#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use linkscope_core::{audit::Audit,engine::EngineHandle,model::*,network::{Network,NetworkConfig},transform::{self,Manifest,Progress,Transform},vault::Vault};
use serde::{Deserialize,Serialize};
use std::{path::PathBuf,sync::Arc};
use tauri::{Manager,State};
use tokio::sync::{mpsc,Mutex};
use tokio_util::sync::CancellationToken;

#[derive(Clone,Serialize,Deserialize)]
struct Config { memory_mb:usize, network:NetworkConfig }
impl Default for Config {fn default()->Self{Self{memory_mb:512,network:NetworkConfig::default()}}}
struct Job { rx:mpsc::Receiver<Progress>,cancel:CancellationToken }
struct App {
    engine:EngineHandle, network:Mutex<Arc<Network>>, config:Mutex<Config>,
    transforms:Vec<Arc<dyn Transform>>, job:Mutex<Option<Job>>, vault:Mutex<Vault>,
    config_dir:PathBuf,data_dir:PathBuf,ephemeral:bool,
}
type Reply<T>=Result<T,String>;
fn err(e:impl std::fmt::Display)->String{e.to_string()}
#[tauri::command]
async fn info(app:State<'_,App>)->Reply<serde_json::Value>{Ok(serde_json::json!({"ephemeral":app.ephemeral,"config":app.config.lock().await.clone(),"transforms":app.transforms.iter().map(|t|t.manifest()).collect::<Vec<_>>()}))}
#[tauri::command]
async fn add_node(kind:String,value:String,app:State<'_,App>)->Reply<Batch>{let n=Node::new(&kind,&value,"analyst");app.engine.call(move|e|e.apply(Batch{nodes:vec![n],edges:vec![]})).await.map_err(err)}
#[tauri::command]
async fn apply_batch(batch:Batch,app:State<'_,App>)->Reply<Batch>{app.engine.call(move|e|e.apply(batch)).await.map_err(err)}
#[derive(Deserialize)]
struct Position {id:String,x:f32,y:f32}
#[tauri::command]
async fn positions(positions:Vec<Position>,app:State<'_,App>)->Reply<()>{
    if positions.len()>256{return Err("position batch exceeds 256 nodes".into())}
    app.engine.call(move|e|{let mut batch=Batch::default();for p in positions{if !p.x.is_finite()||!p.y.is_finite(){anyhow::bail!("invalid coordinates")};if let Some(mut n)=e.store.node(&p.id)?{n.x=p.x;n.y=p.y;batch.nodes.push(n);}}e.graph.admit(&batch)?;e.store.commit(&batch)?;e.graph.apply(batch);Ok(())}).await.map_err(err)
}
#[tauri::command]
async fn page(table:String,after:i64,app:State<'_,App>)->Reply<serde_json::Value>{app.engine.call(move|e|{let rows=e.store.page(&table,after,512)?;let cursor=rows.last().map_or(after,|r|r.0);let items:Vec<serde_json::Value>=rows.into_iter().map(|(_,s)|serde_json::from_str(&s)).collect::<Result<_,_>>()?;Ok(serde_json::json!({"cursor":cursor,"items":items}))}).await.map_err(err)}
#[tauri::command]
async fn node(id:String,app:State<'_,App>)->Reply<Option<Node>>{app.engine.call(move|e|e.node(&id)).await.map_err(err)}
#[tauri::command]
async fn search(query:String,app:State<'_,App>)->Reply<Vec<String>>{if query.len()>512{return Err("query too long".into())}app.engine.call(move|e|e.search(&query)).await.map_err(err)}
#[tauri::command]
async fn stats(app:State<'_,App>)->Reply<linkscope_core::engine::Stats>{app.engine.call(|e|Ok(e.stats())).await.map_err(err)}
#[tauri::command]
async fn run_transforms(id:String,sources:Vec<String>,app:State<'_,App>)->Reply<usize>{
    let input=app.engine.call(move|e|e.node(&id)).await.map_err(err)?.ok_or("node not found")?;
    let list:Vec<_>=app.transforms.iter().filter(|t|t.manifest().input==input.kind&&sources.contains(&t.manifest().id)).cloned().collect();let total=list.len();if total==0{return Err("no compatible transforms selected".into())}
    let mut job=app.job.lock().await;if let Some(old)=job.as_ref(){if !old.rx.is_closed(){return Err("another transform job is active".into())}}
    let(tx,rx)=mpsc::channel(16);let cancel=CancellationToken::new();*job=Some(Job{rx,cancel:cancel.clone()});
    tauri::async_runtime::spawn(transform::run_all(input,list,app.network.lock().await.clone(),app.engine.clone(),tx,cancel));Ok(total)
}
#[tauri::command]
async fn poll_job(app:State<'_,App>)->Reply<Vec<Progress>>{let mut guard=app.job.lock().await;let mut out=Vec::new();if let Some(job)=guard.as_mut(){for _ in 0..4{match job.rx.try_recv(){Ok(p)=>out.push(p),Err(_)=>break}}}Ok(out)}
#[tauri::command]
async fn cancel_job(app:State<'_,App>)->Reply<()>{if let Some(job)=app.job.lock().await.as_ref(){job.cancel.cancel()}Ok(())}
#[tauri::command]
async fn configure(config:Config,app:State<'_,App>)->Reply<()>{
    if !(256..=4096).contains(&config.memory_mb){return Err("memory budget must be 256–4096 MiB".into())}
    if app.job.lock().await.as_ref().is_some_and(|j|!j.rx.is_closed()){return Err("cancel and drain the active job before changing routing".into())}
    let net=Network::new(config.network.clone(),app.engine.clone()).map_err(err)?;
    if !app.ephemeral{let path=app.config_dir.join("config.json");let data=serde_json::to_vec_pretty(&config).map_err(err)?;tauri::async_runtime::spawn_blocking(move||std::fs::write(path,data)).await.map_err(err)?.map_err(err)?;}
    *app.config.lock().await=config;*app.network.lock().await=Arc::new(net);Ok(())
}
#[tauri::command]
async fn export_graph(format:String,app:State<'_,App>)->Reply<String>{
    if !["json","graphml","stix","misp"].contains(&format.as_str()){return Err("unsupported format".into())}
    let directory=app.data_dir.join("exports");let suffix=if format=="graphml"{"graphml"}else{"json"};let path=directory.join(format!("investigation-{}-{}.{}",now(),format,suffix));let display=path.to_string_lossy().to_string();
    app.engine.call(move|e|{std::fs::create_dir_all(directory)?;let file=std::fs::OpenOptions::new().write(true).create_new(true).open(&path)?;let mut writer=std::io::BufWriter::new(file);let result=linkscope_core::export::write(&e.store,&format,&mut writer);use std::io::Write;writer.flush()?;if result.is_err(){drop(writer);let _=std::fs::remove_file(&path);}result}).await.map_err(err)?;Ok(display)
}
#[tauri::command]
async fn vault_save(source:String,key:String,passphrase:String,app:State<'_,App>)->Reply<()>{
    if app.ephemeral{return Err("vault persistence is disabled in ephemeral mode".into())}
    if source.len()>64||key.len()>4096{return Err("invalid key size".into())}
    let path=app.data_dir.join("keys.age");let mut guard=app.vault.lock().await;let mut vault=std::mem::take(&mut *guard);
    let result=tauri::async_runtime::spawn_blocking(move||{
        let pass=zeroize::Zeroizing::new(passphrase);
        let r=(||->anyhow::Result<()>{if path.exists(){vault.unlock(&path,zeroize::Zeroizing::new(pass.to_string()))?;}vault.set(source,key);vault.save(&path,pass)})();(vault,r)
    }).await.map_err(err)?;
    *guard=result.0;result.1.map_err(err)
}
#[tauri::command]
async fn vault_unlock(passphrase:String,app:State<'_,App>)->Reply<()>{let path=app.data_dir.join("keys.age");let (v,r)=tauri::async_runtime::spawn_blocking(move||{let mut v=Vault::default();let r=v.unlock(&path,zeroize::Zeroizing::new(passphrase));(v,r)}).await.map_err(err)?;r.map_err(err)?;*app.vault.lock().await=v;Ok(())}
#[tauri::command]
async fn vault_lock(app:State<'_,App>)->Reply<()>{app.vault.lock().await.lock();Ok(())}

fn main(){
    let ephemeral=std::env::args().any(|a|a=="--ephemeral");
    tauri::Builder::default().setup(move|app|{
        let home=app.path().home_dir()?;
        let config_dir=home.join(".config/linkscope");let data_dir=home.join(".local/share/linkscope");
        let config=if ephemeral{Config::default()}else{
            std::fs::create_dir_all(&config_dir)?;std::fs::create_dir_all(&data_dir)?;
            #[cfg(unix)]{use std::os::unix::fs::PermissionsExt;for p in [&config_dir,&data_dir]{std::fs::set_permissions(p,std::fs::Permissions::from_mode(0o700))?;}}
            let p=config_dir.join("config.json");if p.exists(){serde_json::from_slice(&std::fs::read(p)?)?}else{let c=Config::default();std::fs::write(p,serde_json::to_vec_pretty(&c)?)?;c}
        };
        if !(256..=4096).contains(&config.memory_mb){return Err("memory budget must be 256–4096 MiB".into());}
        let engine=tauri::async_runtime::block_on(EngineHandle::start(if ephemeral{None}else{Some(data_dir.clone())},config.memory_mb*1024*1024/3))?;
        let plugins_dir=config_dir.join("plugins");
        let transforms=transform::load_plugins(if ephemeral{None}else{Some(plugins_dir.as_path())})?;
        let network=Arc::new(Network::new(config.network.clone(),engine.clone())?);
        Audit::new(if ephemeral{None}else{Some(data_dir.join("logs"))}).record("session_start",0)?;
        app.manage(App{engine,network:Mutex::new(network),config:Mutex::new(config),transforms,job:Mutex::new(None),vault:Mutex::new(Vault::default()),config_dir,data_dir,ephemeral});Ok(())
    }).invoke_handler(tauri::generate_handler![info,add_node,apply_batch,positions,page,node,search,stats,run_transforms,poll_job,cancel_job,configure,export_graph,vault_save,vault_unlock,vault_lock]).run(tauri::generate_context!()).expect("LinkScope startup failed");
}
