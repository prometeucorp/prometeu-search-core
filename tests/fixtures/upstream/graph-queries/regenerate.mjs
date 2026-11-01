// Development-only capture from the pinned SQLite query and traversal implementations.
import {createRequire} from 'node:module';
import {execFileSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdtempSync,rmSync} from 'node:fs';
import {dirname,join,resolve} from 'node:path';
import {tmpdir} from 'node:os';
import {fileURLToPath} from 'node:url';
const root=resolve(process.argv[2]),revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('Wrong reference revision');
const require=createRequire(join(root,'package.json'));
const {DatabaseConnection}=require('./dist/db/index.js');
const {QueryBuilder}=require('./dist/db/queries.js');
const {GraphTraverser}=require('./dist/graph/traversal.js');
const {GraphQueryManager}=require('./dist/graph/queries.js');
const directory=dirname(fileURLToPath(import.meta.url));
const clean=value=>JSON.parse(JSON.stringify(value,(_key,value)=>value instanceof Map?[...value.values()]:value),(key,value)=>key==='updatedAt'?0:value);
const old=JSON.parse(readFileSync(join(directory,'../resolution/cases.json'),'utf8'));
const hierarchy=JSON.parse(readFileSync(join(directory,'../hierarchy/cases.json'),'utf8'));
const inputs=[...old.cases.map(c=>({id:c.name,nodes:c.nodes,edges:c.edges,files:c.files})),...hierarchy.snapshots];
const node=(id,kind='function',file='main.ts')=>({id,kind,name:id,qualifiedName:id,filePath:file,language:'typescript',startLine:1,endLine:1,startColumn:0,endColumn:1,updatedAt:0});
const edge=(source,target,kind='calls',line=1,column=0)=>({source,target,kind,line,column});
inputs.push({id:'parallel-and-shorter-path',nodes:['start','a','b','c','d','e'].map(id=>node(id)),edges:[edge('start','a'),edge('a','b'),edge('b','c'),edge('start','c'),edge('c','d'),edge('d','e'),edge('e','start'),edge('start','a','references'),edge('start','a','calls',2),edge('start','start','calls',3)]});
inputs.push({id:'nested-container-impact',nodes:[node('root','class'),node('nested','class'),node('method','method'),node('caller'),node('type','interface'),node('consumer','function','other.test.ts')],edges:[edge('root','nested','contains'),edge('nested','method','contains'),edge('caller','method'),edge('root','type','implements'),edge('consumer','root','type_of'),edge('consumer','root','returns'),edge('consumer','root','decorates'),edge('consumer','caller')]});
const graphs=[];
for(const input of inputs){const temp=mkdtempSync(join(tmpdir(),'prometeu-query-'));let db;try{
 db=DatabaseConnection.initialize(join(temp,'oracle.sqlite'));const queries=new QueryBuilder(db.getDb());
 queries.insertNodes(input.nodes);queries.insertEdges(input.edges);
 for(const path of new Set(input.nodes.map(n=>n.filePath)))queries.upsertFile({path,contentHash:'fixture',language:input.nodes.find(n=>n.filePath===path).language,size:0,modifiedAt:0,indexedAt:0,nodeCount:input.nodes.filter(n=>n.filePath===path).length});
 const walk=new GraphTraverser(queries),manager=new GraphQueryManager(queries);
 const nodes=queries.getAllNodes(),edges=queries.getOutgoingEdgesFrom(nodes.map(n=>n.id));
 const cases=[];
 const edgeKey=edge=>JSON.stringify([edge.source,edge.target,edge.kind,edge.line??null,edge.column??null]);
 const edgeIndex=new Map(edges.map((edge,index)=>[edgeKey(edge),index]));
 const compact=value=>JSON.parse(JSON.stringify(clean(value),(_key,value)=>value&&value.filePath&&value.kind&&value.id?{$node:value.id}:value&&value.source&&value.target&&value.kind?{$edge:edgeIndex.get(edgeKey(value))}:value));
 const add=(operation,id,options,expected)=>cases.push({operation,id,options,expected:compact(expected)});
 const selected=nodes.length>40?[nodes[0],...nodes.filter(n=>n.id==='root'||n.id==='child'||n.id==='leaf')]:nodes;
 for(const node of [...selected,{id:'missing'}]){
  for(const maxDepth of [0,1,2,4])for(const direction of ['outgoing','incoming','both'])for(const includeStart of [true,false]){
   for(const limit of [1,4,1000]){const options={maxDepth,direction,includeStart,limit};add('bfs',node.id,options,walk.traverseBFS(node.id,options));add('dfs',node.id,options,walk.traverseDFS(node.id,options));}
  }
  for(const options of [{edgeKinds:['calls','references'],maxDepth:3,nodeKinds:['function','method']},{direction:'both',maxDepth:2,nodeKinds:['class','interface']},{edgeKinds:['contains'],maxDepth:5}]){add('bfs',node.id,options,walk.traverseBFS(node.id,options));add('dfs',node.id,options,walk.traverseDFS(node.id,options));}
  for(const depth of [0,1,2,4]){add('callers',node.id,{depth},walk.getCallers(node.id,depth));add('callees',node.id,{depth},walk.getCallees(node.id,depth));add('call_graph',node.id,{depth},walk.getCallGraph(node.id,depth));for(const budget of [{},{maxNodes:1,maxEdges:0},{maxNodes:4,maxEdges:3}])add('impact',node.id,{depth,...budget},walk.getImpactRadius(node.id,depth,budget));}
  for(const direction of ['callers','callees'])add('call_sites',node.id,{direction},walk.getCallSites(node.id,direction));
  add('usages',node.id,{},walk.findUsages(node.id));add('ancestors',node.id,{},walk.getAncestors(node.id));add('children',node.id,{},walk.getChildren(node.id));add('type_graph',node.id,{},walk.getTypeHierarchy(node.id));
  if(node.id!=='missing')add('symbol_context',node.id,{},manager.getContext(node.id));
  for(const target of selected.slice(0,3))for(const kinds of [[],['calls']])add('path',node.id,{target:target.id,edgeKinds:kinds},walk.findPath(node.id,target.id,kinds));
 }
 for(const file of queries.getAllFiles()){add('dependencies',file.path,{},manager.getFileDependencies(file.path).sort());add('dependents',file.path,{},manager.getFileDependents(file.path).sort());add('exports',file.path,{},manager.getExportedSymbols(file.path));}
 for(const pattern of ['*','*::?*','*work*','*[literal]*'])add('qualified',pattern,{},manager.findByQualifiedName(pattern));
 for(const includeEdges of [false,true])add('filtered','',{includeEdges},manager.getFilteredSubgraph(n=>n.kind!=='file',includeEdges));
 graphs.push({id:input.id,files:input.files,nodes:clean(nodes),edges:clean(edges),cases});console.log(input.id,cases.length);
}finally{db?.close();rmSync(temp,{recursive:true,force:true});}}
writeFileSync(join(directory,'cases.json'),JSON.stringify({revision,graphs})+'\n');
writeFileSync(join(directory,'LICENSE'),readFileSync(join(root,'LICENSE')));
