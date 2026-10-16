import type {EditorModel} from './model';
import type {Entity,Relationship} from './types';

/** One mutation lane, bound to the loaded project instance rather than just its ID. */
export class EditorQueue {
  private tail:Promise<unknown>=Promise.resolve();
  constructor(private readonly model:EditorModel){}
  get settled(){return this.tail}
  run<T>(work:()=>Promise<T>):Promise<T>{
    const next=this.tail.then(work);this.tail=next.catch(()=>undefined);return next;
  }
  edit<T>(work:(current:EditorModel)=>Promise<T>,origin=this.model.project):Promise<T>{
    return this.run(async()=>{
      if(!origin)throw Error('Project is not ready');
      if(this.model.project!==origin)throw Error('Project changed. Retry the action in the current project.');
      return work(this.model);
    });
  }
}

export function requireEntity(model:EditorModel,id:string):Entity {
  const record=model.entities.get(id);if(!record)throw Error('Entity no longer exists');return record;
}
export function requireRelationship(model:EditorModel,id:string):Relationship {
  const record=model.relationships.get(id);if(!record)throw Error('Relationship no longer exists');return record;
}
