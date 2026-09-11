// Multilevel Louvain, undirected weighted projection. Runs exclusively in a worker.
// Edge direction/provenance remain intact in the investigation graph.
export function louvain(count,links,maxLevels=8){
 let adjacency=Array.from({length:count},()=>new Map());for(let k=0;k<links.length;k+=2){const a=links[k],b=links[k+1];if(a>=count||b>=count)throw new Error('Invalid endpoint');adjacency[a].set(b,(adjacency[a].get(b)||0)+1);adjacency[b].set(a,(adjacency[b].get(a)||0)+1)}
 let membership=Int32Array.from({length:count},(_,i)=>i);
 for(let level=0;level<maxLevels;level++){
  const n=adjacency.length,degree=Float64Array.from(adjacency,a=>[...a.values()].reduce((s,v)=>s+v,0)),total=degree.reduce((s,v)=>s+v,0);if(!total)break;
  const community=Int32Array.from({length:n},(_,i)=>i),totals=degree.slice();let moved=false;
  for(let pass=0;pass<15;pass++){let changes=0;for(let i=0;i<n;i++){
   const old=community[i],weight=new Map();for(const[j,w]of adjacency[i])if(j!==i)weight.set(community[j],(weight.get(community[j])||0)+w);
   totals[old]-=degree[i];let best=old,bestGain=(weight.get(old)||0)-degree[i]*totals[old]/total;
   for(const[c,w]of weight){const gain=w-degree[i]*totals[c]/total;if(gain>bestGain+1e-10){bestGain=gain;best=c}}
   community[i]=best;totals[best]+=degree[i];if(best!==old)changes++;
  }if(!changes)break;moved=true;}
  const renumber=new Map();for(let i=0;i<n;i++){const c=community[i];if(!renumber.has(c))renumber.set(c,renumber.size);community[i]=renumber.get(c)}
  for(let i=0;i<count;i++)membership[i]=community[membership[i]];
  if(!moved||renumber.size===n)break;
  const next=Array.from({length:renumber.size},()=>new Map());for(let i=0;i<n;i++)for(const[j,w]of adjacency[i]){const a=community[i],b=community[j];next[a].set(b,(next[a].get(b)||0)+w)}adjacency=next;
 }return membership;
}
if(typeof self!=='undefined'&&typeof document==='undefined')self.onmessage=e=>{try{const labels=louvain(e.data.count,e.data.links);self.postMessage({labels},[labels.buffer]);}catch(error){self.postMessage({error:error.message})}};
