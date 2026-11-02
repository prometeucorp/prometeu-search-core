// Final type-target decisions recorded from the original resolver's actual calls.
import {createRequire} from 'node:module';
import {execFileSync} from 'node:child_process';
import {readFileSync,writeFileSync} from 'node:fs';
import {join,resolve} from 'node:path';
import vm from 'node:vm';
const root=resolve(process.argv[2]),revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('Wrong reference revision');
const require=createRequire(join(root,'package.json')),ts=require('typescript'),gate=require('./dist/resolution/swift-type-visibility.js');
const Project=Object.values(require('./dist/index.js')).find(v=>typeof v==='function'&&typeof v.init==='function'&&typeof v.open==='function');
const clean=value=>JSON.parse(JSON.stringify(value,(key,value)=>key==='updatedAt'||key==='original'?undefined:value));
const simple=result=>result?{targetNodeId:result.targetNodeId,confidence:result.confidence}:null;
const observations=new WeakMap(),cases=[];let origin='';
const original=gate.gateSwiftTypeTarget;gate.gateSwiftTypeTarget=function(result,reference,ctx){const expected=original(result,reference,ctx);let recorded=observations.get(ctx);if(!recorded){recorded=[];observations.set(ctx,recorded)}if(reference.language==='swift')recorded.push(clean({reference,input:simple(result),expected:simple(expected)}));return expected};
const close=Project.prototype.close;Project.prototype.close=function(){if(origin&&this.queries&&this.resolver){const ctx=this.resolver.context,nodes=this.queries.getAllNodes(),sources=Object.fromEntries(ctx.getAllFiles().map(file=>[file,ctx.readFile(file)]));cases.push(clean({id:cases.length,origin,nodes,sources,answers:observations.get(ctx)??[],allEdges:nodes.flatMap(n=>this.queries.getOutgoingEdges(n.id))}));}return close.call(this)};
const noop=new Proxy(function(){return noop},{get:()=>noop,apply:()=>noop});let stack=[],tests=[],afters=[],befores=[];
const vitest={expect:noop,describe(name,fn){stack.push(name);fn();stack.pop()},it(name,fn){tests.push({name:[...stack,name].join(' / '),fn})},afterAll(fn){afters.push(fn)},beforeAll(fn){befores.push(fn)}};
for(const file of ['swift-type-visibility.test.ts','vapor-route-handlers.test.ts','swift-composition-continuation.test.ts']){tests=[];afters=[];befores=[];const code=ts.transpileModule(readFileSync(join(root,'__tests__',file),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;const localRequire=id=>id==='vitest'?vitest:id.startsWith('../src')?require('./dist'+id.slice('../src'.length)+(id==='../src'?'/index.js':'.js')):require(id);new vm.Script(code,{filename:file}).runInNewContext({require:localRequire,exports:{},console,process,Buffer});origin='__tests__/'+file;for(const before of befores)await before();for(const test of tests){origin=`__tests__/${file}: ${test.name}`;await test.fn();console.log(origin)}for(const after of afters)await after();}
for(const [id,source,column]of [
 ['unicode', 'let marker = "😀"; extension Model {}',18],
 ['long-unicode', 'let marker = "'+ 'a'.repeat(256)+'😀"; extension Model {}',274],
 ['attribute-string','@available(*, deprecated, message: "use class Model")\nextension Model {}',0],
 ['class-string','@available(*, deprecated, message: "use extension Model")\nclass Model {}',0],
 ['crlf','@available(*, deprecated, message: "😀")\r\nextension Model {}',0],
 ]){
 const sourceColumn=source.includes('; extension')?source.indexOf('extension'):column;
 const node=(id,name,kind,file,qn,extra={})=>({id,name,kind,filePath:file,qualifiedName:qn,language:'swift',startLine:1,endLine:source.split('\n').length,startColumn:0,endColumn:0,updatedAt:0,...extra});
 const target=node('extension:Model','Model','class','Types.swift','Model',{startColumn:sourceColumn});const declaration=node('declaration:Model','Model','struct','Model.swift','Model');const from=node('caller','run','function','Caller.swift','run');const nodes=[target,declaration,from],sources={'Types.swift':source,'Model.swift':'struct Model {}','Caller.swift':'let x: Model'};
 const ctx={getNodeById:id=>nodes.find(n=>n.id===id),getNodesByName:name=>nodes.filter(n=>n.name===name),getNodesInFile:file=>nodes.filter(n=>n.filePath===file),readFile:file=>sources[file]??null};
 const reference={fromNodeId:'caller',referenceName:'Model',referenceKind:'references',filePath:'Caller.swift',language:'swift',line:1,column:7};gate.gateSwiftTypeTarget({original:reference,targetNodeId:target.id,confidence:.85,resolvedBy:'framework'},reference,ctx);
 cases.push(clean({id:cases.length,origin:'supplemental Swift extension '+id,nodes,sources,answers:observations.get(ctx),allEdges:[]}));
}
writeFileSync(new URL('./swift-type-gate.json',import.meta.url),JSON.stringify({revision,cases},null,2)+'\n');console.log({contexts:cases.length,answers:cases.reduce((sum,c)=>sum+c.answers.length,0)});
