import './style.css';
import {GraphCanvas} from './canvas';
import {api,invoke} from './bridge';
import {EditorModel,mergeEntities,rekeyImport,splitEntity} from './model';
import {EditorQueue,requireEntity,requireRelationship} from './queue';
import {entity,relationship,normalizeWorkspace} from './types';
import type {Delta,Entity,ImportPreview,Json,Manifest,Patch,Snapshot,Workspace} from './types';

const $=<T extends HTMLElement=HTMLElement>(s:string)=>{const el=document.querySelector<T>(s);if(!el)throw Error(`Missing element ${s}`);return el};
const esc=(v:unknown)=>String(v??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]!));
const paths:Record<string,string>={new:'M8 3H4v18h16V7l-4-4H8m7 0v5h5M8 14h8m-4-4v8',open:'M3 7h7l2 2h9l-3 11H3V7m0 2V4h6l3 3',save:'M4 3h14l3 3v15H3V3h1m3 0v6h10V3M7 21v-8h10v8',palette:'m9 6-6 6 6 6m4 0h8',left:'M3 4h18v16H3V4m5 0v16',right:'M3 4h18v16H3V4m13 0v16',create:'M12 4v16M4 12h16',select:'m5 3 14 10-7 1-3 7L5 3',link:'m10 14 4-4m-6 6-2 2a4 4 0 0 1-6-6l4-4m12 0 2-2a4 4 0 0 1 6 6l-4 4',layout:'M5 5h4v4H5V5m10 10h4v4h-4v-4M5 15h4v4H5v-4m10-14h4v4h-4V1M9 7h6m-8 2v6m10-10v10M9 17h6',fit:'M9 3H3v6m12-6h6v6M3 15v6h6m6 0h6v-6M8 8l8 8m0-8-8 8',undo:'M9 5 3 11l6 5m-6-5h11a6 6 0 0 1 6 6v3',redo:'m15 5 6 6-6 5m6-5H10a6 6 0 0 0-6 6v3','zoom-in':'M12 4v16M4 12h16','zoom-out':'M4 12h16'};
for(const el of document.querySelectorAll<HTMLButtonElement>('button.icon')){const key=el.dataset.action??({'toggle-left':'left','toggle-right':'right','select-tool':'select','link-tool':'link'}[el.id]);if(key&&paths[key])el.innerHTML=`<svg viewBox="0 0 24 24" aria-hidden="true"><path d="${paths[key]}"/></svg>`}
const model=new EditorModel();let ready=false;let dirty=false;let busy=false;let manifests:Manifest[]=[];let jobId:string|undefined;let noticeTimer:ReturnType<typeof setTimeout>|undefined;let workspaceTimer:ReturnType<typeof setTimeout>|undefined;let filterTimer:ReturnType<typeof setTimeout>|undefined;
let workspace:Workspace={left:true,right:true,leftWidth:244,rightWidth:280};
const title=$<HTMLInputElement>('#project-title');const search=$<HTMLInputElement>('#search');const kindFilter=$<HTMLSelectElement>('#type-filter');
const dialog=$<HTMLDialogElement>('#editor-dialog');const form=$<HTMLFormElement>('#dialog-form');const palette=$<HTMLDialogElement>('#palette');
const editorQueue=new EditorQueue(model);const typeCounts=new Map<string,number>();
function countType(kind:string,change:number){const next=(typeCounts.get(kind)??0)+change;if(next<=0)typeCounts.delete(kind);else typeCounts.set(kind,next)}
function notice(message:string,error=false,sticky=false){clearTimeout(noticeTimer);const el=$('#notice');el.hidden=false;el.classList.toggle('error',error);el.querySelector('span')!.textContent=message;if(!sticky)noticeTimer=setTimeout(()=>el.hidden=true,error?14000:5000)}
function safe(work:()=>Promise<unknown>|unknown){const failed=(e:unknown)=>{notice(e instanceof Error?e.message:String(e),true);return undefined};try{return Promise.resolve(work()).catch(failed)}catch(e){return Promise.resolve(failed(e))}}
function serial<T>(work:()=>Promise<T>):Promise<T>{return editorQueue.run(work)}
function projectSerial<T>(work:(current:EditorModel)=>Promise<T>){return editorQueue.edit(work)}
function markDirty(value=true){dirty=value;$('#dirty').hidden=!value;document.title=`${dirty?'• ':''}${model.project?.title??'Untitled'} — LinkScope`}
function acceptDelta(delta:Delta){let listChanged=false;for(const id of delta.patch.removeEntities??[]){const old=model.entities.get(id);if(old)countType(old.kind,-1);listChanged=true}for(const n of delta.patch.entities??[]){const old=model.entities.get(n.id);if(!old||old.kind!==n.kind){if(old)countType(old.kind,-1);countType(n.kind,1)}if(!old||old.label!==n.label||old.kind!==n.kind||(search.value&&JSON.stringify(old.properties)!==JSON.stringify(n.properties)))listChanged=true}model.delta(delta);canvas.update(delta);canvas.setSelection(model.selection,model.edgeId);markDirty();refresh(listChanged);}
async function apply(patch:Patch|((current:EditorModel)=>Patch),origin=model.project){return editorQueue.edit(async current=>{if(!ready)throw Error('Project is not ready');const delta=await api.apply(typeof patch==='function'?patch(current):patch,current.revision);acceptDelta(delta);return delta},origin)}
const canvas=new GraphCanvas($('#canvas-host'),{
  select:(ids,edgeId)=>{model.selection=new Set(ids);model.edgeId=edgeId;refreshSelection()},
  move:positions=>void safe(async()=>{const origin=model.project;try{await apply(current=>({entities:positions.map(p=>({...requireEntity(current,p.id),x:p.x,y:p.y}))}),origin)}catch(error){if(model.project===origin)canvas.update({patch:{entities:positions.map(p=>model.entities.get(p.id)).filter((n):n is Entity=>!!n)},revision:model.revision,canUndo:model.canUndo,canRedo:model.canRedo});throw error}}),
  create:(x,y)=>createEntity(x,y),
  connect:(source,target)=>void safe(async()=>{const edge=relationship(source,target);await apply(current=>{requireEntity(current,source);requireEntity(current,target);return{relationships:[edge]}});model.selection.clear();model.edgeId=edge.id;refreshSelection()}),
  context:(x,y,id,edgeId)=>contextMenu(x,y,id,edgeId),
  camera:()=>persistWorkspace()
});
function refreshSelection(){canvas.setSelection(model.selection,model.edgeId);for(const row of document.querySelectorAll<HTMLButtonElement>('#entity-list button'))row.setAttribute('aria-selected',String(model.selection.has(row.dataset.id!)));renderInspector()}
function refresh(listChanged=true){
  $('#undo').toggleAttribute('disabled',!model.canUndo);$('#redo').toggleAttribute('disabled',!model.canRedo);$('#empty').hidden=model.entities.size>0;
  if(listChanged){const selectedKind=kindFilter.value;const kinds=[...typeCounts.keys()].sort();kindFilter.innerHTML='<option value="">All types</option>'+kinds.map(k=>`<option value="${esc(k)}">${esc(k)}</option>`).join('');kindFilter.value=selectedKind;renderList()}
  renderCollections();renderInspector();
}
function renderList(){
  const query=search.value.toLowerCase().trim(),kind=kindFilter.value;const list=$('#entity-list');const fragment=document.createDocumentFragment();let count=0;
  for(const n of model.entities.values()){
    if(kind&&n.kind!==kind)continue;if(query&&!`${n.label} ${n.kind} ${JSON.stringify(n.properties)}`.toLowerCase().includes(query))continue;
    if(count++>=200)break;
    const b=document.createElement('button');b.role='option';b.dataset.id=n.id;b.setAttribute('aria-selected',String(model.selection.has(n.id)));b.title=`${n.label} · ${n.kind}`;
    b.innerHTML=`<i class="node-dot"></i><span class="entity-name">${esc(n.label)}</span><small>${esc(n.kind)}</small>`;
    b.onclick=e=>{if(e.ctrlKey||e.metaKey||e.shiftKey){if(model.selection.has(n.id))model.selection.delete(n.id);else model.selection.add(n.id)}else model.selection=new Set([n.id]);model.edgeId=undefined;canvas.centerOn(n.id);refreshSelection()};fragment.append(b);
  }
  list.replaceChildren(fragment);$('#search-more').hidden=count<=200;canvas.setFilter(search.value,kindFilter.value);
}
function renderCollections(){const list=$('#collection-list');list.replaceChildren();$('#collections-section').hidden=model.collections.size===0;for(const c of model.collections.values()){const row=document.createElement('div');row.className='collection';row.innerHTML=`<button title="Select collection">${esc(c.label)}</button><button class="icon" title="Ungroup collection" aria-label="Ungroup ${esc(c.label)}">×</button>`;row.children[0].addEventListener('click',()=>{model.selection=new Set(c.members);model.edgeId=undefined;if(c.members[0])canvas.centerOn(c.members[0]);refreshSelection()});row.children[1].addEventListener('click',()=>void safe(()=>apply({removeCollections:[c.id]})));list.append(row)}}
const field=(label:string,name:string,value:string,extra='')=>`<label class="field">${label}<input name="${name}" value="${esc(value)}" ${extra}></label>`;
function properties(text:string):Record<string,Json>{const value:unknown=JSON.parse(text);if(!value||typeof value!=='object'||Array.isArray(value))throw Error('Properties must be a JSON object');return value as Record<string,Json>}
function renderInspector(){
  const origin=model.project;
  const host=$('#inspector-content');const selected=model.selected();const edge=model.edgeId?model.relationships.get(model.edgeId):undefined;
  if(selected.length>1){host.innerHTML=`<div class="subtle">${selected.length} selected</div><div class="row wrap" style="margin-top:12px"><button id="group" class="secondary">Group</button><button id="merge" class="secondary">Merge…</button><button id="remove-selection" class="danger">Delete</button></div>`;$('#group').onclick=groupSelection;$('#merge').onclick=mergeSelection;$('#remove-selection').onclick=()=>void safe(deleteSelection);return}
  const item=edge??selected[0];if(!item){host.replaceChildren();return}
  const isEdge=!!edge;host.innerHTML=`<form id="properties-form">${field('Label','label',item.label,isEdge?'maxlength="512"':'required maxlength="4096"')}${field('Type','kind',item.kind,'required list="entity-types" maxlength="128"')}${isEdge?`<div class="subtle">${esc(model.entities.get(edge!.source)?.label)} → ${esc(model.entities.get(edge!.target)?.label)}</div><label class="row"><input type="checkbox" name="directed" ${edge!.directed?'checked':''}> Directed</label>`:''}<label class="field">Properties<textarea name="properties" class="mono" rows="6" spellcheck="false">${esc(JSON.stringify(item.properties,null,2))}</textarea></label><label class="field">Notes<textarea name="notes" rows="4">${esc(item.notes)}</textarea></label><div class="row"><button type="submit" class="primary">Apply</button>${!isEdge?'<button type="button" id="duplicate" title="Create an independent copy">Duplicate</button>':''}<button type="button" id="delete-item" class="danger">Delete</button></div>${item.sources.length?`<details><summary>Sources</summary>${item.sources.map(s=>`<p>${esc(s.label)}</p>${s.uri?`<pre>${esc(s.uri)}</pre>`:''}${s.record!=null?`<pre>${esc(JSON.stringify(s.record,null,2))}</pre>`:''}`).join('')}</details>`:''}</form>${!isEdge?'<div id="research"></div>':''}`;
  $<HTMLFormElement>('#properties-form').onsubmit=e=>{e.preventDefault();const f=new FormData(e.currentTarget as HTMLFormElement);void safe(async()=>{const entered={label:String(f.get('label')).trim(),kind:String(f.get('kind')).trim(),notes:String(f.get('notes')),properties:properties(String(f.get('properties')))};const changed:Partial<Pick<Entity,'label'|'kind'|'notes'|'properties'>>={};if(entered.label!==item.label)changed.label=entered.label;if(entered.kind!==item.kind)changed.kind=entered.kind;if(entered.notes!==item.notes)changed.notes=entered.notes;if(JSON.stringify(entered.properties)!==JSON.stringify(item.properties))changed.properties=entered.properties;if(isEdge)await apply(current=>({relationships:[{...requireRelationship(current,item.id),...changed,...(f.has('directed')!==edge!.directed?{directed:f.has('directed')}:{})}]}),origin);else await apply(current=>({entities:[{...requireEntity(current,item.id),...changed}]}),origin)})};
  $('#delete-item').onclick=()=>void safe(deleteSelection);
  if(!isEdge){$('#duplicate').onclick=()=>void safe(async()=>{const id=crypto.randomUUID();await apply(current=>{const n=requireEntity(current,item.id);return{entities:[{...n,id,label:`${n.label} copy`,x:n.x+45,y:n.y+45}]}},origin);if(model.project===origin)selectOnly(id)});const split=document.createElement('button');split.type='button';split.textContent='Split…';split.className='secondary';split.onclick=splitSelection;$('#properties-form').append(split);for(const m of manifests.filter(m=>m.inputs.includes(item.kind.toLowerCase()))){const button=document.createElement('button');button.textContent=m.label;button.title=`Sends ${item.label} to ${m.destination}`;button.onclick=()=>runTransform(m,selected[0]);$('#research').append(button)}}
}
function selectOnly(id:string){if(!model.entities.has(id))return;model.selection=new Set([id]);model.edgeId=undefined;refreshSelection()}
function openForm(content:string,onSubmit:(data:FormData)=>Promise<unknown>|unknown){const origin=model.project;form.innerHTML=content;form.onsubmit=e=>{e.preventDefault();const data=new FormData(form);void safe(async()=>{if(model.project!==origin)throw Error('Project changed. Reopen this action.');await onSubmit(data);dialog.close()})};form.querySelector('[data-cancel]')?.addEventListener('click',()=>dialog.close());dialog.showModal();setTimeout(()=>form.querySelector<HTMLInputElement>('input,textarea,select')?.focus(),0)}
const actions=(label='Create')=>`<div class="dialog-actions"><button type="button" data-cancel>Cancel</button><button class="primary" type="submit">${label}</button></div>`;
function createEntity(x?:number,y?:number){if(!ready)return;const anchor=model.selected()[0]??canvas.getCamera();const position={x:x??anchor.x+(model.entities.size?65:0),y:y??anchor.y+(model.entities.size?65:0)};openForm(`<h2>Add entity</h2>${field('Label','label','','required maxlength="4096" autocomplete="off"')}${field('Type','kind','entity','required list="entity-types" maxlength="128"')}${actions()}`,async d=>{const n=entity(String(d.get('label')).trim(),String(d.get('kind')).trim(),position.x,position.y);await apply({entities:[n]});selectOnly(n.id)})}
async function deleteSelection(){const ids=[...model.selection],edgeId=model.edgeId;if(!ids.length&&!edgeId)return;await apply(current=>({removeEntities:ids.filter(id=>current.entities.has(id)),removeRelationships:edgeId&&current.relationships.has(edgeId)?[edgeId]:[]}));model.selection.clear();model.edgeId=undefined;refreshSelection()}
function groupSelection(){if(!model.selection.size)return;const members=[...model.selection];openForm(`<h2>Group selection</h2>${field('Name','name','','required maxlength="256"')}${actions('Group')}`,d=>apply(current=>{for(const id of members)requireEntity(current,id);return{collections:[{id:crypto.randomUUID(),label:String(d.get('name')).trim(),members,color:'#8096bd'}]}}))}
function mergeSelection(){if(model.selection.size<2)return;const selected=model.selected(),ids=selected.map(n=>n.id);openForm(`<h2>Merge selection</h2><label class="field">Keep entity<select name="target">${selected.map(n=>`<option value="${esc(n.id)}">${esc(n.label)} · ${esc(n.id.slice(0,8))}</option>`).join('')}</select></label><div class="subtle">Original records are preserved in metadata. Links are redirected to the kept entity.</div>${actions('Merge')}`,async d=>{const id=String(d.get('target'));await apply(current=>{for(const selectedId of ids)requireEntity(current,selectedId);const view=Object.assign(new EditorModel(),current,{selection:new Set(ids)});return mergeEntities(view,id)});selectOnly(id)})}
function splitSelection(){const n=model.selected()[0];if(!n||model.selection.size!==1)return;openForm(`<h2>Split entity</h2>${field('New label','label',`${n.label} split`,'required')}<div class="subtle">Choose properties to move. Links stay with the original entity.</div>${Object.keys(n.properties).map(key=>`<label class="row"><input type="checkbox" name="property" value="${esc(key)}">${esc(key)}</label>`).join('')}${actions('Split')}`,async d=>{const delta=await apply(current=>splitEntity(requireEntity(current,n.id),String(d.get('label')),d.getAll('property').map(String)));selectOnly(delta.patch.entities![1].id)})}
async function previewImport(preview:ImportPreview,name:string,research=false,origin=model.project){
  if(model.project!==origin)throw Error('Project changed. Import again in the current project.');
  if(!preview.patch.entities?.length&&!preview.patch.relationships?.length){notice(preview.warnings.join(' ')||'No records found.');return}
  // Completed background work must not replace a dialog containing user input.
  while(dialog.open||palette.open){const active=dialog.open?dialog:palette;await new Promise<void>(resolve=>active.addEventListener('close',()=>resolve(),{once:true}));if(model.project!==origin)return}
  const patch=research?preview.patch:rekeyImport(preview.patch);const nodes=patch.entities??[];const shown=nodes.slice(0,150);const graph=!!patch.relationships?.length;
  const columns=[...new Set(nodes.flatMap(n=>Object.keys(n.properties)))];
  openForm(`<h2>${research?'Research results':`Import ${esc(name)}`}</h2>${preview.warnings.map(w=>`<div class="subtle">${esc(w)}</div>`).join('')}<div class="subtle">${nodes.length} entities${graph?`, ${patch.relationships!.length} relationships`:''}</div>${!research&&columns.length?`<div class="row"><label class="field" style="flex:1">Label column<select name="label-column"><option value="">Keep labels</option>${columns.map(c=>`<option>${esc(c)}</option>`).join('')}</select></label>${field('Entity type','entity-type','','placeholder="Keep types" list="entity-types"')}</div>`:''}<div class="preview-table"><table><thead><tr>${!graph&&nodes.length<=150?'<th><input type="checkbox" id="preview-all" aria-label="Select all imported entities" checked></th>':''}<th>Label</th><th>Type</th></tr></thead><tbody>${shown.map(n=>`<tr>${!graph&&nodes.length<=150?`<td><input type="checkbox" name="include" value="${esc(n.id)}" aria-label="Include ${esc(n.label)}" checked></td>`:''}<td>${esc(n.label)}</td><td>${esc(n.kind)}</td></tr>`).join('')}</tbody></table></div>${nodes.length>150?'<div class="subtle">Preview shows the first 150 records. All records will be imported.</div>':''}${actions('Add to graph')}`,async d=>{
    let entities=nodes;if(!graph&&nodes.length<=150){const included=new Set(d.getAll('include').map(String));entities=entities.filter(n=>included.has(n.id))}
    const labelColumn=String(d.get('label-column')??''),type=String(d.get('entity-type')??'').trim();
    entities=entities.map(n=>({...n,label:labelColumn?String(n.properties[labelColumn]??n.label):n.label,kind:type||n.kind}));
    const included=new Set(entities.map(n=>n.id));const collections=patch.collections?.map(c=>({...c,members:c.members.filter(id=>included.has(id)||model.entities.has(id))}));
    await apply({...patch,entities,collections},origin);if(model.project!==origin)return;model.selection=new Set(entities.slice(0,100).map(n=>n.id));model.edgeId=undefined;refreshSelection();canvas.fit();
  });
  form.querySelector<HTMLInputElement>('#preview-all')?.addEventListener('change',e=>{for(const c of form.querySelectorAll<HTMLInputElement>('[name=include]'))c.checked=(e.target as HTMLInputElement).checked});
}
function importText(){const origin=model.project;openForm(`<h2>Import text</h2><label class="field">Format<select name="format"><option value="txt">Text</option><option value="json">JSON</option><option value="csv">CSV</option></select></label><label class="field">Data<textarea name="text" rows="12" required spellcheck="false"></textarea></label>${actions('Preview')}`,async d=>{const result=await api.previewText(String(d.get('text')),String(d.get('format')),'Pasted data');if(model.project!==origin)throw Error('Project changed. Import again in the current project.');dialog.close();setTimeout(()=>void safe(()=>previewImport(result,'Pasted data',false,origin)),0)})}
async function runTransform(manifest:Manifest,input:Entity){
  const origin=model.project;
  if(jobId){notice('Wait for the active research job or cancel it.');return}
  openForm(`<h2>${esc(manifest.label)}</h2><div>${esc(input.label)}</div><div class="subtle">Send this value to ${esc(manifest.destination)}. Results will be previewed.</div>${actions('Run')}`,async()=>{
    if(model.project!==origin)throw Error('Project changed. Run research again in the current project.');
    jobId=await api.run(manifest.id,input.id);$('#job').hidden=false;$('#job-label').textContent=manifest.label;
    const activeId=jobId;
    const poll=async()=>{try{const job=await api.poll(activeId);if(job.status==='running'){setTimeout(()=>void poll(),350);return}jobId=undefined;$('#job').hidden=true;if(model.project!==origin)return;if(job.status==='failed')notice(job.error??'Research failed',true);else if(job.status==='complete'&&job.output)await previewImport(job.output,manifest.label,true,origin);else notice('Research cancelled.')}catch(e){jobId=undefined;$('#job').hidden=true;notice(String(e),true)}};
    void poll();
  })
}
async function load(snapshot:Snapshot,initial=false){canvas.cancelLayout();dialog.close();palette.close();model.load(snapshot);typeCounts.clear();for(const n of model.entities.values())countType(n.kind,1);title.value=model.project.title;ready=true;markDirty(false);search.value='';kindFilter.value='';canvas.setData(model.entities,model.relationships,model.collections);setTool('select');canvas.setSelection(model.selection,model.edgeId);if(initial&&snapshot.workspace){workspace=normalizeWorkspace(snapshot.workspace);applyWorkspace();if(workspace.camera)canvas.setCamera(workspace.camera);else canvas.fit()}else canvas.fit();refresh();if(snapshot.recovery)notice(`Recovered project: ${snapshot.recovery}`,false,true)}
async function save(saveAs=false){await projectSerial(async()=>{const result=await api.save(saveAs);if(result){if(result.revision===model.revision)markDirty(false);notice(`Saved ${result.path}`)}})}
async function arrange(){if(busy){canvas.cancelLayout();return}const origin=model.project;busy=true;$('#job').hidden=false;$('#job-label').textContent='Arranging graph';try{const positions=await canvas.layout();if(positions.length)await apply(current=>({entities:positions.map(p=>({...requireEntity(current,p.id),x:p.x,y:p.y}))}),origin);if(model.project===origin)canvas.fit()}finally{busy=false;$('#job').hidden=true}}
interface Command {id:string;label:string;key?:string;run:()=>Promise<unknown>|unknown;enabled?:()=>boolean}
const commands:Command[]=[
  {id:'create',label:'Add entity',key:'N',run:()=>createEntity()},
  {id:'new',label:'New project',run:()=>serial(async()=>load(await api.newProject()))},
  {id:'open',label:'Open project',key:'Ctrl O',run:()=>serial(async()=>{const s=await api.open();if(s)await load(s)})},
  {id:'save',label:'Save project',key:'Ctrl S',run:()=>save()},
  {id:'save-as',label:'Save project as…',key:'Ctrl Shift S',run:()=>save(true)},
  {id:'import',label:'Import file',key:'Ctrl I',run:async()=>{const origin=model.project;const p=await api.pickImport();if(p)await previewImport(p,p.name,false,origin)}},
  {id:'import-text',label:'Import pasted text',run:importText},
  {id:'undo',label:'Undo',key:'Ctrl Z',enabled:()=>model.canUndo,run:()=>projectSerial(async()=>acceptDelta(await api.undo(model.revision)))},
  {id:'redo',label:'Redo',key:'Ctrl Shift Z',enabled:()=>model.canRedo,run:()=>projectSerial(async()=>acceptDelta(await api.redo(model.revision)))},
  {id:'fit',label:'Fit graph',key:'F',run:()=>canvas.fit()},
  {id:'layout',label:'Arrange graph',run:arrange},
  {id:'select-all',label:'Select all visible entities',key:'Ctrl A',run:()=>{const q=search.value.toLowerCase();model.selection=new Set([...model.entities.values()].filter(n=>(!kindFilter.value||kindFilter.value===n.kind)&&(!q||`${n.label} ${n.kind} ${JSON.stringify(n.properties)}`.toLowerCase().includes(q))).map(n=>n.id));model.edgeId=undefined;refreshSelection()}},
  {id:'group',label:'Group selection',key:'Ctrl G',enabled:()=>model.selection.size>0,run:groupSelection},
  {id:'merge',label:'Merge selection…',enabled:()=>model.selection.size>1,run:mergeSelection},
  {id:'split',label:'Split entity…',enabled:()=>model.selection.size===1,run:splitSelection},
  {id:'delete',label:'Delete selection',key:'Delete',enabled:()=>model.selection.size>0||!!model.edgeId,run:deleteSelection},
  {id:'zoom-in',label:'Zoom in',run:()=>canvas.zoomBy(1.25)},
  {id:'zoom-out',label:'Zoom out',run:()=>canvas.zoomBy(.8)},
  {id:'help',label:'Keyboard and mouse controls',run:()=>openForm('<h2>Controls</h2><div class="subtle">Drag entities to move. Drag the background to pan.<br>Shift-click to select several. Shift-drag to select an area.<br>Scroll to zoom. Double-click the canvas to add an entity.<br>L connects two clicked entities. V returns to selection.<br>Right-click for actions. Escape clears selection.</div>'+actions('Close'),()=>undefined)}
];
function execute(id:string){if(id==='palette'){showPalette();return}if(!ready){notice('Open the desktop app to edit local projects.',true);return}const c=commands.find(c=>c.id===id);if(c&&(!c.enabled||c.enabled()))void safe(c.run)}
document.querySelectorAll<HTMLButtonElement>('[data-action]').forEach(b=>b.onclick=()=>execute(b.dataset.action!));
function showPalette(){if(dialog.open)dialog.close();palette.showModal();$<HTMLInputElement>('#command-search').value='';renderCommands();$<HTMLInputElement>('#command-search').focus()}
function renderCommands(){const query=$<HTMLInputElement>('#command-search').value.toLowerCase();const list=$('#command-list');list.replaceChildren();for(const c of commands.filter(c=>c.label.toLowerCase().includes(query))){const b=document.createElement('button');b.disabled=!!c.enabled&&!c.enabled();b.role='option';b.innerHTML=`${esc(c.label)}<kbd>${esc(c.key??'')}</kbd>`;b.onclick=()=>{palette.close();execute(c.id)};list.append(b)}}
$('#command-search').oninput=renderCommands;
$('#command-search').onkeydown=e=>{if(e.key==='ArrowDown'){e.preventDefault();$('#command-list').querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus()}if(e.key==='Enter'){e.preventDefault();$('#command-list').querySelector<HTMLButtonElement>('button:not(:disabled)')?.click()}};
$('#command-list').onkeydown=e=>{const buttons=[...$('#command-list').querySelectorAll<HTMLButtonElement>('button:not(:disabled)')];const i=buttons.indexOf(document.activeElement as HTMLButtonElement);if(e.key==='ArrowDown'||e.key==='ArrowUp'){e.preventDefault();buttons[(i+(e.key==='ArrowDown'?1:buttons.length-1))%buttons.length]?.focus()}};
function contextMenu(x:number,y:number,id?:string,edgeId?:string){if(id&&!model.selection.has(id))model.selection=new Set([id]);if(edgeId){model.selection.clear();model.edgeId=edgeId}else model.edgeId=undefined;refreshSelection();const menu=$('#context-menu');const ids=id||edgeId?['group','merge','delete']:['create','import','fit'];menu.innerHTML='';for(const key of ids){const c=commands.find(c=>c.id===key)!;if(c.enabled&&!c.enabled())continue;const b=document.createElement('button');b.textContent=c.label;b.onclick=()=>{menu.hidden=true;execute(c.id)};menu.append(b)}menu.hidden=false;menu.style.left=`${Math.min(x,innerWidth-200)}px`;menu.style.top=`${Math.min(y,innerHeight-menu.offsetHeight-8)}px`;menu.querySelector<HTMLButtonElement>('button')?.focus()}
document.addEventListener('pointerdown',e=>{if(!$('#context-menu').contains(e.target as Node))$('#context-menu').hidden=true});
document.addEventListener('keydown',e=>{
  if(e.key==='Escape'){$('#context-menu').hidden=true;if(!dialog.open&&!palette.open){model.selection.clear();model.edgeId=undefined;setTool('select');refreshSelection()}return}
  const target=e.target as HTMLElement;const editing=target.matches('input,textarea,select,[contenteditable=true]');const mod=e.ctrlKey||e.metaKey;
  if(mod&&e.key.toLowerCase()==='k'){e.preventDefault();showPalette();return}if(dialog.open||palette.open)return;
  if(mod&&e.key.toLowerCase()==='s'){e.preventDefault();execute(e.shiftKey?'save-as':'save');return}if(editing)return;
  const map:Record<string,string>={o:'open',i:'import',z:e.shiftKey?'redo':'undo',y:'redo',g:'group',a:'select-all'};
  if(mod&&e.key.toLowerCase()==='b'){e.preventDefault();togglePanel(e.shiftKey?'right':'left');return}
  const action=mod?map[e.key.toLowerCase()]:({n:'create',f:'fit',Delete:'delete',Backspace:'delete'}[e.key]);if(action){e.preventDefault();execute(action)}else if(!mod&&e.key.toLowerCase()==='l')setTool('link');else if(!mod&&e.key.toLowerCase()==='v')setTool('select');
});
function setTool(tool:'select'|'link'){canvas.setTool(tool);$('#select-tool').classList.toggle('active',tool==='select');$('#link-tool').classList.toggle('active',tool==='link')}
$('#select-tool').onclick=()=>setTool('select');$('#link-tool').onclick=()=>setTool('link');
search.oninput=()=>{clearTimeout(filterTimer);filterTimer=setTimeout(renderList,100)};kindFilter.onchange=renderList;
title.onchange=()=>{const name=title.value.trim()||'Untitled';void safe(()=>projectSerial(async()=>{model.revision=await api.rename(name);model.project.title=name;title.value=name;markDirty()}))};
$('#notice button').onclick=()=>$('#notice').hidden=true;
$('#cancel-job').onclick=()=>{if(busy)canvas.cancelLayout();else if(jobId)void safe(()=>api.cancel(jobId!))};
function applyWorkspace(){workspace.leftWidth=Math.max(180,Math.min(440,workspace.leftWidth));workspace.rightWidth=Math.max(230,Math.min(480,workspace.rightWidth));document.body.classList.toggle('left-closed',!workspace.left);document.body.classList.toggle('right-closed',!workspace.right);document.documentElement.style.setProperty('--left',`${workspace.leftWidth}px`);document.documentElement.style.setProperty('--right',`${workspace.rightWidth}px`)}
function persistWorkspace(){if(!ready)return;clearTimeout(workspaceTimer);workspaceTimer=setTimeout(()=>void safe(()=>api.workspace({...workspace,camera:canvas.getCamera()})),600)}
function togglePanel(side:'left'|'right'){workspace[side]=!workspace[side];applyWorkspace();persistWorkspace()}
$('#toggle-left').onclick=()=>togglePanel('left');$('#toggle-right').onclick=()=>togglePanel('right');
for(const side of ['left','right'] as const){const el=$(`#${side}-resizer`);el.onpointerdown=e=>{el.setPointerCapture(e.pointerId);const start=e.clientX;const width=workspace[`${side}Width`];el.onpointermove=m=>{workspace[`${side}Width`]=width+(m.clientX-start)*(side==='left'?1:-1);applyWorkspace()};el.onpointerup=()=>{el.onpointermove=null;persistWorkspace()}};el.onkeydown=e=>{if(e.key==='ArrowLeft'||e.key==='ArrowRight'){e.preventDefault();workspace[`${side}Width`]+=(e.key==='ArrowRight'?10:-10)*(side==='left'?1:-1);applyWorkspace();persistWorkspace()}}}
window.addEventListener('unhandledrejection',e=>{notice(String(e.reason),true);e.preventDefault()});

// Debug-only native test runner. It operates the real DOM and actual Rust IPC.
async function desktopSmoke(restore=false){
  const steps:string[]=[];try{
    if(restore){
      const records=[...model.entities.values()];
      if(records.length!==4||model.relationships.size!==1||!records.some(n=>n.label==='Edited domain')||!records.some(n=>n.label==='Alice'&&n.properties.role==='analyst'))throw Error('Autosave restoration did not retain the smoke graph');
      await invoke('smoke_result',{result:{ok:true,steps:['restore dirty autosave after abrupt process termination','retain edited label, imported properties and relationship'],entities:model.entities.size,relationships:model.relationships.size,userAgent:navigator.userAgent}});return;
    }
    const until=async(predicate:()=>boolean)=>{const deadline=performance.now()+8000;while(!predicate()){if(performance.now()>deadline)throw Error('Timed out waiting for the editor');await new Promise(resolve=>setTimeout(resolve,20))}};
    if(model.entities.size)throw Error('Smoke test requires an isolated empty data directory');
    document.querySelector<HTMLButtonElement>('[data-action=create]')!.click();
    form.querySelector<HTMLInputElement>('[name=label]')!.value='Smoke domain';form.querySelector<HTMLInputElement>('[name=kind]')!.value='domain';form.requestSubmit();await until(()=>!dialog.open&&model.selection.size===1);await editorQueue.settled;
    if(model.entities.size!==1)throw Error('Create via dialog failed');steps.push('create entity through actual dialog and Rust patch');
    const first=model.selected()[0];const second=entity('Second','entity',150,0);await apply({entities:[second]});await apply({relationships:[relationship(first.id,second.id)]});steps.push('directed relationship');
    selectOnly(first.id);const inspector=$<HTMLFormElement>('#properties-form');inspector.querySelector<HTMLInputElement>('[name=label]')!.value='Edited domain';inspector.requestSubmit();await until(()=>model.entities.get(first.id)?.label==='Edited domain');await editorQueue.settled;steps.push('inspector edit');
    await deleteSelection();if(model.relationships.size!==0)throw Error('Cascade delete failed');acceptDelta(await api.undo(model.revision));if(Number(model.relationships.size)!==1)throw Error('Undo failed');steps.push('delete cascade and restore');
    const preview=await api.previewText('name,role\nAlice,analyst\nBob,reviewer','csv','smoke.csv');await apply(rekeyImport(preview.patch));steps.push('CSV import real Rust parser');
    const frame=$<HTMLCanvasElement>('#canvas-host canvas');if(frame.width<100||frame.height<100)throw Error('Canvas has no surface');canvas.fit();steps.push('Canvas2D compatibility surface');
    await invoke('smoke_result',{result:{ok:true,steps,entities:model.entities.size,relationships:model.relationships.size,userAgent:navigator.userAgent}});notice('Desktop smoke passed.');
  }catch(e){await invoke('smoke_result',{result:{ok:false,steps,error:String(e)}});notice(`Desktop smoke failed: ${e}`,true,true)}
}
async function start(){try{await load(await api.boot(),true);manifests=await api.manifests();renderInspector();
  if(window.__TAURI__){const event=(window.__TAURI__ as unknown as {event?:{listen:(name:string,fn:(e:{payload:string})=>void)=>Promise<unknown>}}).event;if(event)await event.listen('autosave-error',e=>notice(e.payload,true,true))}
  // Rust opts into smoke tests by injecting an event; ordinary launches never run it.
  window.addEventListener('linkscope-smoke',event=>void desktopSmoke((event as CustomEvent<{restore?:boolean}>).detail?.restore===true),{once:true});
  await invoke('smoke_ready').catch(()=>undefined);
}catch(e){notice(String(e),true,true);for(const b of document.querySelectorAll<HTMLButtonElement>('button[data-action]:not([data-action=palette])'))b.disabled=true}}
void start();
