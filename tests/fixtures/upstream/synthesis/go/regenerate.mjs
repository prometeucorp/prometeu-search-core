import {execFileSync} from 'node:child_process';
import {createRequire} from 'node:module';
import {readFileSync,writeFileSync} from 'node:fs';
import {join,resolve} from 'node:path';
const root=resolve(process.argv[2]);
const revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('Wrong reference revision');
const require=createRequire(join(root,'package.json')),ts=require('typescript');
const source=readFileSync(join(root,'src/resolution/go-stdlib-methods.ts'),'utf8');
const tree=ts.createSourceFile('stdlib.ts',source,ts.ScriptTarget.Latest,true);
function literal(node){if(ts.isStringLiteral(node))return node.text;if(ts.isObjectLiteralExpression(node))return Object.fromEntries(node.properties.map(p=>[p.name.text,literal(p.initializer)]));throw Error('Expected static table');}
for(const statement of tree.statements)if(ts.isVariableStatement(statement))for(const declaration of statement.declarationList.declarations)if(declaration.name.getText(tree)==='GO_STDLIB_METHODS')writeFileSync('src/synthesis/go_stdlib.json',JSON.stringify(literal(declaration.initializer),null,2)+'\n');
const {mkdirSync,mkdtempSync,rmSync,readdirSync}=await import('node:fs');
const {tmpdir}=await import('node:os');
const {dirname}=await import('node:path');
const {fileURLToPath}=await import('node:url');
const {DatabaseConnection}=require('./dist/db/index.js');
const {QueryBuilder}=require('./dist/db/queries.js');
const {ReferenceResolver}=require('./dist/resolution/index.js');
const {synthesizeCallbackEdges,goSignatureArity}=require('./dist/resolution/callback-synthesizer.js');
const {initGrammars,loadGrammarsForLanguages}=require('./dist/extraction/grammars.js');
const {extractFromSource}=require('./dist/extraction/tree-sitter.js');
await initGrammars();await loadGrammarsForLanguages(['go']);
const projects=[],seen=new Set();
function add(files,origin){if(!Object.keys(files).some(p=>p.endsWith('.go')))return;const key=JSON.stringify(files);if(seen.has(key))return;seen.add(key);projects.push({id:`go-${projects.length}`,origin,files});}
for(const filename of readdirSync(join(root,'__tests__')).filter(n=>/^go-(?:implements|interface-embedding|embedding-no-override).*\.test\.ts$/.test(n))){
 const tree=ts.createSourceFile(filename,readFileSync(join(root,'__tests__',filename),'utf8'),ts.ScriptTarget.Latest,true);
 function string(node){return node&&(ts.isStringLiteral(node)||ts.isNoSubstitutionTemplateLiteral(node))?node.text:undefined;}
 function visit(node){
  if(ts.isObjectLiteralExpression(node)){
   const entries=node.properties.filter(ts.isPropertyAssignment).map(p=>[string(p.name),string(p.initializer)]);
   if(entries.length&&entries.every(([path])=>path&&/\.(?:go|mod)$/.test(path)))add(Object.fromEntries(entries.filter(([,source])=>source!==undefined)),filename);
  }
  if(ts.isCallExpression(node)&&['it','test'].includes(node.expression.getText(tree))&&node.arguments[1]?.body){
   const files={};function writes(n){if(ts.isCallExpression(n)&&n.expression.getText(tree)==='fs.writeFileSync'){
    const path=n.arguments[0],source=string(n.arguments[1]);if(ts.isCallExpression(path)&&source!==undefined){const parts=path.arguments.slice(1).map(string);if(parts.length&&parts.every(p=>p!==undefined))files[parts.join('/')]=source;}
   }ts.forEachChild(n,writes);}writes(node.arguments[1].body);add(files,filename+':'+string(node.arguments[0]));
  }
  ts.forEachChild(node,visit);
 }visit(tree);
}
add({'cap.go':'package cap\ntype Worker interface { Run() }\n'+Array.from({length:45},(_,i)=>`type Worker${i} struct {}\nfunc (w Worker${i}) Run() {}\n`).join('')},'candidate cap');
add({'unicode.go':'package unicode\ntype Contract interface { Écrire() }\ntype Worker struct {}\nfunc (w Worker) Écrire() {}\n'},'unicode exported method');
const cases=[];
for(const project of projects){
 const directory=mkdtempSync(join(tmpdir(),'prometeu-go-'));const connection=DatabaseConnection.initialize(join(directory,'reference.sqlite'));
 try{
  const queries=new QueryBuilder(connection.getDb());
  for(const [path,source]of Object.entries(project.files)){mkdirSync(dirname(join(directory,path)),{recursive:true});writeFileSync(join(directory,path),source);if(!path.endsWith('.go'))continue;
   const result=extractFromSource(path,source,'go',[]);queries.insertNodes(result.nodes);queries.insertEdges(result.edges);queries.insertUnresolvedRefsBatch(result.unresolvedReferences.map(r=>({...r,filePath:path,language:'go'})));queries.upsertFile({path,contentHash:'reference',language:'go',size:Buffer.byteLength(source),indexedAt:0,modifiedAt:0,nodeCount:result.nodes.length});
  }
  const resolver=new ReferenceResolver(directory,queries);await resolver.resolveAndPersistBatched(undefined,5000,undefined,undefined,false);await resolver.resolveChainedCallsViaConformance();await resolver.resolveDeferredThisMemberRefs();
  const nodes=queries.getAllNodes();const baseEdges=nodes.flatMap(n=>queries.getOutgoingEdges(n.id));const unresolved=queries.getUnresolvedReferencesFromNodes(nodes.map(n=>n.id),['extends']);
  await synthesizeCallbackEdges(queries,resolver.context);
  const finalEdges=nodes.flatMap(n=>queries.getOutgoingEdges(n.id));
  const contains=finalEdges.filter(e=>e.metadata?.synthesizedBy==='go-method-contains');
  const expected=finalEdges.filter(e=>e.metadata?.synthesizedBy==='go-implements');
  for(const node of nodes)delete node.updatedAt;
  cases.push({...project,nodes,rawBaseEdges:baseEdges,baseEdges:[...baseEdges,...contains],unresolved,expected,synthesized:finalEdges.filter(e=>e.metadata?.synthesizedBy),arities:nodes.filter(n=>n.kind==='method'||n.kind==='function').map(n=>({signature:n.signature??null,arity:goSignatureArity(n.signature)}))});
  console.log(`${project.id}: ${nodes.length} nodes, ${expected.length} conformance edges`);
 }finally{connection.close();rmSync(directory,{recursive:true,force:true});}
}
const destination=dirname(fileURLToPath(import.meta.url));writeFileSync(join(destination,'cases.json'),JSON.stringify({revision,cases},null,2)+'\n');writeFileSync(join(destination,'LICENSE'),readFileSync(join(root,'LICENSE')));
