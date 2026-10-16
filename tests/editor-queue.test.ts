import test from 'node:test';
import assert from 'node:assert/strict';
import {EditorModel} from '../ui/src/model.ts';
import {EditorQueue,requireEntity,requireRelationship} from '../ui/src/queue.ts';
import {entity} from '../ui/src/types.ts';
import type {Snapshot} from '../ui/src/types.ts';

function setup(){
  const model=new EditorModel();const record=entity('A');
  const snapshot:Snapshot={project:{schemaVersion:2,id:'project',title:'Case',entities:[record],relationships:[],collections:[],metadata:{}},revision:0,canUndo:false,canRedo:false,path:null,recovery:null};
  model.load(snapshot);return{model,queue:new EditorQueue(model),record,snapshot};
}

test('queued movement reads the latest properties after an earlier edit',async()=>{
  const {model,queue,record}=setup();let release!:()=>void;
  const pending=new Promise<void>(resolve=>release=resolve);
  const edit=queue.edit(async current=>{await pending;const latest=requireEntity(current,record.id);current.delta({patch:{entities:[{...latest,notes:'Saved notes',properties:{rank:4}}]},revision:1,canUndo:true,canRedo:false})});
  const move=queue.edit(async current=>{const latest=requireEntity(current,record.id);current.delta({patch:{entities:[{...latest,x:80,y:40}]},revision:2,canUndo:true,canRedo:false})});
  release();await Promise.all([edit,move]);
  assert.equal(model.entities.get(record.id)?.notes,'Saved notes');assert.deepEqual(model.entities.get(record.id)?.properties,{rank:4});assert.equal(model.entities.get(record.id)?.x,80);
});

test('reopening the same project ID rejects old queued edits and undo actions',async()=>{
  const {model,queue,snapshot}=setup();let called=false;
  const change=queue.run(async()=>model.load({...snapshot,project:{...snapshot.project},revision:50}));
  const oldEdit=queue.edit(async()=>{called=true});
  await change;await assert.rejects(oldEdit,/Project changed/);assert.equal(called,false);
  await queue.edit(async()=>{called=true});assert.equal(called,true);
});

test('queued edits cannot resurrect records deleted by an earlier action',async()=>{
  const {model,queue,record}=setup();
  const remove=queue.edit(async current=>current.delta({patch:{removeEntities:[record.id]},revision:1,canUndo:true,canRedo:false}));
  const staleMove=queue.edit(async current=>{const latest=requireEntity(current,record.id);current.entities.set(record.id,{...latest,x:5})});
  await remove;await assert.rejects(staleMove,/no longer exists/);assert.equal(model.entities.size,0);
  assert.throws(()=>requireRelationship(model,'missing'),/no longer exists/);
});

test('a failed operation does not poison the lane for later edits',async()=>{
  const {queue}=setup();await assert.rejects(queue.edit(async()=>{throw Error('Rejected transaction')}),/Rejected transaction/);
  assert.equal(await queue.edit(async()=>42),42);await queue.settled;
});
