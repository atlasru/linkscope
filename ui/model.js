export const confidenceNames=['data','information','intelligence','counter-intelligence'];
export const colors={domain:[.46,.64,.95],ip:[.43,.78,.72],username:[.72,.57,.92],url:[.86,.68,.42],email:[.89,.52,.61]};
export class GraphModel{
 constructor(){this.nodes=[];this.edges=[];this.ids=new Map();this.edgeIds=new Set();this.positions=new Float32Array(0);this.links=new Uint32Array(0);this.revision=0;}
 merge(batch,rebuild=true){let added=[];for(const n of batch.nodes){let i=this.ids.get(n.id);if(i===undefined){i=this.nodes.length;this.ids.set(n.id,i);this.nodes.push({...n,attributes:undefined});added.push(i);}else this.nodes[i]={...this.nodes[i],...n,attributes:undefined};}
 for(const e of batch.edges){if(this.edgeIds.has(e.id))continue;const a=this.ids.get(e.source),b=this.ids.get(e.target);if(a===undefined||b===undefined)continue;this.edgeIds.add(e.id);this.edges.push({id:e.id,a,b});}
 if(rebuild)this.rebuild();this.revision++;return added;}
 rebuild(){this.positions=new Float32Array(this.nodes.length*2);this.nodes.forEach((n,i)=>{this.positions[i*2]=n.x;this.positions[i*2+1]=n.y});this.links=new Uint32Array(this.edges.length*2);this.edges.forEach((e,i)=>{this.links[i*2]=e.a;this.links[i*2+1]=e.b});}
 setPositions(p){this.positions=p.slice(0,this.nodes.length*2);this.nodes.forEach((n,i)=>{n.x=p[i*2];n.y=p[i*2+1]});}
}
export function synthetic(count=350){
 const model=new GraphModel();let seed=1337;const random=()=>{seed=(Math.imul(seed,1664525)+1013904223)>>>0;return seed/4294967296};
 const kinds=Object.keys(colors),clusters=count>1000?36:9;const base=Date.UTC(2026,7,1)/1000;
 for(let i=0;i<count;i++){const c=i%clusters;const angle=c/clusters*Math.PI*2;const r=Math.sqrt(random())*(count>1000?100:65);const a=random()*Math.PI*2;const kind=kinds[i%5];const value=i===0?'example.org':kind==='domain'?`node-${i}.example.org`:kind==='ip'?`192.0.2.${i%254+1}`:kind==='url'?`https://example.org/item/${i}`:kind==='email'?`analyst-${i}@example.org`:`researcher_${i}`;const n={id:`demo-${i}`,kind,value,x:Math.cos(angle)*(count>1000?380:240)+Math.cos(a)*r,y:Math.sin(angle)*(count>1000?250:170)+Math.sin(a)*r,confidence:confidenceNames[i%4],sources:['synthetic fixture'],timestamp:base+Math.floor(random()*40)*86400,cluster:c};model.ids.set(n.id,i);model.nodes.push(n);}
 const target=count===50000?200000:count===100000?500000:count*3;
 for(let i=0;i<target;i++){const a=i%count;let b;if(random()<.93){b=(a%clusters)+Math.floor(random()*Math.floor(count/clusters))*clusters;b=Math.min(b,count-1);}else b=Math.floor(random()*count);if(a===b)b=(b+clusters)%count;model.edges.push({id:`e${i}`,a,b});}
 model.rebuild();model.revision++;return model;
}
export function timeRange(from,to){const lo=from?Date.parse(from+'T00:00:00Z')/1000:-Infinity;const hi=to?Date.parse(to+'T23:59:59Z')/1000:Infinity;if(lo>hi)throw new Error('Начальная дата позже конечной');return[lo,hi]}
export function convexHull(points){if(points.length<3)return points;const p=points.slice().sort((a,b)=>a[0]-b[0]||a[1]-b[1]);const cross=(o,a,b)=>(a[0]-o[0])*(b[1]-o[1])-(a[1]-o[1])*(b[0]-o[0]);const lower=[],upper=[];for(const a of p){while(lower.length>=2&&cross(lower.at(-2),lower.at(-1),a)<=0)lower.pop();lower.push(a)}for(let i=p.length-1;i>=0;i--){const a=p[i];while(upper.length>=2&&cross(upper.at(-2),upper.at(-1),a)<=0)upper.pop();upper.push(a)}return lower.slice(0,-1).concat(upper.slice(0,-1))}
