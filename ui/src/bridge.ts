import type {Delta,ImportPreview,JobResult,Manifest,Patch,Snapshot,Workspace} from './types';
declare global {interface Window {__TAURI_INTERNALS__?:unknown;__TAURI__?:{core:{invoke:<T>(command:string,args?:Record<string,unknown>)=>Promise<T>}}}}
export async function invoke<T>(command:string,args:Record<string,unknown>={}):Promise<T> {
  if(!window.__TAURI__?.core) throw new Error('Open LinkScope desktop to use local projects. Run npm run desktop.');
  return window.__TAURI__.core.invoke<T>(command,args);
}
export const api={
  boot:()=>invoke<Snapshot>('boot'),
  apply:(patch:Patch,revision:number)=>invoke<Delta>('apply_patch',{patch,revision}),
  undo:(revision:number)=>invoke<Delta>('undo',{revision}),redo:(revision:number)=>invoke<Delta>('redo',{revision}),
  newProject:()=>invoke<Snapshot>('new_project'),
  open:()=>invoke<Snapshot|null>('open_project'),
  save:(saveAs=false)=>invoke<{path:string;revision:number}|null>('save_project',{saveAs}),
  rename:(title:string)=>invoke<number>('rename_project',{title}),
  previewText:(text:string,format:string,name:string)=>invoke<ImportPreview>('preview_import',{text,format,name}),
  pickImport:()=>invoke<(ImportPreview & {name:string})|null>('pick_import'),
  workspace:(workspace:Workspace)=>invoke<void>('save_workspace',{workspace}),
  manifests:()=>invoke<Manifest[]>('manifests'),
  run:(kind:string,id:string)=>invoke<string>('start_transform',{kind,id}),
  poll:(id:string)=>invoke<JobResult>('poll_transform',{id}),
  cancel:(id:string)=>invoke<void>('cancel_transform',{id})
};
