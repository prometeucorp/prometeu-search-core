// Independent answers from the pinned source tests and actual indexed projects.
import {createRequire} from 'node:module';
import {execFileSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdirSync,mkdtempSync,rmSync} from 'node:fs';
import {join,resolve,dirname} from 'node:path';
import vm from 'node:vm';
const root=resolve(process.argv[2]),revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('Wrong reference revision');
const require=createRequire(join(root,'package.json')),ts=require('typescript');
const cases=[],extracts=[],projects=[],exactTypes=[];let origin='';
const importer=require('./dist/resolution/import-resolver.js'),names=require('./dist/resolution/name-matcher.js');
const modules=['swift','swift-objc','react-native','expo-modules'].flatMap(file=>Object.values(require(`./dist/resolution/frameworks/${file}.js`))).filter(value=>value&&typeof value==='object'&&typeof value.name==='string'&&['swiftui','uikit','swift-objc-bridge','react-native-bridge','expo-modules-js'].includes(value.name));
const clean=value=>JSON.parse(JSON.stringify(value, (key,value)=>key==='updatedAt'||key==='original'?undefined:value));
function capture(resolver,original,reference,ctx){
 const moduleAnswers=[],resolvePath=importer.resolveImportPath;
 importer.resolveImportPath=(specifier,from,language,context)=>{const answer=resolvePath(specifier,from,language,context);moduleAnswers.push({specifier,from,language,answer});return answer};
 let answer;try{answer=original.call(resolver,reference,ctx)}finally{importer.resolveImportPath=resolvePath}
 if(!origin)return answer;
 const files=ctx.getAllFiles(),nodes=[],ids=new Set();for(const node of [...ctx.getNodesByKind('method'),...files.flatMap(file=>ctx.getNodesInFile(file))])if(!ids.has(node.id)){ids.add(node.id);nodes.push(node)}
 const sources=Object.fromEntries(files.map(file=>[file,ctx.readFile(file)??''])),imports=[];
 for(const file of files){const language=nodes.find(n=>n.filePath===file)?.language??'typescript';for(const m of ctx.getImportMappings(file,language))imports.push({file,local_name:m.localName,source:m.source,imported_name:m.exportedName??'',is_namespace:m.isNamespace??false,is_default:m.isDefault??false,external:false,resolved_path:null});}
 for(const file of files){const language=/\.tsx$/.test(file)?'tsx':/\.[cm]?ts$/.test(file)?'typescript':/\.jsx$/.test(file)?'jsx':'javascript';for(const m of ctx.getImportMappings(file,language))if(m.source.startsWith('.'))moduleAnswers.push({specifier:m.source,from:file,language,answer:resolvePath(m.source,file,language,ctx)});}
 cases.push(clean({id:`mobile-${cases.length}`,origin,framework:resolver.name,nodes,sources,references:[reference],expected:[answer?{targetNodeId:answer.targetNodeId,confidence:answer.confidence,...(answer.alsoTargets?{alsoTargets:answer.alsoTargets}:{}),...(answer.metadata?{metadata:answer.metadata}:{})}:null],imports,moduleAnswers}));return answer;
}
for(const resolver of modules){const original=resolver.resolve;resolver.resolve=function(reference,ctx){return capture(this,original,reference,ctx)};if(resolver.extract){const original=resolver.extract;resolver.extract=function(file,source){const answer=original.call(this,file,source);if(origin)extracts.push(clean({origin,file,source,expected:answer}));return answer}}}

const Project=Object.values(require('./dist/index.js')).find(v=>typeof v==='function'&&typeof v.init==='function'&&typeof v.open==='function');
const close=Project.prototype.close;
Project.prototype.close=function(){if(origin&&this.queries&&this.resolver){
 const ctx=this.resolver.context,nodes=this.queries.getAllNodes(),sources=Object.fromEntries(ctx.getAllFiles().map(file=>[file,ctx.readFile(file)])),allEdges=nodes.flatMap(n=>this.queries.getOutgoingEdges(n.id)),edges=allEdges.filter(e=>modules.some(m=>m.name===e.metadata?.framework));const assets={};for(const file of ['package.json']){const content=ctx.readFile(file);if(content&&!Object.hasOwn(sources,file))assets[file]=content}projects.push(clean({origin,nodes,sources,assets,edges,allEdges}));
 const byId=new Map(nodes.map(n=>[n.id,n]));const refs=allEdges.filter(e=>e.metadata?.refName).map(e=>({fromNodeId:e.source,referenceName:e.metadata.refName,referenceKind:e.metadata.refKind??e.kind,line:e.line??byId.get(e.source).startLine,column:e.column??0,filePath:byId.get(e.source).filePath,language:byId.get(e.source).language}));refs.push(...Object.keys(sources).flatMap(file=>this.queries.getUnresolvedReferencesInFile(file,100000)));
 for(const ref of refs)for(const resolver of modules)if(resolver.languages.includes(ref.language))resolver.resolve(ref,ctx);
 for(const reference of refs){if(reference.language!=='swift'||!/^\w+$/.test(reference.referenceName))continue;const candidates=ctx.getNodesByName(reference.referenceName).filter(n=>n.kind!=='import');if(!candidates.length||candidates.some(n=>n.language!=='swift'||!['class','struct','interface','enum','trait','protocol','record'].includes(n.kind)))continue;const exact=names.matchByExactName(reference,ctx);const expected=exact&&exact.confidence===.9&&names.isVisibleAcrossFiles(byId.get(exact.targetNodeId),reference,ctx)?{targetNodeId:exact.targetNodeId,confidence:.9}:null;exactTypes.push(clean({origin,nodes,sources,reference,expected}));}
 for(const [file,source]of Object.entries(sources))if(file.endsWith('.m')||file.endsWith('.mm'))modules.find(m=>m.name==='react-native-bridge').extract(file,source);
}return close.call(this)};

const noop=new Proxy(function(){return noop},{get:()=>noop,apply:()=>noop});let stack=[],tests=[];
const vitest={expect:noop,describe(name,fn){stack.push({name,before:[],after:[]});fn();stack.pop()},it(name,fn){tests.push({name:[...stack.map(s=>s.name),name].join(' / '),fn,before:stack.flatMap(s=>s.before),after:stack.toReversed().flatMap(s=>s.after)})},beforeEach(fn){stack.at(-1).before.push(fn)},afterEach(fn){stack.at(-1).after.push(fn)}};
for(const file of ['swift-objc-bridge-resolver.test.ts','react-native-bridge.test.ts','expo-modules.test.ts']){
 tests=[];const code=ts.transpileModule(readFileSync(join(root,'__tests__',file),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
 const localRequire=id=>id==='vitest'?vitest:id.startsWith('../src')?require('./dist'+id.slice('../src'.length)+(id==='../src'?'/index.js':'.js')):require(id);
 new vm.Script(code,{filename:file}).runInNewContext({require:localRequire,exports:{},console,process,Buffer});
 for(const test of tests){origin=`__tests__/${file}: ${test.name}`;try{for(const before of test.before)await before();await test.fn();}finally{for(const after of test.after)await after()}console.log(origin)}
}
const extraProjects=[
 {id:'swift-ui',files:{'App.swift':'import SwiftUI\nstruct AppRoot: View { var body: some View { ContentView() } }\n','Views/ContentView.swift':'import SwiftUI\nstruct ContentView: View { var body: some View { Text("hello") } }\n'}},
 {id:'swift-objc',files:{'Cache.m':'@implementation Cache\n- (void)fetchEntryForKey:(NSString *)key {}\n@end\n','Caller.swift':'class Caller { func load(cache: Cache) { cache.fetchEntry(forKey: "x") } }\n'}},
 {id:'swift-extern',files:{'Capture.swift':'import Foundation\n@objc(CaptureView)\nclass CaptureView: NSObject {\n @objc func finalizeCaptureSession() {}\n}\n','Capture.m':'@interface RCT_EXTERN_MODULE(CaptureView, NSObject)\nRCT_EXTERN_METHOD(finalizeCaptureSession)\n@end\n','index.ts':"import {NativeModules} from 'react-native';\nconst captureView = NativeModules.CaptureView;\nexport function save(){captureView.finalizeCaptureSession();}\n"}}
];
for(const [variant,extra]of [
 ['same-file',{'App.swift':'import SwiftUI\nstruct AppRoot: View { var body: some View { ContentView() } }\nstruct ContentView: View { var body: some View { Text("own") } }\n'}],
 ['private',{'Private.swift':'private struct ContentView {}\n'}],
 ['tests',{'Tests/UITests/ContentView.swift':'struct ContentView {}\n'}],
 ['local',{'Helper.swift':'func helper(){\n struct ContentView {}\n}\n'}],
 ['nested',{'Helper.swift':'struct Holder { struct ContentView {} }\n'}]
])extraProjects.push({id:'swift-ui-'+variant,files:{...extraProjects[0].files,...extra}});
for(const input of extraProjects){origin='supplemental indexed '+input.id;const temporary=mkdtempSync('/tmp/prometeu-mobile-');let project;try{for(const [file,source]of Object.entries(input.files)){mkdirSync(dirname(join(temporary,file)),{recursive:true});writeFileSync(join(temporary,file),source)}project=await Project.init(temporary,{silent:true});const result=await project.indexAll();if(!result.success)throw Error('Index failed')}finally{project?.close();rmSync(temporary,{recursive:true,force:true})}}
// Additional scope cases exercise the reference's shared name-pick guards.
const node=(name,file,kind='struct',extra={})=>({id:`${file}:${name}:${extra.startLine??1}`,name,kind,filePath:file,qualifiedName:`${file}::${name}`,language:'swift',startLine:1,endLine:5,startColumn:0,endColumn:0,updatedAt:0,...extra});
function context(nodes,sources={}){const files=[...new Set([...nodes.map(n=>n.filePath),...Object.keys(sources)])];return{getNodeById:id=>nodes.find(n=>n.id===id),getAllFiles:()=>files,getNodesInFile:file=>nodes.filter(n=>n.filePath===file),getNodesByName:name=>nodes.filter(n=>n.name===name),getNodesByQualifiedName:name=>nodes.filter(n=>n.qualifiedName===name),getNodesByKind:kind=>nodes.filter(n=>n.kind===kind),readFile:file=>sources[file]??'',getImportMappings:()=>[],fileExists:file=>files.includes(file),getProjectRoot:()=>'/fixture',getNodesByLowerName:name=>nodes.filter(n=>n.name.toLowerCase()===name)}}
for(const [framework,name,kind]of [['swiftui','ContentView','struct'],['swiftui','AccountViewModel','class'],['swiftui','AccountStore','class'],['swiftui','AccountManager','class'],['swiftui','Person','struct'],['uikit','AccountViewController','class'],['uikit','AccountView','class'],['uikit','AccountCell','class'],['uikit','AccountDelegate','protocol'],['uikit','AccountDataSource','protocol']]){
 const resolver=modules.find(m=>m.name===framework),reference={fromNodeId:'src/Main.swift:run:1',referenceName:name,referenceKind:'uses',filePath:'src/Main.swift',language:'swift',line:20,column:0};
 const ordinary=node(name,'other/Types.swift',kind),own=node(name,'src/Main.swift',kind),near=node(name,'src/Near.swift',kind),preferred=node(name,'app/Views/Types.swift',kind);
 const parent=node('Holder','other/Types.swift','class',{qualifiedName:'other/Types.swift::Holder',endLine:100});const nested=node(name,'other/Types.swift',kind,{qualifiedName:parent.qualifiedName+'::'+name});const fn=node('make','other/Types.swift','function',{endLine:100});const local=node(name,'other/Types.swift',kind,{qualifiedName:fn.qualifiedName+'::'+name});
 for(const [variant,nodes,ref]of [['plain',[ordinary],reference],['same-file',[ordinary,own],reference],['same-dir',[ordinary,preferred,near],reference],['preferred',[ordinary,preferred],reference],['private',[{...ordinary,visibility:'private'}],reference],['test',[node(name,'Tests/FeatureTests.swift',kind)],reference],['nested',[parent,nested],reference],['local',[fn,local],reference],['inside-local',[fn,local],{...reference,filePath:'other/Types.swift'}],['self-inheritance',[own],{...reference,fromNodeId:own.id,referenceKind:'extends'}],['native-language',[ordinary],{...reference,language:'objc'}]]){origin=`supplemental Swift scope ${framework}/${name}/${variant}`;const ctx=context(nodes);resolver.resolve(ref,ctx);const exact=names.matchByExactName(ref,ctx);const winner=exact&&exact.confidence===.9&&names.isVisibleAcrossFiles(nodes.find(n=>n.id===exact.targetNodeId),ref,ctx)?{targetNodeId:exact.targetNodeId,confidence:.9}:null;if(ref.language==='swift')exactTypes.push(clean({origin,nodes,sources:Object.fromEntries(ctx.getAllFiles().map(file=>[file,ctx.readFile(file)])),reference:ref,expected:winner}));}
}
for(const [file,kind]of [['Native.m','struct'],['Native.h','struct'],['Native.mm','class'],['Native.h','class']]){origin=`supplemental Swift native type visibility ${file}/${kind}`;const candidate=node('ContentView',file,kind,{language:'objc'});modules.find(m=>m.name==='swiftui').resolve({fromNodeId:'caller',filePath:'App.swift',language:'swift',referenceName:'ContentView',referenceKind:'calls',line:1,column:0},context([candidate],{'App.swift':'ContentView()'}))}
for(const [file,source]of [
 ['ios/RCTStore.m','@implementation RCTStore\nRCT_EXPORT_MODULE()\nRCT_EXPORT_METHOD(load:(NSString *)key)\nRCT_REMAP_METHOD(save, persist:(NSString *)key)\n@end'],
 ['ios/Store.mm','// 😀 unicode\r\n@implementation Store\r\nRCT_EXPORT_MODULE(Renamed)\r\nRCT_REMAP_METHOD(load, first:(id)value)\r\nRCT_EXPORT_METHOD(load:(id)value)\r\nRCT_EXPORT_METHOD(load:(id)value)\r\n@end'],
 ['ios/NoClass.m','RCT_EXPORT_MODULE(Loose)\nRCT_EXPORT_METHOD(run)'],
 ['ios/Empty.m','RCT_EXPORT_MODULE()\nRCT_EXPORT_METHOD(run)'],
 ['ios/Comments.m','@implementation RCTThing\n// RCT_EXPORT_MODULE()\n// RCT_EXPORT_METHOD(commented)\nRCT_EXPORT_METHOD(real)'],
 ['ios/RCT.m','@implementation RCT (Extension)\nRCT_EXPORT_MODULE()\nRCT_EXPORT_METHOD(go)'],
 ['ios/SwiftShim.m','@interface RCT_EXTERN_MODULE(Foo, NSObject)\nRCT_EXTERN_METHOD(run)\n@end'],
 ['ignored.swift','RCT_EXPORT_MODULE(Loose)\nRCT_EXPORT_METHOD(run)']
]){origin='supplemental native export '+file;modules.find(m=>m.name==='react-native-bridge').extract(file,source)}
origin='';writeFileSync(new URL('./mobile.json',import.meta.url),JSON.stringify({revision,cases,extracts,projects,exactTypes},null,2)+'\n');console.log({cases:cases.length,extracts:extracts.length,projects:projects.length});
