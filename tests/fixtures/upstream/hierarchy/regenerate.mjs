// Development-only oracle; the pinned implementation supplies every expected answer.
import {createRequire} from 'node:module';
import {execFileSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdirSync,mkdtempSync,rmSync} from 'node:fs';
import {dirname,join,resolve} from 'node:path';
import {tmpdir} from 'node:os';
import {fileURLToPath} from 'node:url';
const root=resolve(process.argv[2]), revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('Wrong reference revision');
const require=createRequire(join(root,'package.json'));
const Project=Object.values(require('./dist/index.js')).find(value=>typeof value==='function'&&typeof value.init==='function'&&typeof value.open==='function');
const {buildTypeHierarchy,countImplementers,canHaveHierarchy}=require('./dist/graph/type-hierarchy.js');
const clean=value=>JSON.parse(JSON.stringify(value,(_key,value)=>value instanceof Map?Object.fromEntries(value):value), (key,value)=>key==='updatedAt'?0:value);
const projects=[
 {id:'typescript',files:{'types.ts':`export interface Drawable { draw(): string; }
export abstract class Shape implements Drawable { draw(): string { return 'shape'; } area(): number { return 0; } }
export class Square extends Shape { draw(): string { return 'square'; } }
export class Tile extends Square { label = 'tile'; }
export interface Plugin { run(): void; }
${['Alpha','Bravo','Charlie','Delta','Echo','Foxtrot','Golf','Hotel','India'].map(name=>`export class ${name}Plugin implements Plugin { run(): void {} }`).join('\n')}
export class Unrelated { run(): void {} }
export function work() {}`}},
 {id:'go',files:{'go.mod':'module fixture\n\ngo 1.22\n','clock.go':`package clock
type Clock interface { Now() int }
type System struct{}
func (System) Now() int { return 1 }
type Fixed struct{ At int }
func (f Fixed) Now() int { return f.At }
type Unrelated struct{}
`}},
 {id:'rust',files:{'Cargo.toml':'[package]\nname="fixture"\nversion="0.1.0"\n','src/lib.rs':`pub trait Writer { fn write(&self); }
pub struct Store;
impl Writer for Store { fn write(&self) {} }
pub struct Empty;
`}},
 {id:'java',files:{'Types.java':`interface Task { void execute(); }
abstract class Base implements Task { public void execute() {} }
class Child extends Base { public void execute() {} }
class Leaf extends Child {}
`}},
];
function capture(graph,nodes){return nodes.map(node=>({id:node.id,eligible:canHaveHierarchy(node),count:countImplementers(graph,node.id),hierarchy:clean(buildTypeHierarchy(graph,node)),withoutOverrides:clean(buildTypeHierarchy(graph,node,{overrides:false}))}));}
for(const project of projects){const dir=mkdtempSync(join(tmpdir(),'prometeu-hierarchy-'));let graph;try{
 for(const[path,source]of Object.entries(project.files)){mkdirSync(dirname(join(dir,path)),{recursive:true});writeFileSync(join(dir,path),source);}
 graph=await Project.init(dir,{silent:true,config:{exclude:[]}});const report=await graph.indexAll();if(!report.success)throw Error('Index failed');
 const nodes=graph.queries.getAllNodes();project.nodes=clean(nodes);project.edges=clean(graph.getOutgoingEdgesFrom(nodes.map(n=>n.id)));project.queries=capture(graph,nodes);
 console.log(project.id,project.nodes.length,project.queries.length);
}finally{graph?.close();rmSync(dir,{recursive:true,force:true});}}
const snapshots=[];
const node=(id,name=id,kind='class',line=1)=>({id,name,kind,qualifiedName:name,filePath:'types.ts',language:'typescript',startLine:line,endLine:line,startColumn:0,endColumn:0,updatedAt:0});
const edge=(source,target,kind='extends',extra={})=>({source,target,kind,...extra});
function add(id,nodes,edges){const query=(ids,kinds,up)=>edges.filter(e=>ids.includes(up?e.source:e.target)&&(!kinds||kinds.includes(e.kind))).sort((a,b)=>{const x=up?a.source:a.target,y=up?b.source:b.target;return x<y?-1:x>y?1:a.kind<b.kind?-1:a.kind>b.kind?1:0;});const graph={getNodesByIds:ids=>new Map(nodes.filter(n=>ids.includes(n.id)).map(n=>[n.id,n])),getOutgoingEdgesFrom:(ids,kinds)=>query(ids,kinds,true),getIncomingEdgesTo:(ids,kinds)=>query(ids,kinds,false)};snapshots.push({id,nodes,edges,queries:capture(graph,nodes.filter(n=>n.id==='root'||n.id==='leaf'||n.id==='child'))});}
add('no-hierarchy',[node('root'),node('child','work','function')],[]);
add('cycle-and-missing',[node('root'),node('child')],[edge('root','child'),edge('child','root'),edge('child','missing'),edge('missing','root')]);
add('duplicate-relations',[node('root'),node('child')],[edge('child','root','implements',{provenance:'heuristic'}),edge('child','root')]);
add('locale-order',[node('root'),...['Z','z','á','a','Ä','a2','a10','a_1','中文','😀'].map((name,i)=>node(`child${i}`,name))],Array.from({length:10},(_,i)=>edge(`child${i}`,'root')));
add('depth-limit',[node('root'),...Array.from({length:10},(_,i)=>node(i===9?'leaf':`n${i}`))],Array.from({length:10},(_,i)=>edge(i===9?'leaf':`n${i}`,i?`n${i-1}`:'root')));
add('fan-limit',[node('root','Contract','interface'),...Array.from({length:430},(_,i)=>node(`n${i}`))],Array.from({length:430},(_,i)=>edge(`n${i}`,'root','implements',{provenance:'heuristic',metadata:{synthesizedBy:'go-implements'}})));
add('hidden-fan',[node('root'),node('child'),...Array.from({length:410},(_,i)=>node(`n${i}`))],[edge('child','root'),...Array.from({length:410},(_,i)=>edge(`n${i}`,'child'))]);
add('overrides',[node('root'),node('child'),node('leaf'),node('base','run','method',2),node('near','run','method',3),node('own','run','method',4),node('field','value','field',5),node('basefield','value','property',6)],[edge('child','root'),edge('leaf','child','implements'),edge('root','base','contains'),edge('child','near','contains'),edge('leaf','own','contains'),edge('root','basefield','contains'),edge('leaf','field','contains')]);
const destination=dirname(fileURLToPath(import.meta.url));writeFileSync(join(destination,'cases.json'),JSON.stringify({revision,projects,snapshots},null,2)+'\n');writeFileSync(join(destination,'LICENSE'),readFileSync(join(root,'LICENSE')));
