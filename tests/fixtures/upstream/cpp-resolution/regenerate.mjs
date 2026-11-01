// Development-only capture of C++ type and receiver decisions from the pinned resolver.
import {createRequire} from 'node:module';
import {execFileSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdtempSync,mkdirSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join,resolve,dirname} from 'node:path';
import vm from 'node:vm';
const root=resolve(process.argv[2]),revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('Wrong reference revision');
const require=createRequire(join(root,'package.json')),ts=require('typescript'),gate=require('./dist/resolution/cpp-type-aliases.js');
const Project=Object.values(require('./dist/index.js')).find(v=>typeof v==='function'&&typeof v.init==='function'&&typeof v.open==='function');
const clean=value=>JSON.parse(JSON.stringify(value,(key,value)=>key==='updatedAt'||key==='original'?undefined:value));
const simple=result=>result?{targetNodeId:result.targetNodeId,confidence:result.confidence}:null;
const observations=new WeakMap(),references=new WeakMap(),cases=[];let origin='';
const Resolver=require('./dist/resolution/index.js').ReferenceResolver,resolveOne=Resolver.prototype.resolveOne;
Resolver.prototype.resolveOne=function(ref){let refs=references.get(this.context);if(!refs){refs=new Map();references.set(this.context,refs)}refs.set(JSON.stringify([ref.fromNodeId,ref.referenceName,ref.referenceKind,ref.line,ref.column]),clean(ref));return resolveOne.call(this,ref)};
const names=['resolveCppAliasedType','cppClassNamed','cppTypeNamed','cppAliasedTypeWritten','cppTemplateParameters'];
for(const name of names){const original=gate[name];gate[name]=function(...args){const result=original(...args);const ctx=args.find(value=>value&&typeof value.getNodeById==='function');if(ctx){let recorded=observations.get(ctx);if(!recorded){recorded=[];observations.set(ctx,recorded)}const reference=args.find(value=>value&&typeof value.referenceName==='string');recorded.push(clean({name,written:typeof args[0]==='string'?args[0]:undefined,node:typeof args[0]==='object'?args[0]?.id:undefined,scopes:Array.isArray(args[1])?args[1]:undefined,reference,visible:args.some(value=>typeof value==='function'),visibleIds:args.some(value=>typeof value==='function')?ctx.getAllFiles().flatMap(file=>ctx.getNodesInFile(file)).filter(args.find(value=>typeof value==='function')).map(node=>node.id):undefined,expected:result===undefined?{unclaimed:true}:result instanceof Set?[...result]:result}));}return result;};}
const close=Project.prototype.close;Project.prototype.close=function(){if(origin&&this.queries&&this.resolver){const ctx=this.resolver.context,nodes=this.queries.getAllNodes(),sources=Object.fromEntries(ctx.getAllFiles().map(file=>[file,ctx.readFile(file)]));cases.push(clean({id:cases.length,origin,nodes,sources,answers:observations.get(ctx)??[],references:[...(references.get(ctx)?.values()??[])],allEdges:nodes.flatMap(n=>this.queries.getOutgoingEdges(n.id))}));}return close.call(this)};
const noop=new Proxy(function(){return noop},{get:()=>noop,apply:()=>noop});let stack=[],tests=[],afters=[],befores=[],afterEach=[],beforeEach=[];
const vitest={expect:noop,describe(name,fn){stack.push(name);fn();stack.pop()},it(name,fn){tests.push({name:[...stack,name].join(' / '),fn})},afterAll(fn){afters.push(fn)},beforeAll(fn){befores.push(fn)},afterEach(fn){afterEach.push(fn)},beforeEach(fn){beforeEach.push(fn)}};
vitest.it.each=values=>(name,fn)=>{for(const value of values)vitest.it(name.replace('%s',String(value)),()=>fn(...(Array.isArray(value)?value:[value])))};
for(const file of ['cpp-namespace-qualifier.test.ts','cpp-type-alias-receiver.test.ts','cpp-global-alias-target.test.ts','cpp-alias-inherited-member.test.ts','cpp-macro-and-constructor-calls.test.ts']){tests=[];afters=[];befores=[];afterEach=[];beforeEach=[];const code=ts.transpileModule(readFileSync(join(root,'__tests__',file),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;const localRequire=id=>id==='vitest'?vitest:id.startsWith('../src')?require('./dist'+id.slice('../src'.length)+(id==='../src'?'/index.js':'')):require(id);new vm.Script(code,{filename:file}).runInNewContext({require:localRequire,exports:{},console,process,Buffer,__dirname:join(root,"__tests__")});origin='__tests__/'+file;for(const before of befores)await before();for(const test of tests){origin=`__tests__/${file}: ${test.name}`;for(const before of beforeEach)await before();try{await test.fn();console.log(origin)}finally{for(const after of afterEach)await after()}}for(const after of afters)await after();}


writeFileSync(new URL('./cases.json',import.meta.url),JSON.stringify({revision,cases},null,2)+'\n');console.log({contexts:cases.length,answers:cases.reduce((sum,c)=>sum+c.answers.length,0)});
