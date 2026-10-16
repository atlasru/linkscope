import type {Collection,Delta,Entity,Patch,Project,Relationship,Snapshot} from './types';
export class EditorModel {
  entities=new Map<string,Entity>();relationships=new Map<string,Relationship>();collections=new Map<string,Collection>();
  project!:Project;revision=0;canUndo=false;canRedo=false;selection=new Set<string>();edgeId:string|undefined;
  load(snapshot:Snapshot){this.project=snapshot.project;this.entities=new Map(this.project.entities.map(n=>[n.id,n]));this.relationships=new Map(this.project.relationships.map(e=>[e.id,e]));this.collections=new Map(this.project.collections.map(g=>[g.id,g]));this.revision=snapshot.revision;this.canUndo=snapshot.canUndo;this.canRedo=snapshot.canRedo;this.selection.clear();this.edgeId=undefined;}
  delta(d:Delta){for(const id of d.patch.removeRelationships??[])this.relationships.delete(id);for(const id of d.patch.removeEntities??[]){this.entities.delete(id);this.selection.delete(id)}for(const id of d.patch.removeCollections??[])this.collections.delete(id);for(const n of d.patch.entities??[])this.entities.set(n.id,n);for(const e of d.patch.relationships??[])this.relationships.set(e.id,e);for(const g of d.patch.collections??[])this.collections.set(g.id,g);if(this.edgeId&&!this.relationships.has(this.edgeId))this.edgeId=undefined;this.revision=d.revision;this.canUndo=d.canUndo;this.canRedo=d.canRedo;}
  selected(){return [...this.selection].map(id=>this.entities.get(id)).filter((n):n is Entity=>!!n)}
}
/** Import into a project creates new identities, even when external IDs collide. */
export function rekeyImport(patch:Patch):Patch {
  const ids=new Map((patch.entities??[]).map(n=>[n.id,crypto.randomUUID()]));
  return {entities:(patch.entities??[]).map(n=>({...n,id:ids.get(n.id)!,metadata:{...n.metadata,importedId:n.id}})),relationships:(patch.relationships??[]).map(e=>({...e,id:crypto.randomUUID(),source:ids.get(e.source)??e.source,target:ids.get(e.target)??e.target})),collections:(patch.collections??[]).map(c=>({...c,id:crypto.randomUUID(),members:c.members.map(id=>ids.get(id)??id)}))};
}
export function mergeEntities(model:EditorModel,targetId:string):Patch {
  const selected=model.selected();const target=model.entities.get(targetId);if(!target||selected.length<2)throw Error('Select two or more entities to merge');
  const removed=new Set(selected.filter(n=>n.id!==targetId).map(n=>n.id));
  const archive=target.metadata.mergedRecords;
  const prior=archive===undefined?[]:Array.isArray(archive)?archive:[{previousValue:archive}];
  const merged={...target,sources:selected.flatMap(n=>n.sources),metadata:{...target.metadata,mergedRecords:[...prior,...selected.filter(n=>n.id!==targetId).map(n=>JSON.parse(JSON.stringify(n)))]}};
  return {entities:[merged],removeEntities:[...removed],relationships:[...model.relationships.values()].filter(e=>removed.has(e.source)||removed.has(e.target)).map(e=>({...e,source:removed.has(e.source)?targetId:e.source,target:removed.has(e.target)?targetId:e.target})),collections:[...model.collections.values()].filter(g=>g.members.some(id=>removed.has(id))).map(g=>({...g,members:[...new Set(g.members.map(id=>removed.has(id)?targetId:id))]}))};
}
/** Explicitly transfer selected properties into a distinct entity; relationships stay with the original. */
export function splitEntity(original:Entity,label:string,keys:string[]):Patch {
  if(!label.trim())throw Error('The new entity needs a label');
  const remaining={...original.properties},moved:Record<string,import('./types').Json>={};
  for(const key of keys){if(!Object.hasOwn(remaining,key))throw Error(`Property no longer exists: ${key}`);moved[key]=remaining[key];delete remaining[key]}
  const copy={...original,id:crypto.randomUUID(),label:label.trim(),x:original.x+65,y:original.y+65,properties:moved,notes:'',metadata:{splitFrom:original.id}};
  return{entities:[{...original,properties:remaining},copy]};
}
