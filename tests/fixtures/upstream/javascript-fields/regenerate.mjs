// Development-only oracle from the original resolver and its project regression inputs.
import {execFileSync} from 'node:child_process';
import {createRequire} from 'node:module';
import * as fs from 'node:fs';
import {dirname,join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {runInNewContext} from 'node:vm';
const root=resolve(process.argv[2]),revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('Wrong reference revision');
const require=createRequire(join(root,'package.json')),ts=require('typescript');
const actual=require('./dist/index.js'),Project=Object.values(actual).find(v=>typeof v==='function'&&typeof v.init==='function'&&typeof v.open==='function');
const {ReferenceResolver}=require('./dist/resolution/index.js');
const {extractFromSource}=require('./dist/extraction/tree-sitter.js');
const grammar=require('./dist/extraction/grammars.js');await grammar.initGrammars();await grammar.loadGrammarsForLanguages(['typescript','javascript','tsx','jsx']);
const code=fs.readFileSync(join(root,'dist/resolution/name-matcher.js'),'utf8'),module={exports:{}};
new Function('require','module','exports',code+'\nmodule.exports.helpers={inferLocalReceiverType,matchMethodCall,matchTsThisFieldCall,tsFieldOnLine,tsFieldElementType,tsCallEndsStatement,tsReturnsItsArgument,tsFieldDeclaration,tsHeadSupertypes,tsAliasValueName,tsClassTypeParameter,callReceiverKind};')(createRequire(join(root,'dist/resolution/name-matcher.js')),module,module.exports);
const helper=module.exports.helpers,projects=[],seen=new Set();let origin='';
const strip=node=>{const result={...node};delete result.updatedAt;return result;};
async function capture(graph,directory){
 const files={};function walk(dir){for(const entry of fs.readdirSync(dir,{withFileTypes:true})){if(entry.name.startsWith('.'))continue;const path=join(dir,entry.name);if(entry.isDirectory())walk(path);else if(/\.(?:[cm]?[jt]sx?|json)$/.test(path))files[path.slice(directory.length+1)]=fs.readFileSync(path,'utf8');}}walk(directory);
 const key=JSON.stringify(files);if(seen.has(key))return;seen.add(key);
 const nodes=graph.queries.getAllNodes().map(strip);if(!nodes.length)return;
 const owner=new ReferenceResolver(directory,graph.queries);owner.initialize();const context=owner.context;
 const importAnswers=[];const resolveImport=context.resolveImport?.bind(context);context.resolveImport=ref=>{const answer=resolveImport?.(ref);importAnswers.push({reference:ref,answer:answer?.targetNodeId??null});return answer;};
 const imports=[];for(const[path]of Object.entries(files)){const language=/\.[jt]sx$/.test(path)?path.endsWith('.tsx')?'tsx':'jsx':/\.m?js$/.test(path)?'javascript':path.endsWith('.ts')?'typescript':null;if(!language)continue;for(const entry of context.getImportMappings?.(path,language)??[])imports.push({file:path,...entry,external:context.isOutOfRepoImport?.(entry.source,path,language)===true});}
 const references=[];for(const[path,source]of Object.entries(files)){const language=path.endsWith('.tsx')?'tsx':path.endsWith('.jsx')?'jsx':/\.[cm]?js$/.test(path)?'javascript':path.endsWith('.ts')?'typescript':null;if(language){const result=extractFromSource(path,source,language);references.push(...result.unresolvedReferences.map(ref=>({...ref,filePath:path,language})));}}
 const fields=[];for(const reference of references){if(reference.referenceKind!=='calls'||!reference.referenceName.startsWith('this.'))continue;const end=reference.referenceName.lastIndexOf('.');if(end<=4)continue;const rawField=reference.referenceName.slice(5,end),element=rawField.endsWith('[]'),field=element?rawField.slice(0,-2):rawField,method=reference.referenceName.slice(end+1);const answer=helper.matchTsThisFieldCall(field,method,reference,context,element);fields.push({reference,field,method,element,expected:answer?{id:answer.targetNodeId,confidence:answer.confidence,resolvedBy:answer.resolvedBy}:null});}
 const locals=[];for(const reference of references){if(reference.referenceKind!=='calls')continue;const match=/^([\w.]+)\.([\w$]+)$/.exec(reference.referenceName);if(!match||match[1].startsWith('this.')||match[1]==='this'||match[1]==='super')continue;const declaration={};const inferred=helper.inferLocalReceiverType(match[1],reference,context,declaration),answer=helper.matchMethodCall(reference,context);locals.push({reference,receiver:match[1],method:match[2],inferred,declaration,expected:answer?{id:answer.targetNodeId,confidence:answer.confidence,resolvedBy:answer.resolvedBy}:null});}
 const declarations=[];for(const node of nodes.filter(n=>n.kind==='class'||n.kind==='component'))for(const field of new Set(fields.map(f=>f.field).filter(f=>!f.includes('.')))){declarations.push({node:node.id,field,expected:helper.tsFieldDeclaration(node,field,context)});}
 const headers=nodes.filter(n=>['class','interface','type_alias'].includes(n.kind)).map(node=>({node:node.id,supers:helper.tsHeadSupertypes(node,context),alias:node.kind==='type_alias'?helper.tsAliasValueName(node,context):null}));
 const returns=nodes.filter(n=>n.kind==='method').map(node=>({node:node.id,expected:helper.tsReturnsItsArgument(node,context)}));
 const {scanAmbientModules}=require('./dist/resolution/ambient-modules.js');const ambientBlocks=Object.entries(files).filter(([file])=>/\.d\.[cm]?ts$/i.test(file)).map(([file,source])=>({file,expected:scanAmbientModules(source).blocks.map(({specifier,startLine,startColumn,endLine,endColumn})=>({specifier,startLine,startColumn,endLine,endColumn}))}));
 const edges=graph.getOutgoingEdgesFrom(nodes.map(n=>n.id));const callEdges=edges.filter(e=>e.kind==='calls'&&fields.some(f=>f.reference.fromNodeId===e.source&&f.reference.line===e.line&&f.reference.column===e.column));
 const functionRefs=references.filter(reference=>reference.referenceKind==='function_ref'&&reference.referenceName.startsWith('this.')).map(reference=>({reference}));
 projects.push({functionRefs,id:origin+'-'+projects.length,files,nodes,edges,imports,fields,locals,declarations,headers,returns,ambientBlocks,importAnswers,callEdges});console.log(projects.at(-1).id,fields.length);
}
function attach(graph,directory){if(graph.__captured)return graph;graph.__captured=true;const index=graph.indexAll.bind(graph);graph.indexAll=async(...args)=>{const result=await index(...args);await capture(graph,directory);return result;};return graph;}
const proxy=new Proxy(Project,{get(target,key){if(key==='initSync')return(...args)=>attach(Project.initSync(...args),args[0]);if(key==='init')return async(...args)=>{const graph=attach(await Project.init(...args),args[0]);await capture(graph,args[0]);return graph;};return Reflect.get(target,key);}});
const assertProxy=new Proxy(()=>assertProxy,{get(_target,key){return key==='then'?undefined:assertProxy;},apply(){return assertProxy;}});
for(const file of ['ts-this-field-call.test.ts','ts-wrapped-field-initializers.test.ts','ts-injected-receivers.test.ts','ts-field-classification.test.ts','ts-array-annotated-receivers.test.ts','ts-chained-receiver.test.ts']){
 origin=file.replace('.test.ts','');const suite=()=>({children:[],before:[],after:[],beforeEach:[],afterEach:[]});const main=suite();let current=main;
 const describe=(_name,fn)=>{const parent=current,child=suite();parent.children.push(child);current=child;fn();current=parent;};describe.skip=()=>{};describe.each=cases=>(_name,fn)=>cases.forEach(value=>describe(_name,()=>fn(...(Array.isArray(value)?value:[value]))));
 const it=(_name,fn)=>current.children.push({fn});it.skip=()=>{};it.each=cases=>(_name,fn)=>cases.forEach(value=>it(_name,()=>fn(...(Array.isArray(value)?value:[value]))));
 const vitest={describe,it,test:it,expect:assertProxy,beforeAll:fn=>current.before.push(fn),afterAll:fn=>current.after.push(fn),beforeEach:fn=>current.beforeEach.push(fn),afterEach:fn=>current.afterEach.push(fn)};
 const localRequire=specifier=>{if(specifier==='vitest')return vitest;if(specifier.startsWith('../src')){let path=specifier.replace('../src','./dist');if(path==='./dist')path+='/index';const result=require(path);if(path.endsWith('/grammars'))return{...result,loadAllGrammars:async()=>{}};const mapped=Object.fromEntries(Object.entries(result).map(([name,value])=>[name,value===Project?proxy:value]));Object.defineProperty(mapped,"__esModule",{value:result.__esModule});return mapped;}return require(specifier);};
 const output=ts.transpileModule(fs.readFileSync(join(root,'__tests__',file),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022,esModuleInterop:true}}).outputText;
 runInNewContext(output,{require:localRequire,exports:{},module:{exports:{}},process,console,setTimeout,clearTimeout,Buffer,URL,URLSearchParams},{filename:file});
 async function run(suite,parents=[]){for(const fn of suite.before)await fn();for(const child of suite.children){if(child.fn){for(const group of [...parents,suite])for(const fn of group.beforeEach)await fn();try{await child.fn();}finally{for(const group of [...parents,suite].reverse())for(const fn of group.afterEach)await fn();}}else await run(child,[...parents,suite]);}for(const fn of suite.after)await fn();}await run(main);
}
const custom=[
 ['callback-import-alias', {'handler.ts': `export function work() {}`, 'app.ts': `import { work as callback } from './handler';
export function bind() { listen(callback); }`}],
 ['callback-class', {'handler.ts': `export class Handler {}`, 'app.ts': `import { Handler } from './handler';
export function bind() { listen(Handler); }`}],
 ['callback-local-shadow', {'app.ts': `function callback() {}
function bind(callback: unknown) { listen(callback); }
function outer() { const callback = 1; listen(callback); }`}],
 ['callback-lexical-shadow', {'app.ts': `function callback() {}
function bind() { function callback() {} listen(callback); }
function sibling() { listen(callback); }`}],
 ['callback-crossfile-ambiguous', {'one.ts': `export function callback() {}`, 'two.ts': `export function callback() {}`, 'app.ts': `export function bind() { listen(callback); }`}],
 ['ambient-alias', {'api.d.ts': `declare module 'remote' { interface Local { send(): void; } export { Local as Service }; }`, 'app.ts': `import { Service } from 'remote';
class App { service: Service; }`, 'package.json': `{"dependencies":{"remote":"1"}}`}],
 ['ambient-default', {'api.d.ts': `declare module 'remote' { export default class Service { send(): void; } }`, 'app.ts': `import Service from 'remote';
class App { service: Service; }`, 'package.json': `{"dependencies":{"remote":"1"}}`}],
 ['ambient-namespace', {'api.d.ts': `declare module 'remote' { export interface Service { send(): void; } }`, 'app.ts': `import * as api from 'remote';
class App { service: api.Service; }`, 'package.json': `{"dependencies":{"remote":"1"}}`}],
 ['ambient-augmentation', {'api.d.ts': `export {}; declare module 'remote' { export interface Service { send(): void; } }`, 'app.ts': `import { Service } from 'remote';
class App { service: Service; }`, 'package.json': `{"dependencies":{"remote":"1"}}`}],
 ['namespace-missing-type', {'types.ts': `export interface Existing {}`, 'app.ts': `import * as api from './types';
class Service {}
class App { service: api.Service; }`}],

 ['member-value-inherited',{'base.ts':`export class Base { handle() {} }`,'app.ts':`import { Base } from './base';\nclass Other { handle() {} }\nclass Child extends Base { bind() { listen(this.handle); } }`}],
 ['member-value-interface',{'app.ts':`interface Listener { handle(): void; }\nclass Child implements Listener { bind() { listen(this.handle); } }`}],
 ['member-value-data',{'app.ts':`class App { handler: () => void; bind() { listen(this.handler); } }\nclass Other { handler() {} }`}],
 ['member-value-self',{'app.ts':`class App { run() { listen(this.run); } }`}],
 ['member-value-object',{'app.ts':`class App { handle() {} run() { const view = { arrow: () => listen(this.handle), own() { listen(this.handle); } }; } }`}],
 ['typed-same-line-sibling',{'store.ts':`export class Store { save() {} }`,'other.ts':`export class Other { save() {} }`,'entry.ts':`import { Store } from './store'; function run(value: Store) { value.save(); } function separate(value) { value.save(); }`}],
 ['typed-same-line-nested',{'store.ts':`export class Store { save() {} }`,'other.ts':`export class Other { save() {} }`,'entry.ts':`import { Store } from './store'; function outer(value: Store) { function nested(value) { value.save(); } }`}],
 ['typed-name-single',{'entry.ts':`import { absent } from './missing'; class Empty {} function run(value: Empty) { value.save(); absent(); }`,'other.ts':`export function absent() {} class Other { save() {} }`}],
 ['typed-name-overlap',{'app.ts':`class Empty {}\nclass NetworkClient { send() {} }\nclass SqlQuery { send() {} }\nfunction run(httpClient: Empty) { httpClient.send(); }`}],
 ['typed-name-unrelated',{'app.ts':`class Empty {}\nclass First { save() {} }\nclass Second { save() {} }\nfunction run(value: Empty) { value.save(); }`}],
 ['typed-name-dispatched',{'app.ts':`class Empty {}\nclass ApiController { update() {} }\nclass Thing { update() {} }\nfunction run(value: Empty) { value.update(); }`}],
 ['typed-name-double',{'app.ts':`class Empty {}\nfunction run(response: Empty) { response.json(); }`,'double.ts':`class MockedResponse { json() {} }`}],
 ['typed-name-test-scope',{'app.ts':`class Empty {}\nfunction run(value: Empty) { value.save(); }`,'tests/fixture.ts':`class TestStore { save() {} }`}],
 ['typed-name-lexical',{'app.ts':`class Empty {}\nfunction setup() {\n class Local { save() {} }\n}\nclass RemoteRepo { save() {} }\nfunction run(repo: Empty) { repo.save(); }`}],
 ['typed-name-prototype',{'app.ts':`class Empty {}\nLegacy.prototype.save = function() {};\nfunction run(value: Empty) { value.save(); }`,'other.ts':`class Service { save() {} }`}],
 ['typed-name-react',{'app.ts':`class Empty {}\nclass View extends React.Component { render() {} }\nfunction run(value: Empty) { value.render(); }`}],
 ['alias-chain',{'types.ts':`export class Bridge { send(): void {} }
export type Backend<A = string> = Bridge | null;
export type Twice = Backend<string>;`,'app.ts':`import { Twice } from './types';
class App { backend: Twice; run() { this.backend.send(); } }`}],
 ['generic-alias',{'app.ts':`class Item { send() {} }
type Box<Item> = Item | null;
class App { box: Box<Item>; run() { this.box.send(); } }`}],
 ['interface-chain',{'types.ts':`export interface Parent { send(): void; }
export interface Middle extends Parent {}
export interface Child extends Middle {}`,'app.ts':`import { Child as Service } from './types';
class App { service: Service; run() { this.service.send(); } }`}],
 ['known-missing',{'own.ts':`class Service { own() {} }
class App { service: Service; run() { this.service.send(); } }`,'other.ts':`class Service { send() {} }`}],
 ['external',{'package.json':JSON.stringify({dependencies:{remote:'1'}}),'app.ts':`import { Service } from 'remote';
class App { service: Service; run() { this.service.send(); } }`,'other.ts':`class Service { send() {} }`}],
 ['ambient',{'package.json':JSON.stringify({dependencies:{remote:'1'}}),'api.d.ts':`declare module 'remote' { export interface Service { send(): void; } }`,'app.ts':`import { Service } from 'remote';
class App { service: Service; run() { this.service.send(); } }`}],
 ['merged-ambient',{'package.json':JSON.stringify({dependencies:{remote:'1'}}),'api.d.ts':`declare module 'remote' { export interface Service { first(): void; } }`,'extra.d.ts':`declare module 'remote' { export interface Service { send(): void; } }`,'app.ts':`import { Service } from 'remote';
class App { service: Service; run() { this.service.send(); } }`}],
 ['base-import',{'service.ts':`export class Service { send() {} }`,'base.ts':`import { Service } from './service';
export class Base { service: Service; }`,'app.ts':`import { Base } from './base';
class Service { send() {} }
class App extends Base { run() { this.service.send(); } }`}],
 ['object-scopes',{'app.ts':`class Service { send() {} }
class App { service: Service; run() { const view = { arrow: () => this.service.send(), own() { this.service.send(); }, nested: { run: () => this.service.send() } }; return view; } }`}],
 ['value-alias',{'app.ts':`function send() {}
const API = { send, direct() {}, nested: { direct() {} } };
class App { api: typeof API; run() { this.api.send(); this.api.direct(); } }`}],
 ['namespace-type',{'types.ts':`export class Service { send() {} }`,'app.ts':`import * as api from './types';
class App { service: api.Service; run() { this.service.send(); } }`}],
 ['unicode-crlf',{'app.ts':`// 😀 before declarations\r\nclass Service { send() {} }\r\nclass App { service: Service; run() { const text = '😀'; this.service.send(); } }\r\n`}],
 ['constructor-property',{'app.ts':`class Service { constructor() {} }
class Holder { service = new Service(); }`}],
 ['constructor-arrow',{'app.ts':`class Service { constructor() {} }
class Factory { create = () => new Service(); nested() { return (() => new Service())(); } }`}],
 ['constructor-namespace',{'store.ts':`export class Store { constructor() {} }`,'app.ts':`import * as ns from './store';
export function make() { return new ns.Store(); }`}],
 ['constructor-same-line',{'app.ts':`class Store { constructor() {} }
function make() { Store(); new Store(); new Store(); }`}],
 ['constructor-unicode',{'app.ts':`class Store { constructor() {} }
function make() { const label = '😀'; Store(); new Store(); new Store(); }`}],
 ['constructor-shadowed',{'store.ts':`export class Store { constructor() {} }`,'app.ts':`import { Store } from './store';
export function make(Store: any) { return new Store(); }
export function own() { return new Store(); }`}],
 ['namespace-local-reexport-receivers',{
  'model.ts':`export class Store { save() {} } export interface Writer { write(): void }
             class Hidden { hide() {} }`,
  'barrel.ts':`export * as model from './model';`,
  'alias.ts':`export { model as storage } from './barrel';`,
  'entry.ts':`import * as model from './model'; import { storage } from './alias';
             function direct(value: model.Store) { value.save(); }
             function signature(value: model.Writer) { value.write(); }
             function reexported(value: storage.Store) { value.save(); }
             function privateType(value: model.Hidden) { value.hide(); }
             function missingType(value: model.Missing) { value.save(); }`,
  'other.ts':`export class Store { save() {} } export class Hidden { hide() {} }`,
 }],
];
for(const [name,files] of custom){origin=name;const directory=fs.mkdtempSync(join(require('node:os').tmpdir(),'prometeu-js-fields-'));let project;try{for(const[path,source]of Object.entries(files)){fs.mkdirSync(dirname(join(directory,path)),{recursive:true});fs.writeFileSync(join(directory,path),source);}project=await Project.init(directory,{index:true});await capture(project,directory);}finally{project?.close();fs.rmSync(directory,{recursive:true,force:true});}}
const {scanAmbientModules}=require('./dist/resolution/ambient-modules.js');
const ambientInputs=[
 "declare module 'pkg' { interface Service { send(): void; } }",
 "// declare module 'fake' { }\ndeclare module 'real' { }",
 `const fake = "declare module 'fake' {}"; declare module 'real' {}`,
 "declare /* comment */ module 'real' { interface X {} }",
 "declare module 'p\\kg' { interface X {} }",
 "const braces = /[{}]/g; declare module 'real' { interface X {} }",
 "const division = value / 2; declare module 'real' { interface X {} }",
 '`declare module "fake" {} ${1}`; declare module "real" {}',
 '`text ${ {a: 1} } more ${2}`; declare module "real" {}',
 'namespace Outer { declare module "nested" {} } declare module "root" {}',
 'declare module "" {} declare module "short"; declare module "open" {',
 "declare module '*.svg' { export default value; }",
 '// 😀\r\ndeclare module "first" {}\r\ndeclare module "second" {}',
 'export {}; declare module "pkg" { export { Service as Other }; }',
 'declare module "pkg" { type Text = `text ${string}`; }',
 'const pattern = /declare module "fake" {[}]/; declare module "real" {}',
 'declare module "a" {} declare module "b" {}',
 '/* unclosed declare module "fake" {}',
].map(source=>({source,expected:scanAmbientModules(source).blocks.map(({specifier,startLine,startColumn,endLine,endColumn})=>({specifier,startLine,startColumn,endLine,endColumn}))}));
const destination=dirname(fileURLToPath(import.meta.url));fs.writeFileSync(join(destination,'cases.json'),JSON.stringify({revision,projects,ambientInputs},null,2)+'\n');fs.writeFileSync(join(destination,'LICENSE'),fs.readFileSync(join(root,'LICENSE')));
