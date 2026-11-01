// Run against the pinned checkout; expectations come from its real SQLite queries.
import {createRequire} from 'node:module';
import {mkdtempSync,rmSync,writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join,resolve} from 'node:path';
const require=createRequire(join(resolve(process.argv[2]),'package.json'));
const {DatabaseConnection}=require('./dist/db/index.js');
const {QueryBuilder}=require('./dist/db/queries.js');
const {buildDeadCodeReport}=require('./dist/graph/dead-code.js');
const {referenceNameTail}=require('./dist/db/reference-tail.js');
const cases=[];
function fixture(){
 const input={nodes:[],edges:[],unresolved:[],sources:{},generated:[]};
 const files=new Set();
 const node=(id,name,kind='function',filePath='src/work.ts',extra={})=>{
  const startLine=1+input.nodes.filter(n=>n.filePath===filePath).length*3;
  const value={id,name,kind,filePath,qualifiedName:name,language:'typescript',startLine,endLine:startLine+1,startColumn:0,endColumn:0,updatedAt:0,...extra};input.nodes.push(value);
  input.sources[filePath]=(input.sources[filePath]??'')+`${name}\n`;
  return value;
 };
 const edge=(source,target,kind='calls')=>input.edges.push({source:typeof source==='string'?source:source.id,target:typeof target==='string'?target:target.id,kind});
 const ref=(from,name,kind='calls')=>input.unresolved.push({fromNodeId:typeof from==='string'?from:from.id,referenceName:name,referenceKind:kind,line:1,column:0});
 const reached=(file='src/work.ts')=>{
  if(files.has(file))return;files.add(file);
  const anchor=node(`anchor:${file}`,`anchor${files.size}`,'variable',file,{isExported:true});
  const outside=node(`outside:${file}`,`outside${files.size}`,'file',`entry/${files.size}.ts`);
  edge(outside,anchor);
 };
 return {input,node,edge,ref,reached};
}
function add(id,build,options={},corroborate=true){
 const f=fixture();build(f);const input=f.input;
 const temp=mkdtempSync(join(tmpdir(),'prometeu-deadcode-oracle-'));
 const db=DatabaseConnection.initialize(join(temp,'oracle.sqlite'));
 try{
  const queries=new QueryBuilder(db.getDb());
  for(const file of new Set(input.nodes.map(n=>n.filePath)))queries.upsertFile({path:file,contentHash:'fixture',language:'typescript',size:(input.sources[file]??'').length,modifiedAt:0,indexedAt:0,nodeCount:0,generated:input.generated.includes(file)});
  queries.insertNodes(input.nodes);queries.insertEdges(input.edges);queries.insertUnresolvedRefsBatch(input.unresolved);
  for(const ref of input.unresolved)db.getDb().prepare('UPDATE unresolved_refs SET status=\'failed\',name_tail=? WHERE from_node_id=? AND reference_name=?').run(referenceNameTail(ref.referenceName,ref.referenceKind),ref.fromNodeId,ref.referenceName);
  input.nodes=[...queries.getNodesByIds(input.nodes.map(n=>n.id)).values()];
  const graph=new Proxy({}, {get:(_,name)=> name==='getFileDependentCounts'?(paths)=>new Map(queries.getFileDependentCounts(paths).map(r=>[r.filePath,r.dependents])):name==='getFileDependents'?(path)=>queries.getDependentFilePaths(path):queries[name].bind(queries)});
  const {include_exported:includeExported,include_tests:includeTests,include_generated:includeGenerated,...rest}=options;
  const expected=buildDeadCodeReport(graph,{...rest,includeExported,includeTests,includeGenerated,limit:options.limit??20,readSource:corroborate?path=>input.sources[path]??null:null});
  cases.push({id,input,options,corroborate,expected});
 }finally{db.close();rmSync(temp,{recursive:true});}
}
add('empty',()=>{});
add('plain-and-folded',f=>{f.reached();f.node('dead','unused');const c=f.node('c','UnusedClass','class',undefined,{endLine:100});const m=f.node('m','unusedMethod','method',undefined,{qualifiedName:'UnusedClass.unusedMethod'});f.edge(c,m,'contains');f.node('small','smaller');});
add('cheap-exclusions',f=>{for(const file of ['tests/code.ts','generated/code.ts','src/header.d.ts','external/code.ts','src/component.vue'])f.reached(file);f.reached();f.node('test','testingHelper','function','tests/code.ts');f.node('generated','generatedHelper','function','generated/code.ts');f.input.generated.push('generated/code.ts');f.node('header','headerHelper','function','src/header.d.ts');f.node('vendor','vendorHelper','function','external/code.ts');f.node('markup','markupHelper','function','src/component.vue',{language:'vue'});f.node('public','publicEntry','function',undefined,{isExported:true});f.node('abstract','abstractEntry','method',undefined,{isAbstract:true});f.node('scope','scopeHelper','function',undefined,{qualifiedName:'Service.tests.scopeHelper'});f.node('implicit','constructor','method');f.node('python','pythonHelper','function',undefined,{language:'python'});});
for(const wide of [false,true])add(`options-${wide}`,f=>{f.reached('tests/api.ts');f.node('export','exportedTest','function','tests/api.ts',{isExported:true});f.input.generated.push('tests/api.ts');}, {include_tests:wide,include_generated:wide,include_exported:wide});
add('unknown-export-language',f=>{f.node('py','pythonHelper','function','src/python.py',{language:'python'});f.reached('src/python.py');});
add('unreachable-before-decorators',f=>{f.node('island','islandFunction','function',undefined,{decorators:['External']});});
add('all-decorator-sources',f=>{f.reached();const a=f.node('a','decoratedList','method',undefined,{decorators:['External']});const b=f.node('b','decoratedEdge','method');const c=f.node('c','decoratedRef','method');const decorator=f.node('decorator','External','variable');f.edge(b,decorator,'decorates');f.ref(c,'ExternalDecorator','decorates');});
add('interface-and-override',f=>{f.reached();const base=f.node('base','Base','class');const method=f.node('base-m','run','method');f.edge(base,method,'contains');const child=f.node('child','Child','class');const override=f.node('child-m','run','method');f.edge(child,override,'contains');f.edge(child,base,'extends');const contract=f.node('i','Contract','interface');const declaration=f.node('i-m','declared','method');f.edge(contract,declaration,'contains');const impl=f.node('impl','Implementation','class');const body=f.node('impl-m','body','method');f.edge(impl,body,'contains');f.edge(impl,contract,'implements');});
add('opaque-and-external-ancestors',f=>{f.reached();const opaque=f.node('opaque','Opaque','interface');const a=f.node('a','Impl','class');const am=f.node('am','someMethod','method');f.edge(a,am,'contains');f.edge(a,opaque,'implements');const b=f.node('b','ExternalBased','class');const bm=f.node('bm','otherMethod','method');f.edge(b,bm,'contains');f.ref(b,'Library.Base','extends');const c=f.node('c','Descendant','class');const cm=f.node('cm','descendantMethod','method');f.edge(c,cm,'contains');f.edge(c,b,'extends');});
add('unresolved-and-ambiguous-names',f=>{f.reached();const caller=f.node('caller','caller','variable');const a=f.node('a','same','method');f.node('b','same','method');f.edge(a,a);f.node('pending','pending');f.ref(caller,'pkg.pending');f.node('arity','erlangCall');f.ref(caller,'module::erlangCall/2');f.node('self','recursive');f.edge('self','self');});
add('source-corroboration',f=>{f.reached();f.node('mentioned','mentioned');f.input.sources['src/work.ts']+='// registration: mentioned\n';f.node('cross','dependentMention');f.input.sources['entry/1.ts']+='dependentMention\n';f.node('alone','standalone');});
add('source-disabled',f=>{f.reached();f.node('mentioned','mentioned');f.input.sources['src/work.ts']+='mentioned\n';},{},false);
add('source-unreadable',f=>{f.reached();f.node('missing','missing');delete f.input.sources['src/work.ts'];});
add('unicode-mention-boundaries',f=>{f.reached();for(const name of ['café','foo','$value','𐐀'])f.node(name,name);f.input.sources['src/work.ts']+='décafé foo2 𐐀foo $valueMore 𐐀𐐀\n';});
add('implicit-ascii-case-folding',f=>{f.reached();for(const name of ['__ENTER__','__K__','____','_____','ToString','mainly'])f.node(name,name);});
function hookFixture(f,weak=false,plain=false){
 f.reached();
 for(let i=0;i<3;i++){
  const c=f.node(`class${i}`,`Class${i}`,'class',undefined,{decorators:plain?[]:['Component']});
  if(i<2)f.ref(c,'Lifecycle','implements');
  if(i!==1||!weak){const m=f.node(`hook${i}`,'onLifecycle','method',undefined,{qualifiedName:`Class${i}.onLifecycle`});f.edge(c,m,'contains');}
  const helper=f.node(`helper${i}`,`unusedHelper${i}`,'method');f.edge(c,helper,'contains');
 }
}
add('by-name-framework-hook',f=>hookFixture(f));
add('insufficient-hook-evidence',f=>hookFixture(f,true));
add('undecorated-hook-names',f=>hookFixture(f,false,true));
for(const variation of ['inherited','opaque','missing','minority'])add(`hook-contract-${variation}`,f=>{
 hookFixture(f);
 const c=f.node('writer3','Writer3','class',undefined,{decorators:['Component']});f.ref(c,'Lifecycle','implements');
 if(variation==='inherited')f.edge(c,'class0','extends');
 if(variation==='opaque')f.ref(c,'ExternalBase','extends');
 if(variation==='minority'){const d=f.node('writer4','Writer4','class',undefined,{decorators:['Component']});f.ref(d,'Lifecycle','implements');f.ref(c,'ExternalBase','extends');f.ref(d,'ExternalBase','extends');}
});
add('ancestor-cycle',f=>{f.reached();const a=f.node('a','A','class'),b=f.node('b','B','class'),c=f.node('c','C','class');f.edge(a,b,'extends');f.edge(b,c,'extends');f.edge(c,a,'extends');for(const [id,owner] of [['x',a],['y',b],['z',c]]){const m=f.node(id,id,'method');f.edge(owner,m,'contains');}});
add('reference-tail-kinds',f=>{f.reached();const caller=f.node('caller','caller','variable');for(const [id,name,ref,kind] of [['a','db','inc/db.php','imports'],['b','price.liquid','snippets/price.liquid','references'],['c','php','inc/file.php','imports'],['d','fn','mod::fn/123','calls'],['e','Name','import:./component#Name','calls']]){f.node(id,name);f.ref(caller,ref,kind);}});
add('source-cache-cap',f=>{for(let i=0;i<305;i++){const file=`src/unit${String(i).padStart(3,'0')}.ts`;f.reached(file);f.node(`candidate${i}`,`candidate${i}`,'function',file);}},{limit:1});
add('classes-only-and-limit',f=>{f.reached();f.node('a','Small','class');f.node('b','Large','class',undefined,{endLine:80});f.node('c','functionIgnored');},{kinds:['class','class','banana'],limit:1});
add('invalid-kinds-default',f=>{f.reached();f.node('a','candidate');},{kinds:['banana']});
add('explicit-variable-kind',f=>{f.reached();f.node('a','value','variable');},{kinds:['variable']});
writeFileSync(new URL('./dead-code.json',import.meta.url),JSON.stringify({revision:'6aa358ccbae30fbf27318f79ee183212f726f9ba',cases},null,2)+'\n');
