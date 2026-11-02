// Development-only capture of generic and Scala scope decisions from the pinned resolver.
import {createRequire} from 'node:module';
import {execFileSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdtempSync,mkdirSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join,resolve,dirname} from 'node:path';
import vm from 'node:vm';
const root=resolve(process.argv[2]),revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('Wrong reference revision');
const require=createRequire(join(root,'package.json')),ts=require('typescript'),gate=require('./dist/resolution/type-parameters.js');
const Project=Object.values(require('./dist/index.js')).find(v=>typeof v==='function'&&typeof v.init==='function'&&typeof v.open==='function');
const clean=value=>JSON.parse(JSON.stringify(value,(key,value)=>key==='updatedAt'||key==='original'?undefined:value));
const simple=result=>result?{targetNodeId:result.targetNodeId,confidence:result.confidence}:null;
const module={exports:{}};
new Function('require','module','exports',readFileSync(join(root,'dist/resolution/type-parameters.js'),'utf8')+'\nmodule.exports.readers={declaredTypeParameters,declaredValueParameters};')(createRequire(join(root,'dist/resolution/type-parameters.js')),module,module.exports);
const readers=module.exports.readers;
const observations=new WeakMap(),cases=[];let origin='';
const original=gate.gateTypeParameter;gate.gateTypeParameter=function(result,reference,ctx){const expected=original(result,reference,ctx);let recorded=observations.get(ctx);if(!recorded){recorded=[];observations.set(ctx,recorded)}recorded.push(clean({reference,input:simple(result),expected:simple(expected)}));return expected};
const close=Project.prototype.close;Project.prototype.close=function(){if(origin&&this.queries&&this.resolver){const ctx=this.resolver.context,nodes=this.queries.getAllNodes(),sources=Object.fromEntries(ctx.getAllFiles().map(file=>[file,ctx.readFile(file)]));cases.push(clean({id:cases.length,origin,nodes,sources,answers:observations.get(ctx)??[],heads:nodes.map(node=>({node:node.id,types:[...readers.declaredTypeParameters(node,ctx)].map(([name,value])=>[name,value??null]),values:[...readers.declaredValueParameters(node,ctx)]})),allEdges:nodes.flatMap(n=>this.queries.getOutgoingEdges(n.id))}));}return close.call(this)};
const noop=new Proxy(function(){return noop},{get:()=>noop,apply:()=>noop});let stack=[],tests=[],afters=[],befores=[];
const vitest={expect:noop,describe(name,fn){stack.push(name);fn();stack.pop()},it(name,fn){tests.push({name:[...stack,name].join(' / '),fn})},afterAll(fn){afters.push(fn)},beforeAll(fn){befores.push(fn)}};
for(const file of ['type-parameter-refs.test.ts','scala-bare-call-scope.test.ts','scala-package-object-scope.test.ts']){tests=[];afters=[];befores=[];const code=ts.transpileModule(readFileSync(join(root,'__tests__',file),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;const localRequire=id=>id==='vitest'?vitest:id.startsWith('../src')?require('./dist'+id.slice('../src'.length)+(id==='../src'?'/index.js':'.js')):require(id);new vm.Script(code,{filename:file}).runInNewContext({require:localRequire,exports:{},console,process,Buffer});origin='__tests__/'+file;for(const before of befores)await before();for(const test of tests){origin=`__tests__/${file}: ${test.name}`;await test.fn();console.log(origin)}for(const after of afters)await after();}


const custom=[
 ['cpp', {'api.cpp':`struct T {};
struct TcParser { static void ParseLoop() {} };
template <typename T, class Parser = TcParser>
class Holder { T value; void parse() { Parser::ParseLoop(); } };
template <class Shard>
class Cache { void run() { Shard::ParseLoop(); } };
`}],
 ['go', {'go.mod':'module example.org/app\n','main.go':`package app
type T struct{}
type Box[T any] struct { Value T }
func (b Box[T]) Get() T { return b.Value }
func First[T any](values []T) T { return values[0] }
`}],
 ['kotlin', {'App.kt':`class T
class Box<out T>(val value: T)
fun <T> first(values: List<T>): T = values[0]
`}],
 ['csharp', {'App.cs':`class T {}
class Box<T> { public T Value; public T Get() { return Value; } }
class Utility { public T First<T>(T[] values) { return values[0]; } }
`}],
 ['swift', {'App.swift':`struct T {}
struct Box<T> { let value: T; func get() -> T { value } }
func first<T>(_ values: [T]) -> T { values[0] }
`}],
 ['rust', {'src/lib.rs':`pub struct T;
pub struct Boxed<T> { value: T }
pub fn first<T>(mut values: Vec<T>) -> T { values.remove(0) }
`}],
 ['vbnet', {'App.vb':`Public Class T
End Class
Public Class Box(Of T)
 Public Function GetValue(value As T) As T
  Return value
 End Function
End Class
`}],
 ['arkts', {'App.ets':`class T {}
class Box<T> { value: T; get(): T { return this.value; } }
`}],
];
for(const [name,files] of custom){const dir=mkdtempSync(join(tmpdir(),'prometeu-generics-'));let project;try{origin='generic '+name;for(const [file,source]of Object.entries(files)){mkdirSync(dirname(join(dir,file)),{recursive:true});writeFileSync(join(dir,file),source);}project=await Project.init(dir,{index:true});}finally{project?.close();rmSync(dir,{recursive:true,force:true});}}
writeFileSync(new URL('./cases.json',import.meta.url),JSON.stringify({revision,cases},null,2)+'\n');console.log({contexts:cases.length,answers:cases.reduce((sum,c)=>sum+c.answers.length,0)});
