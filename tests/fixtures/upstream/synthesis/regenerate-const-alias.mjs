// Independent alias inputs and records from the pinned test suite and resolver.
import {createRequire} from 'node:module';
import {execFileSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdirSync,mkdtempSync,rmSync} from 'node:fs';
import {join,dirname,resolve} from 'node:path';
import {tmpdir} from 'node:os';
import {runInNewContext} from 'node:vm';
const root=resolve(process.argv[2]),revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('Wrong reference revision');
const require=createRequire(join(root,'package.json')),ts=require('typescript');
const {aliasedPaths,constAliasEdges}=require('./dist/resolution/const-alias-synthesizer.js');
const {isStatedDefaultExport,resolveImportPath}=require('./dist/resolution/import-resolver.js');
const {DatabaseConnection}=require('./dist/db/index.js'),{QueryBuilder}=require('./dist/db/queries.js');
const {ReferenceResolver}=require('./dist/resolution/index.js');
const {extractFromSource}=require('./dist/extraction/tree-sitter.js');
const {initGrammars,loadGrammarsForLanguages}=require('./dist/extraction/grammars.js');
await initGrammars();await loadGrammarsForLanguages(['typescript','javascript','tsx','jsx']);
const filename='const-alias-flow.test.ts',source=readFileSync(join(root,'__tests__',filename),'utf8');
const tree=ts.createSourceFile(filename,source,ts.ScriptTarget.Latest,true);
let global='';const inputs=[],paths=[],seen=new Set();
const transpile=code=>ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;
function visit(node){
 if(ts.isVariableDeclaration(node)&&node.name.getText(tree)==='PATHS')global=`const PATHS=${node.initializer.getText(tree)};`;
 if(ts.isCallExpression(node)&&node.expression.getText(tree)==='it.each'&&ts.isArrayLiteralExpression(node.arguments[0])){
  const rows=runInNewContext(transpile(`(${node.arguments[0].getText(tree)})`),{}, {timeout:1000});
  for(const row of rows){const source=Array.isArray(row)?row[0]:row;paths.push({source,expected:aliasedPaths(source)});}
 }
 if(ts.isCallExpression(node)&&node.expression.getText(tree)==='it') {
  const callback=node.arguments[1];if(!callback?.body?.statements)return;
  const label=node.arguments[0]?.text??String(inputs.length);let prefix='';let captured;
  for(const statement of callback.body.statements) {
   let call;const find=n=>{if(ts.isCallExpression(n)&&n.expression.getText(tree)==='indexProject')call??=n;ts.forEachChild(n,find);};find(statement);
   if(call){runInNewContext(transpile(`${global}\n${prefix}\ncapture(${call.arguments[0].getText(tree)});`),{capture:files=>captured=files},{timeout:1000});break;}
   prefix+=statement.getText(tree)+'\n';
  }
  if(captured){const key=JSON.stringify(captured);if(!seen.has(key)){seen.add(key);inputs.push({id:`const-alias-${inputs.length}`,origin:label,files:captured});}}
 }
 ts.forEachChild(node,visit);
}
visit(tree);
for(const source of ['a?.b ?? (x ? c.d! : e.f as F)','cond ? x : (y => y)','(a.b as jest.Mock) || c.d','((((((((((fn))))))))))','a.b /* 😀 */ ?? c.d'])paths.push({source,expected:aliasedPaths(source)});
const cases=[];
const language=path=>({ts:'typescript',mts:'typescript',cts:'typescript',tsx:'tsx',js:'javascript',mjs:'javascript',cjs:'javascript',jsx:'jsx'})[path.split('.').at(-1)];
for(const project of inputs){const temporary=mkdtempSync(join(tmpdir(),'prometeu-alias-oracle-'));const db=DatabaseConnection.initialize(join(temporary,'index.sqlite'));try{
 const queries=new QueryBuilder(db.getDb());
 for(const [path,source]of Object.entries(project.files)){mkdirSync(dirname(join(temporary,path)),{recursive:true});writeFileSync(join(temporary,path),source);const lang=language(path);const result=extractFromSource(path,source,lang);queries.insertNodes(result.nodes);queries.insertEdges(result.edges);queries.insertUnresolvedRefsBatch(result.unresolvedReferences.map(ref=>({...ref,filePath:path,language:lang})));queries.upsertFile({path,contentHash:'oracle',language:lang,size:Buffer.byteLength(source),indexedAt:0,modifiedAt:0,nodeCount:result.nodes.length});}
 const resolver=new ReferenceResolver(temporary,queries);resolver.initialize();await resolver.resolveAndPersistBatched(undefined,5000,undefined,undefined,false);
 const ctx=resolver.context,nodes=queries.getAllNodes(),baseEdges=nodes.flatMap(node=>queries.getOutgoingEdges(node.id)),imports=[];
 for(const file of Object.keys(project.files))for(const binding of ctx.getImportMappings(file,language(file)))imports.push({file,local_name:binding.localName,source:binding.source,imported_name:binding.importedName,is_default:binding.isDefault,is_namespace:binding.isNamespace,mapped_path:binding.resolvedPath??null,resolved_path:resolveImportPath(binding.source,file,language(file),ctx)??null,external:ctx.isOutOfRepoImport?.(binding.source,file,language(file))===true});
 const answers=[],original=ctx.resolveImport;
 ctx.resolveImport=ref=>{const hit=original(ref);const binding=ctx.getImportMappings(ref.filePath,ref.language).find(binding=>binding.localName===ref.referenceName.split('.')[0]);const target=hit?ctx.getNodeById(hit.targetNodeId):null;answers.push({reference:ref,target:target&&(!binding?.isDefault||isStatedDefaultExport(target,ctx))?target.id:null});return hit;};
 const expected=await constAliasEdges(queries,ctx,async()=>{});for(const node of nodes)delete node.updatedAt;
 cases.push({...project,nodes,baseEdges,imports,answers,expected});
}finally{db.close();rmSync(temporary,{recursive:true,force:true});}}
writeFileSync(new URL('./const-alias.json',import.meta.url),JSON.stringify({revision,paths,cases},null,2)+'\n');
console.log(paths.length,cases.length);
