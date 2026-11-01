// Complete command/event records from the actual pinned project and synthesis pass.
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
const {tauriEdges}=require('./dist/resolution/tauri-synthesizer.js'),{resolveImportPath}=require('./dist/resolution/import-resolver.js');
const source=readFileSync(join(root,'__tests__/tauri-ipc.test.ts'),'utf8'),tree=ts.createSourceFile('tauri-ipc.test.ts',source,ts.ScriptTarget.Latest,true);
const transpile=code=>ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;
let globals='';const projects=[];
function constants(node){if(ts.isVariableDeclaration(node)&&['CORE','TYPED'].includes(node.name.getText(tree)))globals+=`const ${node.name.getText(tree)}=${node.initializer.getText(tree)};\n`;ts.forEachChild(node,constants);}constants(tree);
function visit(node){if(ts.isCallExpression(node)&&node.expression.getText(tree)==='it'){
 const body=node.arguments[1]?.body;if(!body?.statements)return;let prefix='';let captured;
 for(const statement of body.statements){let call;const find=n=>{if(ts.isCallExpression(n)&&n.expression.getText(tree)==='index')call??=n;ts.forEachChild(n,find);};find(statement);if(call){runInNewContext(transpile(`${globals}\n${prefix}\ncapture(${call.arguments[0].getText(tree)});`),{capture:files=>captured=files},{timeout:2000});break;}prefix+=statement.getText(tree)+'\n';}
 if(captured)projects.push({id:`tauri-${projects.length}`,origin:node.arguments[0]?.text.replace(/\b\w+_explore\b/g,'graph exploration'),files:captured});
}ts.forEachChild(node,visit);}visit(tree);
projects.push({id:'unicode-and-local-constants',origin:'UTF16 positions, comments, same-file constants, typed wrappers and literal holes',files:{
 'src-tauri/src/lib.rs':'#[tauri::command]\nfn save_file() {}\nfn run(){tauri::Builder::default().invoke_handler(tauri::generate_handler![save_file]).run();}\n',
 'src/unicode.ts':'import { invoke } from "@tauri-apps/api/core";\nconst LABEL = "😀"; const COMMAND = "save_file";\nexport function click(){ const prefix="界😀"; /* 😀界 */ return invoke(COMMAND); }\nexport function direct(){ /* 😀 */ return invoke<() => string>(`save_file`); }\nexport function dynamic(name:string){ return invoke(`save_${name}`); }\nexport function local(){const ONLY="save_file";return invoke(ONLY); }\nexport function outside(){return invoke(ONLY); }\n'
}});
projects.push({id:'command-variant-cap',origin:'Seven equally-scored attribute-generated command variants remain uncertain',files:Object.fromEntries([
 ['src-tauri/src/main.rs','use tauri;\nfn run(){tauri::generate_handler![save];}\n'],
 ['src/main.ts','import {invoke} from "@tauri-apps/api/core"; export function run(){invoke("save");}\n'],
 ...Array.from({length:7},(_,i)=>[`src-tauri/src/platform${i}.rs`,'#[tauri::command]\npub fn save() {}\n'])
])});
projects.push({id:'event-fanout-boundary',origin:'Sixteen emitting nodes are retained at the event limit',files:{
 'src-tauri/src/lib.rs':'use tauri::Emitter;\n'+Array.from({length:16},(_,i)=>`fn send${i}(app: tauri::AppHandle){ app.emit("ready",()); }\n`).join(''),
 'src/main.ts':'import {listen} from "@tauri-apps/api/event"; export function onReady() {}\nexport function setup(){listen("ready",onReady);}\n'
}});
const cases=[];
for(const input of projects){const temporary=mkdtempSync(join(tmpdir(),'prometeu-tauri-oracle-'));let project;try{
 for(const [path,source]of Object.entries(input.files)){mkdirSync(dirname(join(temporary,path)),{recursive:true});writeFileSync(join(temporary,path),source);}
 project=await Project.init(temporary,{silent:true});const result=await project.indexAll();if(!result.success)throw Error('Index failed');
 const queries=project.queries,ctx=project.resolver.context,nodes=queries.getAllNodes(),baseEdges=nodes.flatMap(n=>queries.getOutgoingEdges(n.id)).filter(e=>!e.metadata?.synthesizedBy),files={},assets={},imports=[],modules=[];
 for(const [path,source]of Object.entries(input.files))(ctx.getAllFiles().includes(path)?files:assets)[path]=source;
 for(const file of Object.keys(files)){const language=file.endsWith('.vue')?'vue':file.endsWith('.svelte')?'svelte':'typescript';if(file.endsWith('.rs'))continue;for(const binding of ctx.getImportMappings(file,language)){const resolved=resolveImportPath(binding.source,file,language,ctx);imports.push({file,local_name:binding.localName,source:binding.source,imported_name:binding.exportedName??'',is_default:binding.isDefault??false,is_namespace:binding.isNamespace??false,resolved_path:resolved,mapped_path:binding.resolvedPath??null,external:ctx.isOutOfRepoImport?.(binding.source,file,language)===true});modules.push({file,language,source:binding.source,resolved});}}
 const expected=await tauriEdges(queries,ctx,async()=>{});for(const node of nodes)delete node.updatedAt;
 cases.push({...input,files,assets,nodes,baseEdges,imports,modules,expected});console.log(input.id,nodes.length,expected.length);
}finally{project?.close();rmSync(temporary,{recursive:true,force:true});}}
writeFileSync(new URL('./tauri.json',import.meta.url),JSON.stringify({revision,cases},null,2)+'\n');
