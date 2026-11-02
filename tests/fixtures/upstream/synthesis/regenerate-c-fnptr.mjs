// Actual reference project indexing and direct function-pointer synthesis.
import {createRequire} from 'node:module';
import {execFileSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdirSync,mkdtempSync,rmSync} from 'node:fs';
import {join,dirname,resolve} from 'node:path';
import {tmpdir} from 'node:os';
import {runInNewContext} from 'node:vm';
const root=resolve(process.argv[2]),revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('Wrong reference revision');
const require=createRequire(join(root,'package.json')),ts=require('typescript');
const Project=Object.values(require('./dist/index.js')).find(value=>typeof value==='function'&&typeof value.init==='function'&&typeof value.open==='function');
const {cFnPointerDispatchEdges}=require('./dist/resolution/c-fnptr-synthesizer.js');
const transpile=code=>ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;
const projects=[];
for(const filename of ['c-fnptr-synthesizer.test.ts','c-fnptr-kernel-sweep.test.ts']){
 const source=readFileSync(join(root,'__tests__',filename),'utf8'),tree=ts.createSourceFile(filename,source,ts.ScriptTarget.Latest,true);
 function visit(node){
  if(filename.includes('kernel-sweep')&&ts.isVariableDeclaration(node)&&node.name.getText(tree)==='writeFixture'){
   const files={};runInNewContext(transpile(`(${node.initializer.getText(tree)})();`),{write:(path,source)=>files[path]=source},{timeout:2000});projects.push({id:'c-native-sweep',origin:filename,files});
  }
  if(!filename.includes('kernel-sweep')&&ts.isCallExpression(node)&&node.expression.getText(tree)==='it'){
   const body=node.arguments[1]?.body;if(!body?.statements)return;let prefix='';
   for(const statement of body.statements){let awaited=false;const find=n=>{if(ts.isAwaitExpression(n))awaited=true;ts.forEachChild(n,find);};find(statement);if(awaited)break;prefix+=statement.getText(tree)+'\n';}
   const files={};runInNewContext(transpile(prefix),{write:(path,source)=>files[path]=source},{timeout:2000});if(Object.keys(files).length)projects.push({id:`c-fnptr-${projects.length}`,origin:node.arguments[0]?.text,files});
  }ts.forEachChild(node,visit);
 }visit(tree);
}
projects.push({id:'c-fanout-cap',oracleOnly:true,files:{'many.c':`struct Ops { int (*run)(void); };\n${Array.from({length:305},(_,i)=>`int h${i}(void){return ${i};}`).join('\n')}\nstruct Ops handlers[] = {${Array.from({length:305},(_,i)=>`{h${i}}`).join(',')}};\nint dispatch(struct Ops* p){return p->run();}\n`}});
projects.push({id:'c-macro-expansion-limits',files:{'main.c':`struct Ops { int (*run)(void); };\nint good(void){return 1;}\nint deep(void){return 2;}\n#define A(x) B(x)\n#define B(x) C(x)\n#define C(x) D(x)\n#define D(x) E(x)\n#define E(x) F(x)\n#define F(x) G(x)\n#define G(x) x\n#define ONE(x) x\nint anchor(void){return 0;}\nstruct Ops ops[]={{ONE(good)},{A(deep)}};\nint dispatch(struct Ops *p){return p->run();}\n`}});
projects.push({id:'c-depth-two-include-environment',files:{'main.c':`#include "headers/a.h"\n#include "tables.def"\nstruct Ops { int (*run)(void); };\nint good(void){return 1;}\nint dispatch(struct Ops *p){return p->run();}\n`,'headers/a.h':'#include "b.tbl"\n','headers/b.tbl':'#define ENTRY(x) {x}\n','tables.def':'struct Ops table[] = {ENTRY(good)};\n'}});
const cases=[];
for(const input of projects){const temporary=mkdtempSync(join(tmpdir(),'prometeu-c-fnptr-oracle-'));let project;try{
 for(const [path,source]of Object.entries(input.files)){mkdirSync(dirname(join(temporary,path)),{recursive:true});writeFileSync(join(temporary,path),source);}
 project=await Project.init(temporary,{silent:true});const result=await project.indexAll();if(!result.success)throw Error('Index failed');
 const queries=project.queries,ctx=project.resolver.context,nodes=queries.getAllNodes(),baseEdges=nodes.flatMap(n=>queries.getOutgoingEdges(n.id)).filter(e=>!e.metadata?.synthesizedBy),files={},auxiliary={};
 for(const [path,source]of Object.entries(input.files))(ctx.getAllFiles().includes(path)?files:auxiliary)[path]=source;
 const expected=await cFnPointerDispatchEdges(queries,ctx,async()=>{});for(const node of nodes)delete node.updatedAt;
 cases.push({...input,files,auxiliary,nodes,baseEdges,expected});console.log(input.id,nodes.length,expected.length);
}finally{project?.close();rmSync(temporary,{recursive:true,force:true});}}
writeFileSync(new URL('./c-fnptr.json',import.meta.url),JSON.stringify({revision,cases},null,2)+'\n');
