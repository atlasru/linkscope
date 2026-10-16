export type Json = null | boolean | number | string | Json[] | {[key:string]:Json};
export interface SourceRef {id:string;label:string;uri?:string|null;retrievedAt?:string|null;record?:Json}
export interface Entity {id:string;kind:string;label:string;x:number;y:number;properties:Record<string,Json>;metadata:Record<string,Json>;notes:string;sources:SourceRef[]}
export interface Relationship {id:string;source:string;target:string;kind:string;label:string;directed:boolean;properties:Record<string,Json>;metadata:Record<string,Json>;notes:string;sources:SourceRef[]}
export interface Collection {id:string;label:string;members:string[];color:string}
export interface Project {schemaVersion:number;id:string;title:string;entities:Entity[];relationships:Relationship[];collections:Collection[];metadata:Record<string,Json>}
export interface Patch {entities?:Entity[];relationships?:Relationship[];collections?:Collection[];removeEntities?:string[];removeRelationships?:string[];removeCollections?:string[]}
export interface Delta {patch:Patch;revision:number;canUndo:boolean;canRedo:boolean}
export interface Snapshot {project:Project;revision:number;canUndo:boolean;canRedo:boolean;path:string|null;recovery:string|null;workspace?:Workspace}
export interface Workspace {left:boolean;right:boolean;leftWidth:number;rightWidth:number;camera?:{x:number;y:number;zoom:number}}
export function normalizeWorkspace(value:unknown):Workspace {
  const p=value&&typeof value==='object'?value as Partial<Workspace>:{};
  const size=(n:unknown,fallback:number,min:number,max:number)=>typeof n==='number'&&Number.isFinite(n)?Math.max(min,Math.min(max,n)):fallback;
  const result:Workspace={left:typeof p.left==='boolean'?p.left:true,right:typeof p.right==='boolean'?p.right:true,leftWidth:size(p.leftWidth,244,180,440),rightWidth:size(p.rightWidth,280,230,480)};
  if(p.camera&&[p.camera.x,p.camera.y,p.camera.zoom].every(n=>typeof n==='number'&&Number.isFinite(n))&&p.camera.zoom>0)result.camera={x:p.camera.x,y:p.camera.y,zoom:size(p.camera.zoom,1,.015,8)};
  return result;
}
export interface ImportPreview {patch:Patch;warnings:string[]}
export interface Manifest {id:string;label:string;inputs:string[];destination:string}
export interface JobResult {id:string;status:'running'|'complete'|'cancelled'|'failed';output?:ImportPreview & {cached:boolean};error?:string}
export const entity = (label:string,kind='entity',x=0,y=0):Entity => ({id:crypto.randomUUID(),label,kind,x,y,properties:{},metadata:{},notes:'',sources:[]});
export const relationship = (source:string,target:string):Relationship => ({id:crypto.randomUUID(),source,target,kind:'related',label:'',directed:true,properties:{},metadata:{},notes:'',sources:[]});
