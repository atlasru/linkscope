#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod dialog;
use anyhow::{bail, Context, Result};
use linkscope_engine::{import, model::*, storage, Graph};
use linkscope_transforms::Service;
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use tauri::{Manager, State};
use tokio_util::sync::CancellationToken;

struct Session {
    graph: Graph,
    epoch: u64,
    path: Option<PathBuf>,
    recovery: Option<String>,
    autosaved: Option<u64>,
}
struct Job {
    cancel: CancellationToken,
    result: Value,
    sequence: u64,
}
static JOB_SEQUENCE: AtomicU64 = AtomicU64::new(0);
struct App {
    session: Arc<Mutex<Session>>,
    service: Arc<Service>,
    jobs: Arc<Mutex<HashMap<String, Job>>>,
    directory: PathBuf,
    workspace: Arc<Mutex<Value>>,
    closing: Arc<AtomicBool>,
    _lock: File,
    io: Arc<Mutex<()>>,
    #[cfg(debug_assertions)]
    smoke_path: Option<PathBuf>,
    #[cfg(debug_assertions)]
    smoke_restore: bool,
}
type Reply<T> = std::result::Result<T, String>;
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Reply<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(error)?
        .map_err(error)
}
fn snapshot(s: &Session, workspace: Value) -> Value {
    json!({"project":s.graph.snapshot(),"revision":session_revision(s),"canUndo":s.graph.can_undo(),"canRedo":s.graph.can_redo(),"path":s.path,"recovery":s.recovery,"workspace":workspace})
}
fn session_revision(s: &Session) -> u64 {
    s.epoch + s.graph.revision()
}
fn check_revision(s: &Session, revision: u64) -> Result<()> {
    if session_revision(s) != revision {
        bail!("Project changed. Reload the editor before retrying this edit.")
    }
    Ok(())
}
fn ensure_editable(closing: &AtomicBool) -> Result<()> {
    if closing.load(Ordering::SeqCst) {
        bail!("The window is saving before closing")
    }
    Ok(())
}
fn archive(s: &Session, dir: &std::path::Path) -> Result<()> {
    let p = s.graph.snapshot();
    if p.entities.is_empty()
        && p.relationships.is_empty()
        && p.collections.is_empty()
        && p.metadata.is_empty()
        && p.title == "Untitled"
    {
        return Ok(());
    }
    // Preserve unsaved work when switching projects. These are user-recoverable portable files.
    let folder = dir.join("recoveries");
    std::fs::create_dir_all(&folder)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    storage::save(&folder.join(format!("{stamp}.linkscope")), &p)
}
fn install_project(
    s: &mut Session,
    graph: Graph,
    path: Option<PathBuf>,
    recovery: Option<String>,
    directory: &Path,
) -> Result<()> {
    let epoch = session_revision(s)
        .checked_add(1)
        .context("Session revision exhausted")?;
    archive(s, directory)?;
    // Complete all fallible writes before replacing authoritative state.
    storage::save(&directory.join("recovery.linkscope"), &graph.snapshot())?;
    *s = Session {
        graph,
        epoch,
        path,
        recovery,
        autosaved: Some(epoch),
    };
    Ok(())
}
fn recovery_family(path: &Path) -> [PathBuf; 3] {
    let sidecar = |suffix: &str| {
        let mut value = path.as_os_str().to_owned();
        value.push(suffix);
        PathBuf::from(value)
    };
    [path.to_path_buf(), sidecar(".pending"), sidecar(".bak")]
}
fn quarantine_recovery(path: &Path) -> Result<PathBuf> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let folder = path
        .parent()
        .context("Recovery has no directory")?
        .join("recoveries")
        .join(format!("corrupt-{stamp}"));
    std::fs::create_dir_all(&folder)?;
    for original in recovery_family(path) {
        if original.exists() {
            let destination =
                folder.join(original.file_name().context("Recovery filename missing")?);
            std::fs::rename(&original, &destination).with_context(|| {
                format!(
                    "Preserve {} in {}; earlier files may already be there",
                    original.display(),
                    folder.display()
                )
            })?;
        }
    }
    Ok(folder)
}
fn open_recovery(path: &Path) -> Result<(Graph, Option<String>)> {
    if !recovery_family(path).iter().any(|p| p.exists()) {
        return Ok((Graph::empty(), None));
    }
    match storage::load(path) {
        Ok(loaded) => {
            let graph = Graph::new(loaded.project)?;
            if let Some(source) = loaded.recovered_from {
                let preserved = quarantine_recovery(path)?;
                storage::save(path, &graph.snapshot())?;
                Ok((
                    graph,
                    Some(format!(
                        "Recovered from {source}. Previous files preserved at {}",
                        preserved.display()
                    )),
                ))
            } else {
                Ok((graph, None))
            }
        }
        Err(error) => {
            let preserved = quarantine_recovery(path)?;
            let graph = Graph::empty();
            storage::save(path, &graph.snapshot())?;
            Ok((
                graph,
                Some(format!(
                    "Autosave could not be opened: {error}. Original files preserved at {}",
                    preserved.display()
                )),
            ))
        }
    }
}
fn persist_recovery(
    session: &Mutex<Session>,
    io: &Mutex<()>,
    path: &Path,
    force: bool,
) -> Result<()> {
    let _io = io.lock().map_err(error).map_err(anyhow::Error::msg)?;
    let (project, revision) = {
        let s = session.lock().map_err(error).map_err(anyhow::Error::msg)?;
        let revision = session_revision(&s);
        if !force && s.autosaved == Some(revision) {
            return Ok(());
        }
        (s.graph.snapshot(), revision)
    };
    // No graph lock during serialization, flush, backup or atomic replacement.
    storage::save(path, &project)?;
    let mut s = session.lock().map_err(error).map_err(anyhow::Error::msg)?;
    if session_revision(&s) == revision {
        s.autosaved = Some(revision)
    }
    Ok(())
}
fn trim_jobs(jobs: &mut HashMap<String, Job>) {
    while jobs
        .values()
        .filter(|job| job.result["status"] != "running")
        .count()
        > 32
    {
        let oldest = jobs
            .iter()
            .filter(|(_, job)| job.result["status"] != "running")
            .min_by_key(|(_, job)| job.sequence)
            .map(|(id, _)| id.clone());
        if let Some(id) = oldest {
            jobs.remove(&id);
        } else {
            break;
        }
    }
}
#[tauri::command]
async fn boot(app: State<'_, App>) -> Reply<Value> {
    let s = app.session.clone();
    let workspace = app.workspace.lock().map_err(error)?.clone();
    blocking(move || {
        let session = s.lock().map_err(error).map_err(anyhow::Error::msg)?;
        Ok(snapshot(&session, workspace))
    })
    .await
}
#[tauri::command]
async fn apply_patch(patch: Patch, revision: u64, app: State<'_, App>) -> Reply<Delta> {
    let s = app.session.clone();
    let closing = app.closing.clone();
    blocking(move || {
        let mut s = s.lock().map_err(error).map_err(anyhow::Error::msg)?;
        ensure_editable(&closing)?;
        check_revision(&s, revision)?;
        let mut delta = s.graph.apply(patch)?;
        delta.revision += s.epoch;
        Ok(delta)
    })
    .await
}
#[tauri::command]
async fn undo(revision: u64, app: State<'_, App>) -> Reply<Delta> {
    let s = app.session.clone();
    let closing = app.closing.clone();
    blocking(move || {
        let mut s = s.lock().map_err(error).map_err(anyhow::Error::msg)?;
        ensure_editable(&closing)?;
        check_revision(&s, revision)?;
        let mut delta = s.graph.undo()?;
        delta.revision += s.epoch;
        Ok(delta)
    })
    .await
}
#[tauri::command]
async fn redo(revision: u64, app: State<'_, App>) -> Reply<Delta> {
    let s = app.session.clone();
    let closing = app.closing.clone();
    blocking(move || {
        let mut s = s.lock().map_err(error).map_err(anyhow::Error::msg)?;
        ensure_editable(&closing)?;
        check_revision(&s, revision)?;
        let mut delta = s.graph.redo()?;
        delta.revision += s.epoch;
        Ok(delta)
    })
    .await
}
fn cancel_jobs(app: &App) {
    if let Ok(jobs) = app.jobs.lock() {
        for job in jobs.values() {
            job.cancel.cancel();
        }
    }
}
#[tauri::command]
async fn new_project(app: State<'_, App>) -> Reply<Value> {
    cancel_jobs(&app);
    let s = app.session.clone();
    let dir = app.directory.clone();
    let w = app.workspace.lock().map_err(error)?.clone();
    let io = app.io.clone();
    let closing = app.closing.clone();
    blocking(move || {
        let _io = io.lock().map_err(error).map_err(anyhow::Error::msg)?;
        let mut s = s.lock().map_err(error).map_err(anyhow::Error::msg)?;
        ensure_editable(&closing)?;
        install_project(&mut s, Graph::empty(), None, None, &dir)?;
        Ok(snapshot(&s, w))
    })
    .await
}
#[tauri::command]
async fn open_project(app: State<'_, App>) -> Reply<Option<Value>> {
    let chosen = blocking(|| dialog::choose(false, false)).await?;
    let Some(path) = chosen else { return Ok(None) };
    cancel_jobs(&app);
    let s = app.session.clone();
    let dir = app.directory.clone();
    let w = app.workspace.lock().map_err(error)?.clone();
    let io = app.io.clone();
    let closing = app.closing.clone();
    blocking(move || {
        let loaded = storage::load(&path)?;
        let graph = Graph::new(loaded.project)?;
        let _io = io.lock().map_err(error).map_err(anyhow::Error::msg)?;
        let mut s = s.lock().map_err(error).map_err(anyhow::Error::msg)?;
        ensure_editable(&closing)?;
        install_project(&mut s, graph, Some(path), loaded.recovered_from, &dir)?;
        Ok(Some(snapshot(&s, w)))
    })
    .await
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Saved {
    path: String,
    revision: u64,
}
#[tauri::command]
async fn save_project(save_as: bool, app: State<'_, App>) -> Reply<Option<Saved>> {
    let (current, epoch) = {
        let s = app.session.lock().map_err(error)?;
        (s.path.clone(), s.epoch)
    };
    let path = if save_as || current.is_none() {
        blocking(|| dialog::choose(true, false)).await?
    } else {
        current
    };
    let Some(path) = path else { return Ok(None) };
    let s = app.session.clone();
    let io = app.io.clone();
    let closing = app.closing.clone();
    blocking(move || {
        let _io = io.lock().map_err(error).map_err(anyhow::Error::msg)?;
        let (project, revision) = {
            let s = s.lock().map_err(error).map_err(anyhow::Error::msg)?;
            ensure_editable(&closing)?;
            if s.epoch != epoch {
                bail!("Project changed while choosing a save location")
            }
            (s.graph.snapshot(), session_revision(&s))
        };
        storage::save(&path, &project)?;
        let mut s = s.lock().map_err(error).map_err(anyhow::Error::msg)?;
        s.path = Some(path.clone());
        s.recovery = None;
        Ok(Some(Saved {
            path: path.to_string_lossy().into(),
            revision,
        }))
    })
    .await
}
#[tauri::command]
async fn rename_project(title: String, app: State<'_, App>) -> Reply<u64> {
    let s = app.session.clone();
    let closing = app.closing.clone();
    blocking(move || {
        let mut s = s.lock().map_err(error).map_err(anyhow::Error::msg)?;
        ensure_editable(&closing)?;
        s.graph.set_title(title)?;
        Ok(session_revision(&s))
    })
    .await
}
#[tauri::command]
async fn preview_import(
    text: String,
    format: String,
    name: String,
) -> Reply<import::ImportPreview> {
    blocking(move || import::preview(&text, &format, &name)).await
}
#[tauri::command]
async fn pick_import() -> Reply<Option<Value>> {
    blocking(|| {
        let Some(path) = dialog::choose(false, true)? else {
            return Ok(None);
        };
        if std::fs::metadata(&path)?.len() > 32 * 1024 * 1024 {
            bail!("Import is larger than 32 MiB. Split the dataset before importing.")
        }
        let format = path.extension().and_then(|s| s.to_str()).unwrap_or("txt");
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let preview = import::preview(
            &std::fs::read_to_string(&path).context("Import must be UTF-8 text")?,
            format,
            &name,
        )?;
        Ok(Some(
            json!({"patch":preview.patch,"warnings":preview.warnings,"name":name}),
        ))
    })
    .await
}
#[tauri::command]
async fn save_workspace(workspace: Value, app: State<'_, App>) -> Reply<()> {
    if serde_json::to_vec(&workspace).map_err(error)?.len() > 8192 {
        return Err("Workspace settings are too large".into());
    }
    *app.workspace.lock().map_err(error)? = workspace;
    let path = app.directory.join("workspace.json");
    let current = app.workspace.clone();
    let io = app.io.clone();
    blocking(move || {
        use std::io::Write;
        let _io = io.lock().map_err(error).map_err(anyhow::Error::msg)?;
        let workspace = current
            .lock()
            .map_err(error)
            .map_err(anyhow::Error::msg)?
            .clone();
        let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
        serde_json::to_writer(&mut file, &workspace)?;
        file.flush()?;
        file.as_file().sync_all()?;
        file.persist(path).map_err(|e| e.error)?;
        Ok(())
    })
    .await
}
#[tauri::command]
fn manifests() -> Vec<linkscope_transforms::Manifest> {
    Service::manifests()
}
#[tauri::command]
async fn start_transform(kind: String, id: String, app: State<'_, App>) -> Reply<String> {
    let entity = app
        .session
        .lock()
        .map_err(error)?
        .graph
        .entity(&id)
        .cloned()
        .ok_or("Entity no longer exists")?;
    let mut jobs = app.jobs.lock().map_err(error)?;
    if jobs
        .values()
        .filter(|j| j.result["status"] == "running")
        .count()
        >= 4
    {
        return Err("Four research jobs are already queued. Wait or cancel one.".into());
    }
    trim_jobs(&mut jobs);
    let sequence = JOB_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let job_id = format!("job-{sequence}");
    let cancel = CancellationToken::new();
    jobs.insert(
        job_id.clone(),
        Job {
            cancel: cancel.clone(),
            result: json!({"id":job_id,"status":"running"}),
            sequence,
        },
    );
    drop(jobs);
    let service = app.service.clone();
    let jobs = app.jobs.clone();
    let output_id = job_id.clone();
    tauri::async_runtime::spawn(async move {
        let result = service.run(&kind, &entity, cancel.clone()).await;
        let value = if cancel.is_cancelled() {
            json!({"id":output_id,"status":"cancelled"})
        } else {
            match result {
                Ok(output) => json!({"id":output_id,"status":"complete","output":output}),
                Err(e) => json!({"id":output_id,"status":"failed","error":e.to_string()}),
            }
        };
        if let Ok(mut jobs) = jobs.lock() {
            if let Some(job) = jobs.get_mut(&output_id) {
                job.result = value;
            }
            trim_jobs(&mut jobs);
        }
    });
    Ok(job_id)
}
#[tauri::command]
fn poll_transform(id: String, app: State<'_, App>) -> Reply<Value> {
    app.jobs
        .lock()
        .map_err(error)?
        .get(&id)
        .map(|j| j.result.clone())
        .ok_or("Job is no longer available".into())
}
#[tauri::command]
fn cancel_transform(id: String, app: State<'_, App>) -> Reply<()> {
    if let Some(j) = app.jobs.lock().map_err(error)?.get(&id) {
        j.cancel.cancel();
    }
    Ok(())
}
#[cfg(debug_assertions)]
#[tauri::command]
fn smoke_result(result: Value, app: State<'_, App>) -> Reply<()> {
    let Some(path) = &app.smoke_path else {
        return Err("Desktop smoke runner is disabled".into());
    };
    std::fs::write(path, serde_json::to_vec_pretty(&result).map_err(error)?).map_err(error)
}
#[cfg(debug_assertions)]
#[tauri::command]
fn smoke_ready(window: tauri::WebviewWindow, app: State<'_, App>) -> Reply<()> {
    if app.smoke_path.is_some() {
        window
            .eval(if app.smoke_restore {
                "window.dispatchEvent(new CustomEvent('linkscope-smoke',{detail:{restore:true}}))"
            } else {
                "window.dispatchEvent(new Event('linkscope-smoke'))"
            })
            .map_err(error)?;
    }
    Ok(())
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let data_override = args
        .windows(2)
        .find(|w| w[0] == "--data-dir")
        .map(|w| PathBuf::from(&w[1]));
    #[cfg(debug_assertions)]
    let smoke_path = args
        .windows(2)
        .find(|w| w[0] == "--smoke-test")
        .map(|w| PathBuf::from(&w[1]));
    #[cfg(debug_assertions)]
    let smoke_restore = args.iter().any(|arg| arg == "--smoke-restore");
    let builder = tauri::Builder::default()
        .setup(move |app| {
            let directory = data_override.unwrap_or(app.path().app_data_dir()?);
            std::fs::create_dir_all(&directory)?;
            let lock = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(directory.join("session.lock"))?;
            fs2::FileExt::try_lock_exclusive(&lock)
                .context("Another LinkScope window is using this workspace")?;
            let recovery = directory.join("recovery.linkscope");
            let (graph, message) = open_recovery(&recovery)?;
            let workspace = std::fs::read(directory.join("workspace.json"))
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or(json!({}));
            let session = Arc::new(Mutex::new(Session {
                graph,
                epoch: 0,
                path: None,
                recovery: message,
                autosaved: Some(0),
            }));
            let jobs = Arc::new(Mutex::new(HashMap::new()));
            let io = Arc::new(Mutex::new(()));
            app.manage(App {
                session: session.clone(),
                service: Arc::new(Service::new()?),
                jobs,
                directory: directory.clone(),
                workspace: Arc::new(Mutex::new(workspace)),
                closing: Arc::new(AtomicBool::new(false)),
                io: io.clone(),
                _lock: lock,
                #[cfg(debug_assertions)]
                smoke_path,
                #[cfg(debug_assertions)]
                smoke_restore,
            });
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    let s = session.clone();
                    let path = directory.join("recovery.linkscope");
                    let io = io.clone();
                    let result = tauri::async_runtime::spawn_blocking(move || {
                        persist_recovery(&s, &io, &path, false)
                    })
                    .await;
                    let problem = match result {
                        Ok(Ok(())) => None,
                        Ok(Err(e)) => Some(e.to_string()),
                        Err(e) => Some(e.to_string()),
                    };
                    if let Some(problem) = problem {
                        use tauri::Emitter;
                        let _ = handle.emit("autosave-error", problem);
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let state = window.state::<App>();
                if state.closing.swap(true, Ordering::SeqCst) {
                    return;
                }
                cancel_jobs(&state);
                let s = state.session.clone();
                let path = state.directory.join("recovery.linkscope");
                let closing = state.closing.clone();
                let handle = window.app_handle().clone();
                let io = state.io.clone();
                tauri::async_runtime::spawn(async move {
                    let result = blocking(move || persist_recovery(&s, &io, &path, true)).await;
                    match result {
                        Ok(()) => handle.exit(0),
                        Err(e) => {
                            closing.store(false, Ordering::SeqCst);
                            use tauri::Emitter;
                            let _ = handle.emit(
                                "autosave-error",
                                format!("Could not save before closing: {e}"),
                            );
                        }
                    }
                });
            }
        });
    #[cfg(debug_assertions)]
    let builder = builder.invoke_handler(tauri::generate_handler![
        boot,
        apply_patch,
        undo,
        redo,
        new_project,
        open_project,
        save_project,
        rename_project,
        preview_import,
        pick_import,
        save_workspace,
        manifests,
        start_transform,
        poll_transform,
        cancel_transform,
        smoke_result,
        smoke_ready
    ]);
    #[cfg(not(debug_assertions))]
    let builder = builder.invoke_handler(tauri::generate_handler![
        boot,
        apply_patch,
        undo,
        redo,
        new_project,
        open_project,
        save_project,
        rename_project,
        preview_import,
        pick_import,
        save_workspace,
        manifests,
        start_transform,
        poll_transform,
        cancel_transform
    ]);
    builder
        .run(tauri::generate_context!())
        .expect("LinkScope could not start");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        Session {
            graph: Graph::empty(),
            epoch: 0,
            path: None,
            recovery: None,
            autosaved: Some(0),
        }
    }

    #[test]
    fn project_switch_invalidates_old_revisions_even_for_empty_graphs() {
        let directory = tempfile::tempdir().unwrap();
        let mut session = session();
        let original = session_revision(&session);
        install_project(&mut session, Graph::empty(), None, None, directory.path()).unwrap();
        assert!(check_revision(&session, original).is_err());
        assert_eq!(session_revision(&session), original + 1);
        let delta = session
            .graph
            .apply(Patch {
                entities: vec![Entity {
                    id: "new".into(),
                    ..Entity::default()
                }],
                ..Patch::default()
            })
            .unwrap();
        assert_eq!(session.epoch + delta.revision, session_revision(&session));
    }

    #[test]
    fn failed_recovery_save_does_not_replace_working_project() {
        let directory = tempfile::tempdir().unwrap();
        let mut session = session();
        session
            .graph
            .apply(Patch {
                entities: vec![Entity {
                    id: "retained".into(),
                    ..Entity::default()
                }],
                ..Patch::default()
            })
            .unwrap();
        std::fs::write(directory.path().join("recovery.linkscope"), "corrupt").unwrap();
        assert!(
            install_project(&mut session, Graph::empty(), None, None, directory.path()).is_err()
        );
        assert!(session.graph.entity("retained").is_some());
        assert_eq!(session.epoch, 0);
        assert_eq!(
            std::fs::read(directory.path().join("recovery.linkscope")).unwrap(),
            b"corrupt"
        );
    }

    #[test]
    fn corrupt_autosave_family_is_retained_before_fresh_recovery_is_created() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("recovery.linkscope");
        for original in recovery_family(&path) {
            std::fs::write(original, b"original corrupt bytes").unwrap();
        }
        let (graph, message) = open_recovery(&path).unwrap();
        assert_eq!(graph.entity_count(), 0);
        assert!(message.unwrap().contains("preserved at"));
        assert!(storage::load(&path).is_ok());
        let folders = std::fs::read_dir(directory.path().join("recoveries"))
            .unwrap()
            .collect::<std::io::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(folders.len(), 1);
        let preserved = std::fs::read_dir(folders[0].path())
            .unwrap()
            .collect::<std::io::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(preserved.len(), 3);
        for original in preserved {
            assert_eq!(
                std::fs::read(original.path()).unwrap(),
                b"original corrupt bytes"
            );
        }
    }

    #[test]
    fn valid_pending_autosave_is_recovered_and_preserved_with_bad_original() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("recovery.linkscope");
        let project = Project {
            title: "Recovered title".into(),
            ..Project::default()
        };
        storage::save(&recovery_family(&path)[1], &project).unwrap();
        std::fs::write(&path, b"bad original").unwrap();
        let (graph, message) = open_recovery(&path).unwrap();
        assert_eq!(graph.snapshot().title, "Recovered title");
        assert!(message.unwrap().contains("Recovered from"));
        assert_eq!(
            storage::load(&path).unwrap().project.title,
            "Recovered title"
        );
        assert!(!recovery_family(&path)[1].exists());
    }

    #[test]
    fn completed_previews_survive_new_jobs_with_a_bounded_retention_window() {
        let mut jobs = HashMap::new();
        for sequence in 0..42 {
            jobs.insert(
                sequence.to_string(),
                Job {
                    cancel: CancellationToken::new(),
                    sequence,
                    result: json!({"status":if sequence>=40{"running"}else{"complete"}}),
                },
            );
        }
        trim_jobs(&mut jobs);
        assert_eq!(jobs.len(), 34);
        assert!(jobs.contains_key("40") && jobs.contains_key("41"));
        assert!(!jobs.contains_key("0"));
        assert!(jobs.contains_key("8"));
    }

    #[test]
    fn autosave_marks_only_the_saved_revision() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("recovery.linkscope");
        let mut session = session();
        session
            .graph
            .apply(Patch {
                entities: vec![Entity {
                    id: "saved".into(),
                    ..Entity::default()
                }],
                ..Patch::default()
            })
            .unwrap();
        let session = Mutex::new(session);
        let io = Mutex::new(());
        persist_recovery(&session, &io, &path, false).unwrap();
        let guard = session.lock().unwrap();
        assert_eq!(guard.autosaved, Some(session_revision(&guard)));
        drop(guard);
        assert_eq!(storage::load(&path).unwrap().project.entities.len(), 1);
        assert!(ensure_editable(&AtomicBool::new(true)).is_err());
    }
}
