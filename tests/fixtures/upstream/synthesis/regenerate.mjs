// Independent canonical graphs from the pinned extractor, resolver, and synthesis pipeline.
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { runInNewContext } from 'node:vm';

const revision = '6aa358ccbae30fbf27318f79ee183212f726f9ba';
const upstream = resolve(process.argv[2] ?? '');
if (!process.argv[2] || execFileSync('git', ['-C', upstream, 'rev-parse', 'HEAD'], {encoding:'utf8'}).trim() !== revision) throw new Error('Pass the pinned prepared reference checkout.');
const require = createRequire(join(upstream,'package.json'));
const ts = require('typescript');
const { DatabaseConnection } = require('./dist/db/index.js');
const { QueryBuilder } = require('./dist/db/queries.js');
const { ReferenceResolver } = require('./dist/resolution/index.js');
const importReader=require('./dist/resolution/import-resolver.js');
const resolveImportPath=importReader.resolveImportPath;
let moduleAnswers=[];
importReader.resolveImportPath=(specifier,from,language,context)=>{const result=resolveImportPath(specifier,from,language,context);moduleAnswers.push({specifier,from,language,result:result??null});return result;};
const { synthesizeCallbackEdges } = require('./dist/resolution/callback-synthesizer.js');
const { initGrammars, loadGrammarsForLanguages } = require('./dist/extraction/grammars.js');
const { extractFromSource } = require('./dist/extraction/tree-sitter.js');
await initGrammars();
const projects = [
  {id:'go-cross-file',files:{'model.go':'package demo\ntype Worker struct {}\n','methods.go':'package demo\nfunc (w *Worker) Run() {}\n','other/model.go':'package other\ntype Worker struct {}\n'}},
  {id:'go-already-contained',files:{'model.go':'package demo\ntype Worker struct {}\nfunc (w Worker) Run() {}\n'}},
  {id:'go-grpc',files:{'rpc/service_grpc.pb.go':'package rpc\ntype UnimplementedTaskServer struct {}\nfunc (s *UnimplementedTaskServer) Run() {}\nfunc (s *UnimplementedTaskServer) mustEmbedTask() {}\n','app/task.go':'package app\ntype Handler struct {}\nfunc (h *Handler) Run() {}\n','app/not.go':'package app\ntype Other struct {}\nfunc (h *Other) Stop() {}\n'}},
  {id:'go-grpc-handwritten-negative',files:{'rpc/service.go':'package rpc\ntype UnimplementedTaskServer struct {}\nfunc (s *UnimplementedTaskServer) Run() {}\n','app/task.go':'package app\ntype Handler struct {}\nfunc (h *Handler) Run() {}\n'}},
  {id:'kotlin-platforms',files:{'commonMain/Api.kt':'package demo\nexpect fun platform(): String\nexpect class Worker { fun run() }\n','jvmMain/Api.kt':'package demo\nactual fun platform(): String = "jvm"\nactual class Worker { actual fun run() {} }\n','iosMain/Api.kt':'package demo\nactual fun platform(): String = "ios"\nactual class Worker { actual fun run() {} }\n'}},
  {id:'kotlin-typealias',files:{'commonMain/Alias.kt':'package demo\nexpect class Handle\n','jvmMain/Alias.kt':'package demo\nactual typealias Handle = String\n'}},
  {id:'kotlin-plain-negative',files:{'first/Api.kt':'package demo\nfun platform() {}\n','second/Api.kt':'package demo\nfun platform() {}\n'}},
  {id:'mybatis',files:{'UserMapper.java':'package example;\ninterface UserMapper { String find(); void save(); }\n','UserMapper.xml':'<mapper namespace="example.UserMapper">\n<select id="find">SELECT 1</select>\n<insert id="save">INSERT INTO users VALUES (1)</insert>\n</mapper>\n'}},
  {id:'mybatis-ambiguous',files:{'first/UserMapper.java':'package first;\ninterface UserMapper { String find(); }\n','second/UserMapper.java':'package second;\ninterface UserMapper { String find(); }\n','UserMapper.xml':'<mapper namespace="first.UserMapper"><select id="find">SELECT 1</select></mapper>\n'}},
  {id:'mybatis-kotlin-vendor',files:{'UserMapper.kt':'package example\ninterface UserMapper { fun find(): String }\n','UserMapper.xml':'<mapper namespace="example.UserMapper">\n<select id="find">SELECT 1</select>\n<select id="find" databaseId="mysql">SELECT 2</select>\n</mapper>\n'}},
  {id:'react-render',files:{'Counter.ts':'export class Counter {\n increment() { this.setState({ count: 1 }); }\n read() { return this.state; }\n render() { return null; }\n}\n'}},
  {id:'react-render-mock-exclusion',files:{'__mocks__/Counter.ts':'export class Counter {\n increment() { this.setState({ count: 1 }); }\n render() { return null; }\n}\n'}},
  {id:'flutter-build',files:{'counter.dart':'class Counter {\n void increment() { setState(() {}); }\n void read() {}\n Widget build(BuildContext context) { return Text("count"); }\n}\n'}},
  {id:'arkui-state',files:{'Counter.ets':'@Entry\n@Component\nstruct Counter {\n @State count: number = 0\n @State items: number[] = []\n countUp() { this.count++ }\n append() { this.items.push(1) }\n read() { return this.count == 1 }\n comments() { /* this.count = 1 */ }\n build() { Text("counter") }\n}\n'}},
  {id:'arkui-unobserved-negative',files:{'Counter.ets':'@Component\nstruct Counter {\n count: number = 0\n countUp() { this.count++ }\n build() { Text("counter") }\n}\n'}},
  {id:'fabric-native',assets:{'package.json':'{"dependencies":{"react-native":"1"}}'},frameworks:['fabric-view'],files:{'Screen.ts':'import codegenNativeComponent from "react-native/Libraries/Utilities/codegenNativeComponent";\nexport default codegenNativeComponent<NativeProps>("Screen");\n','Screen.kt':'class ScreenView { fun show() {} }\n','Screen.java':'class ScreenViewManager { void show() {} }\n'}},
  {id:'expo-native',assets:{'package.json':'{"dependencies":{"expo-modules-core":"1"}}'},frameworks:['expo-modules'],files:{'Battery.swift':'import ExpoModulesCore\npublic class BatteryModule: Module {\n public func definition() -> ModuleDefinition {\n  Name("Battery")\n  AsyncFunction("getLevel") { () -> Int in return 1 }\n }\n}\n','Battery.kt':'import expo.modules.kotlin.modules.Module\nclass BatteryModule : Module() {\n override fun definition() = ModuleDefinition {\n  Name("Battery")\n  AsyncFunction("getLevel") { 1 }\n }\n}\n'}},
  {id:'native-bridge',files:{'Battery.java':'class Battery { public int getLevel() { return 1; } }\n','Battery.m':'@interface Battery\n- (int)getLevel;\n@end\n@implementation Battery\n- (int)getLevel { return 1; }\n@end\n','main.ts':'export function read() { return NativeModules.Battery.getLevel(); }\n'}},
];

// Reuse literal file writes from the reference's full Nix synthesis regression suite.
const nixTests = ts.createSourceFile('nix.test.ts',readFileSync(join(upstream,'__tests__/nix-option-synthesizer.test.ts'),'utf8'),ts.ScriptTarget.Latest,true);
function literal(node) { if(node && ts.isBinaryExpression(node) && node.operatorToken.kind===ts.SyntaxKind.PlusToken) { const left=literal(node.left),right=literal(node.right); return left!==undefined && right!==undefined ? left+right:undefined; } return node && (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) ? node.text : undefined; }
function visitTests(node) {
  if (ts.isCallExpression(node) && node.expression.getText(nixTests) === 'it' && node.arguments[1]?.body) {
    const files = {};
    const visit = n => {
      if (ts.isCallExpression(n) && n.expression.getText(nixTests) === 'fs.writeFileSync') {
        const path = n.arguments[0]; const source = literal(n.arguments[1]);
        if (ts.isCallExpression(path) && source !== undefined) {
          const parts = path.arguments.slice(1).map(literal);
          if (parts.every(p=>p !== undefined)) files[parts.join('/')] = source;
        }
      }
      ts.forEachChild(n,visit);
    };
    visit(node.arguments[1].body);
    if (Object.keys(files).length) projects.push({id:`nix-${projects.filter(p=>p.id.startsWith('nix-')).length+1}`,origin:literal(node.arguments[0]),files});
  }
  ts.forEachChild(node,visitTests);
}
visitTests(nixTests);
const overloadTests = ts.createSourceFile('overloads.test.ts',readFileSync(join(upstream,'__tests__/override-overload-pairing.test.ts'),'utf8'),ts.ScriptTarget.Latest,true);
const overloadFiles = {};
function visitOverloads(node) {
  if (ts.isVariableDeclaration(node) && node.name.getText(overloadTests) === 'FILES' && ts.isObjectLiteralExpression(node.initializer)) {
    for (const property of node.initializer.properties) {
      if (ts.isPropertyAssignment(property) && literal(property.name) && literal(property.initializer) !== undefined) overloadFiles[literal(property.name)] = literal(property.initializer);
    }
  }
  ts.forEachChild(node,visitOverloads);
}
visitOverloads(overloadTests);
projects.push({id:'interface-overload-regressions',origin:'override-overload-pairing.test.ts FILES',files:overloadFiles});
projects.push({id:'typescript-static-overrides',files:{'classes.ts':[
  'class Base { static run() {} run() {} }',
  'class Static extends Base { static run() {} }',
  'class Instance extends Base { run() {} }',
  'class Both extends Base { static run() {} run() {} }',
  'interface Worker { work(): void; }',
  'class Implementation implements Worker { work() {} }',
].join('\n')}});
projects.push({id:'cpp-overrides',files:{'classes.cpp':[
  'class Base { public: virtual void run() {} template<typename T> void process(T value) {} };',
  'class Derived : public Base { public: void run() override {} template<typename T> void process(T value) {} };',
  'template<> void Derived::process<int>(int value) {}',
].join('\n')}});
projects.push({id:'swift-protocol-extension',files:{'Protocol.swift':'protocol Worker {}\nextension Worker { func run() {} }\nclass Task: Worker { func run() {} }\n'}});

const languageFor = path => ({mm:'objc',vue:'vue',tsx:'tsx',jsx:'jsx',mjs:'javascript',cjs:'javascript',py:'python',rb:'ruby',php:'php',erl:'erlang',hrl:'erlang',pas:'pascal',dfm:'pascal',fmx:'pascal',svelte:'svelte',js:'javascript',go:'go',kt:'kotlin',java:'java',xml:'xml',ts:'typescript',dart:'dart',ets:'arkts',nix:'nix',m:'objc',swift:'swift',cs:'csharp',scala:'scala',cpp:'cpp',rs:'rust'})[path.split('.').at(-1)];
const closureTests = ts.createSourceFile('closure.test.ts',readFileSync(join(upstream,'__tests__/closure-collection-synthesizer.test.ts'),'utf8'),ts.ScriptTarget.Latest,true);
function visitClosure(node) {
  if (ts.isCallExpression(node) && node.expression.getText(closureTests) === 'it' && node.arguments[1]?.body) {
    const files = {};
    function writes(n) {
      if (ts.isCallExpression(n) && n.expression.getText(closureTests) === 'fs.writeFileSync' && ts.isCallExpression(n.arguments[0]) && literal(n.arguments[1]) !== undefined) {
        const parts=n.arguments[0].arguments.slice(1).map(literal);if(parts.every(p=>p!==undefined))files[parts.join('/')]=literal(n.arguments[1]);
      }
      ts.forEachChild(n,writes);
    }
    writes(node.arguments[1].body);
    if(Object.keys(files).length)projects.push({id:'closure-'+projects.filter(p=>p.id.startsWith('closure-')).length,origin:literal(node.arguments[0]),files});
  }
  ts.forEachChild(node,visitClosure);
}
visitClosure(closureTests);
projects.push({id:'callback-field',files:{'Signal.ts':[
  'class Signal {',
  ' callbacks = new Set();',
  ' onUpdate(callback: () => void) { this.callbacks.add(callback); }',
  ' triggerUpdate() { for (const callback of this.callbacks) { callback(); } }',
  ' setup() { this.onUpdate(this.refresh); }',
  ' refresh() {}',
  '}',
].join('\n')}});
projects.push({id:'callback-import',files:{'signal.ts':[
  'export class Signal {',
  ' callbacks = new Set();',
  ' onUpdate(callback: () => void) { this.callbacks.push(callback); }',
  ' notify() { this.callbacks.forEach(callback => callback()); }',
  '}',
].join('\n'),'handler.ts':'export function refresh() {}\n','main.ts':'import { Signal } from "./signal";\nimport { refresh } from "./handler";\nfunction setup(signal: Signal) { signal.onUpdate(refresh); }\n'}});
projects.push({id:'emitter-order-unicode',files:{'events.ts':[
  'export function handler() {}',
  'export function setup() { /* 🌍 */ bus.on("first", handler); bus.once("second", handler); }',
  'export function dispatch() { bus.emit("first"); bus.emit("second"); }',
].join('\n')}});
projects.push({id:'emitter-fanout-negative',files:{'events.ts':Array.from({length:7},(_,i)=>'function handler'+i+'() {}\nbus.on("event", handler'+i+');').join('\n')+'\nfunction dispatch() { bus.emit("event"); }\n'}});
projects.push({id:'emitter-manual-mock-negative',files:{'__mocks__/handler.ts':'export function handler() {}\n','main.ts':'export function setup() { bus.on("event", handler); }\nexport function dispatch() { bus.emit("event"); }\n'}});
projects.push({id:'pascal-forms',files:{'Main.pas':'unit Main;\ninterface\nimplementation\n{$R *.dfm}\nend.\n','Main.dfm':'object MainForm: TMainForm\n  Caption = \"Main\"\nend\n','Unrelated.dfm':'object Other: TOther\nend\n'}});
projects.push({id:'svelte-loaders',routeResolvers:['svelte'],files:{'src/routes/+page.svelte':'<script>export let data;</script>\n<h1>{data.title}</h1>\n','src/routes/+page.server.ts':'export function load() { return { title: "page" }; }\nexport const actions = { logout: async () => {} };\nfunction helper() {}\n','src/routes/+layout.svelte':'<slot />\n','src/routes/+layout.ts':'export const load = () => ({ shared: true });\n'}});
projects.push({id:'redux-thunks',files:{'actions.ts':[
'export const first = createAsyncThunk("first", async (_, api) => {',
' api.dispatch(second());',
' // dispatch(ignored());',
'});',
'export const second = createAsyncThunk("second", async () => {});',
'export function ignored() {}',
].join('\n'),'service.ts':'export function second() {}\n'}});
for (const [prefix,filename] of [['spring','spring-event-synthesizer.test.ts'],['mediatr','mediatr-dispatch-synthesizer.test.ts'],['celery','celery-dispatch-synthesizer.test.ts'],['sidekiq','sidekiq-dispatch-synthesizer.test.ts'],['erlang','erlang-behaviour-synthesizer.test.ts'],['laravel','laravel-event-synthesizer.test.ts'],['arkui','arkts-resolution.test.ts'],['gin','gin-middleware-chain.test.ts'],['goframe','goframe.test.ts'],['rtk','rtk-query-synthesizer.test.ts'],['pinia','pinia-store-synthesizer.test.ts'],['vuex','vuex-dispatch-synthesizer.test.ts'],['registry','object-registry-synthesizer.test.ts'],['rn','rn-event-channel.test.ts']]) {
  const tree=ts.createSourceFile(filename,readFileSync(join(upstream,'__tests__',filename),'utf8'),ts.ScriptTarget.Latest,true);
  function visit(node) {
    if(ts.isCallExpression(node) && node.expression.getText(tree)==='it' && node.arguments[1]?.body) {
      const files={};let complete=true;
      if(prefix==='arkui' && !/links emit|numeric-literal|links the navigating/.test(literal(node.arguments[0])??''))return;
      function writes(n) {
        if(ts.isCallExpression(n) && ['write','fs.writeFileSync'].includes(n.expression.getText(tree))) {
          let path=literal(n.arguments[0]); const source=literal(n.arguments[1]);
          if(ts.isCallExpression(n.arguments[0])) {
            const parts=n.arguments[0].arguments.slice(1).map(literal);
            if(parts.every(p=>p!==undefined))path=parts.join('/');
          }
          if(path===undefined || source===undefined)complete=false;else files[path]=source;
        }
        ts.forEachChild(n,writes);
      }
      writes(node.arguments[1].body);
      if(complete && Object.keys(files).length) {
        const assets={};for(const path of Object.keys(files))if(!languageFor(path)){assets[path]=files[path];delete files[path];}
        projects.push({id:prefix+'-'+projects.filter(p=>p.id.startsWith(prefix+'-')).length,origin:literal(node.arguments[0]),files,assets,...(prefix==='goframe'?{routeResolver:'goframe'}:{})});
      }
    }
    ts.forEachChild(node,visit);
  }
  visit(tree);
}
projects.push({id:'laravel-registration',files:{
  'events.php':'<?php\nclass First {}\nclass Second {}\nclass Third {}\n',
  'listeners.php':'<?php\nclass Typed { public function handle(First|Second $event) {} }\nclass Untyped { public function handle() {} }\nclass Extra { public function handle(Third $event) {} }\n',
  'provider.php':`<?php
class Provider {
 protected $listen = [Third::class => [Untyped::class, Extra::class]];
}
`,
  'controller.php':`<?php
class Controller {
 public function dispatch() {
  event(new First());
  event(new Second());
  event(new Third());
  // event(new Missing());
 }
}
`,
}});
projects.push({id:'celery-ambiguity-comments',files:{
  'first.py':'@shared_task\ndef task(): pass\n\ndef run():\n    task.delay()\n',
  'second.py':'@app.task()\ndef task(): pass\n',
  'caller.py':`def run():
    # task.delay()
    """task.delay()"""
    task.delay()
`,
}});
projects.push({id:'sidekiq-namespaces',files:{
 'a.rb':'module A\n class Worker\n  include Sidekiq::Job\n  def perform; end\n end\nend\n',
 'b.rb':'module B\n class Worker\n  include Sidekiq::Worker\n  def perform; end\n end\nend\n',
 'caller.rb':'def run\n A::Worker.perform_async\n B::Worker.perform_in(5)\n Worker.perform_async\n # A::Worker.perform_async\nend\n',
}});
projects.push({id:'erlang-ambiguous-behaviour',files:{
 'a.erl':'-module(a).\n-callback run(term()) -> ok.\n',
 'b.erl':'-module(b).\n-callback run(term()) -> ok.\n',
 'impl.erl':'-module(impl).\n-behaviour(a).\n-export([run/1]).\nrun(_) -> ok.\n',
 'caller.erl':'-module(caller).\n-export([call/1]).\ncall(Mod) -> Mod:run(<<1,2>>).\n',
}});
projects.push({id:'arkui-workspace-scopes',assets:{'oh-package.json5':JSON.stringify({dependencies:{a:'file:./a',b:'file:./b'}})},files:{
 'a/src/main/ets/pages/Home.ets':`@Entry
@Component
struct Home {
 open() { router.pushUrl({url:'pages/Detail'}); emitter.emit(Channel.changed); }
 listen() { emitter.on(Channel.changed, () => {}); }
 build() {}
}
`,
 'a/src/main/ets/pages/Detail.ets':'@Entry\n@Component\nstruct Detail { build() {} }\n',
 'b/src/main/ets/pages/Detail.ets':'@Entry\n@Component\nstruct Detail { build() {} }\n',
 'b/listener.ets':'export function listen() { emitter.on(Channel.changed, () => {}); }\n',
}});
projects.push({id:'arkui-numeric-same-file',files:{'event.ets':`export function fire() { emitter.emit({eventId: 4}); }
export function listen() { emitter.on({eventId: 4}, () => {}); }
export function alias() { const id = new EventsId(4); emitter.once(id, () => {}); }
`,'other.ets':'export function listen() { emitter.on({eventId:4}, () => {}); }\n'}});
projects.push({id:'arkui-fanout-negative',files:{'event.ets':Array.from({length:9},(_,i)=>'export function listen'+i+'() { emitter.on(Channel.changed, () => {}); }').join('\n')+'\nexport function fire() { emitter.emit(Channel.changed); }\n'}});
projects.push({id:'vue-template-handlers',files:{
 'src/Home.vue':`<script setup lang="ts">
import { useDialog } from './useDialog';
const { close: closeDialog } = useDialog();
function save() {}
</script>
<template><button @click="save" @keydown.enter="closeDialog()"/><media-card/><BaseButton/></template>
`,
 'src/components/media/Card.vue':'<template><div>Card</div></template>\n',
 'src/components/base/BaseButton.vue':'<template><button>Button</button></template>\n',
 'src/useDialog.ts':'export function useDialog() {\n function close() {}\n return {close};\n}\n',
}});
projects.push({id:'vue-external-imports',assets:{'package.json':JSON.stringify({dependencies:{'external-ui':'1','external-hooks':'1'}})},files:{
 'src/Home.vue':`<script setup lang="ts">
import { Card } from 'external-ui';
import { useDialog } from 'external-hooks';
const { close } = useDialog();
</script>
<template><Card/><button @click="close"/></template>
`,
 'src/Card.vue':'<template><div>Local</div></template>\n',
 'src/useDialog.ts':'export function useDialog() {\n function close() {}\n return {close};\n}\n',
}});
projects.push({id:'rn-scoped-constants',files:{
 'Emitter.kt':`class Emitter {
 companion object { const val EVENT = "location" }
 fun fire() { emitter.emit(EVENT, null) }
}
`,
 'events.ts':'export const EVENT = "location";\n',
 'app.ts':`import * as Events from './events';
export function listen() { emitter.addListener(Events.EVENT, listener); }
export function listener() {}
`,
 'other.ts':'export const EVENT = "other";\n',
}});
projects.push({id:'next-markup-navigation',routeResolvers:['nextjs'],assets:{'package.json':'{"dependencies":{"next":"1"}}'},files:{
 'app/page.tsx':`export default function Home() {
 return <main>
   <Link href="/users/12?tab=profile">User</Link>
   <a href="/about">About</a>
   <NextLink href={\`/users/\${id}\`}>User</NextLink>
   <Link href={{pathname: '/about'}}>Object</Link>
   <a href="https://external.test/about">External</a>
 </main>;
}
`,
 'app/users/[id]/page.tsx':'export default function Profile() { return null; }\n',
 'app/about/page.tsx':'export default function About() { return null; }\n',
}});
projects.push({id:'svelte-markup-navigation',routeResolvers:['svelte'],assets:{'package.json':'{"dependencies":{"@sveltejs/kit":"1"}}'},files:{
 'src/routes/+page.svelte':`<script>let username = 'x';</script>
<a href="/login">Login</a>
<a href="/profile/@{username}">Profile</a>
<a href={\`/article/\${slug}\`}>Article</a>
<a href="/docs">Docs</a>
<a href="https://example.test/login">External</a>
`,
 'src/routes/login/+page.svelte':'<h1>Login</h1>\n',
 'src/routes/profile/@[user]/+page.svelte':'<h1>Profile</h1>\n',
 'src/routes/article/[slug]/+page.svelte':'<h1>Article</h1>\n',
 'src/routes/docs/[[optional]]/+page.svelte':'<h1>Docs</h1>\n',
 'src/routes/[...rest]/+page.svelte':'<h1>Fallback</h1>\n',
}});
projects.push({id:'svelte-app-boundaries',routeResolvers:['svelte'],files:{
 'apps/a/src/routes/+page.svelte':'<a href="/detail">Detail</a>\n',
 'apps/a/src/routes/detail/+page.svelte':'<h1>A detail</h1>\n',
 'apps/b/src/routes/+page.svelte':'<a href="/detail">Detail</a>\n',
 'apps/b/src/routes/detail/+page.svelte':'<h1>B detail</h1>\n',
}});
projects.push({id:'expo-helper-return',routeResolvers:['expo-router'],assets:{'package.json':'{"dependencies":{"expo-router":"1"}}'},files:{
 'app/index.tsx':`import { destination } from '../flow';
export default function Home() { router.push(destination()); return null; }
`,
 'app/welcome.tsx':'export default function Welcome() { return null; }\n',
 'app/home.tsx':'export default function Main() { return null; }\n',
 'flow.ts':`export function destination(): '/welcome' | '/home' {
 return /* 🌍 */ ready ? '/home' : '/welcome';
}
`,
}});
const expoCrLf=projects.find(project=>project.id==='expo-helper-return');
projects.push({...expoCrLf,id:'expo-helper-return-crlf',files:Object.fromEntries(Object.entries(expoCrLf.files).map(([path,source])=>[path,source.replaceAll('\n','\r\n')]))});
projects.push({id:'next-call-navigation',routeResolvers:['nextjs'],assets:{'package.json':'{"dependencies":{"next":"1"}}'},files:{
 'app/page.tsx':`export default function Home() {
 const target = allowed ? '/login' : '/users/42';
 router.push(target as any);
 router.replace(ready ? '/login' : computed);
 redirect({pathname: '/login'});
 NextResponse.redirect(new URL('/login', request.url));
 /* 🌍 */ router.prefetch(\`/users/\${id}?tab=x\`);
 let changed = '/login';
 changed = unknown;
 router.push(changed);
 return null;
}
`,
 'app/login/page.tsx':'export default function Login() { return null; }\n',
 'app/users/[id]/page.tsx':'export default function User() { return null; }\n',
}});
projects.push({id:'expo-call-navigation',routeResolvers:['expo-router'],assets:{'package.json':'{"dependencies":{"expo-router":"1"}}'},files:{
 'app/capture/review.tsx':`export default function Review() {
 const target = ok ? '/home' : '/welcome';
 router.navigate(target);
 router.push('./edit');
 safePush('../home');
 router.replace({pathname: '/user/[id]', params:{id}});
 router.dismissTo(ready ? '/home' : '/welcome');
 return null;
}
`,
 'app/capture/edit.tsx':'export default function Edit() { return null; }\n',
 'app/home.tsx':'export default function Home() { return null; }\n',
 'app/welcome.tsx':'export default function Welcome() { return null; }\n',
 'app/user/[id].tsx':'export default function User() { return null; }\n',
}});
projects.push({id:'svelte-call-navigation',routeResolvers:['svelte'],assets:{'package.json':'{"dependencies":{"@sveltejs/kit":"1"}}'},files:{
 'src/routes/+page.svelte':`<script lang="ts">
function go() {
 const target = ok ? '/login' : '/user/7';
 goto(target);
 redirect(303, '/login');
 redirect(302, ok ? '/login' : '/user/7');
}
</script><button on:click={go}>Go</button>
`,
 'src/routes/login/+page.svelte':'<h1>Login</h1>\n',
 'src/routes/user/[id]/+page.svelte':'<h1>User</h1>\n',
}});
// Read the original Next proxy project's literal inputs, including its two pure helpers.
const proxyTestSource=readFileSync(join(upstream,'__tests__/nextjs-proxy.test.ts'),'utf8');
const proxyTree=ts.createSourceFile('nextjs-proxy.test.ts',proxyTestSource,ts.ScriptTarget.Latest,true);
function proxyValue(node) {
  const text=literal(node);if(text!==undefined)return text;
  if(!node)return undefined;
  if(node.kind===ts.SyntaxKind.TrueKeyword)return true;
  if(node.kind===ts.SyntaxKind.FalseKeyword)return false;
  if(ts.isArrayLiteralExpression(node)){const values=node.elements.map(proxyValue);return values.every(x=>x!==undefined)?values:undefined;}
  if(ts.isObjectLiteralExpression(node)){
    const object={};for(const property of node.properties){if(!ts.isPropertyAssignment(property))return undefined;const value=proxyValue(property.initializer);if(value===undefined)return undefined;object[literal(property.name)??property.name.getText(proxyTree)]=value;}return object;
  }
  if(ts.isCallExpression(node)){
    const argument=proxyValue(node.arguments[0]);if(argument===undefined)return undefined;
    const name=node.expression.getText(proxyTree);
    if(name==='page')return `export default function ${argument}() {\n  return null\n}\n`;
    if(name==='manifest')return JSON.stringify({name:'web',dependencies:{next:argument,react:'19.0.0'}});
    if(name==='JSON.stringify')return JSON.stringify(argument);
  }
  return undefined;
}
function proxyProjects(node){
  if(ts.isCallExpression(node)&&node.expression.getText(proxyTree)==='project'){
    const all=proxyValue(node.arguments[0]);if(all){
      const files={},assets={};for(const [path,source]of Object.entries(all)){(languageFor(path)?files:assets)[path]=source;}
      const next=Object.entries(assets).some(([path,source])=>path.endsWith('package.json')&&JSON.parse(source).dependencies?.next);
      projects.push({id:'next-proxy-'+projects.filter(p=>p.id.startsWith('next-proxy-')).length,origin:'nextjs-proxy.test.ts project()',files,assets,routeResolvers:next?['nextjs']:[]});
    }
  }
  ts.forEachChild(node,proxyProjects);
}
proxyProjects(proxyTree);
const tanstackTree=ts.createSourceFile('tanstack-router.test.ts',readFileSync(join(upstream,'__tests__/tanstack-router.test.ts'),'utf8'),ts.ScriptTarget.Latest,true);
const tanstackFiles={};
function tanstackWrites(node){
  if(ts.isCallExpression(node)&&node.expression.getText(tanstackTree)==='write'){
    const path=literal(node.arguments[0]),source=literal(node.arguments[1]);
    if(path!==undefined&&source!==undefined)tanstackFiles[path]=source;
  }
  ts.forEachChild(node,tanstackWrites);
}
tanstackWrites(tanstackTree);
projects.push({id:'tanstack-original-app',origin:'tanstack-router.test.ts beforeAll sources',assets:{'package.json':JSON.stringify({name:'app',dependencies:{react:'19','@tanstack/react-router':'1'}})},files:tanstackFiles,routeResolvers:['tanstack-router']});
projects.push({id:'tanstack-code-tree',assets:{'package.json':JSON.stringify({dependencies:{'@tanstack/solid-router':'1'}})},routeResolvers:['tanstack-router'],files:{
 'apps/web/src/router.tsx':`const root = createRootRoute({component:Root});
const posts = createRoute({path:'posts',getParentRoute:()=>root,component:Posts});
const index = createRoute({path:'/',getParentRoute:()=>posts,component:PostsIndex});
const post = createRoute({path:'$id',getParentRoute:()=>posts,component:Post});
function Root() { return <Outlet/>; }
function Posts() { return <Outlet/>; }
function PostsIndex() { return <Link to="/posts/$id" params={{id:1}}>Post</Link>; }
function Post() { router.navigate({to:'/posts'}); return <Navigate to={'/posts'}/>; }
`,
}});
const vueTree=ts.createSourceFile('vue-router.test.ts',readFileSync(join(upstream,'__tests__/vue-router.test.ts'),'utf8'),ts.ScriptTarget.Latest,true);
const vueFiles={};
function vueWrites(node){
  if(ts.isCallExpression(node)&&node.expression.getText(vueTree)==='write'){
    const path=literal(node.arguments[0]),source=literal(node.arguments[1]);
    if(path!==undefined&&source!==undefined)vueFiles[path]=source;
  }
  ts.forEachChild(node,vueWrites);
}
vueWrites(vueTree);
projects.push({id:'vue-router-original-app',origin:'vue-router.test.ts beforeAll sources',assets:{'package.json':JSON.stringify({name:'conduit',dependencies:{vue:'3','vue-router':'4'}})},files:vueFiles,routeResolvers:['vue-router']});
projects.push({id:'vue-router-nested-layouts',assets:{'package.json':JSON.stringify({dependencies:{'vue-router':'4'}})},routeResolvers:['vue-router'],files:{
 'src/router/index.ts':`import Layout from '@/layout';
const routes = [{path:'/dashboard',component:Layout,redirect:'/dashboard/main',children:[
 {path:'main',name:'Main',component:()=>import('#/views/main/index.vue')},
 {path:'user/:id',name:'User',component:()=>import('@/views/User.vue')}
]}];
`,
 'src/layout/index.vue':'<template><nav><RouterLink :to="{ name: \'Main\' }">Main</RouterLink></nav></template>\n',
 'src/views/main/index.vue':`<template><RouterLink :to="{path:'/dashboard/user/7'}">User</RouterLink></template>
<script setup>
function go() { router.push({name:'User',params:{id:3}}); }
function other() { router.replace(ok?'/dashboard/main':'/dashboard/user/4'); navigateTo('https://outside.example/dashboard/main'); }
</script>
`,
 'src/views/User.vue':'<template>User</template>\n',
}});
// Run the reference JSX tests only through their first index call, capturing
// their actual setup file writes; extraction/resolution below remain real.
const jsxText=readFileSync(join(upstream,'__tests__/jsx-child-disambiguation.test.ts'),'utf8');
const jsxTree=ts.createSourceFile('jsx.test.ts',jsxText,ts.ScriptTarget.Latest,true);
const jsxProgram=jsxTree.statements.filter(statement=>!ts.isImportDeclaration(statement)).map(statement=>statement.getText(jsxTree)).join('\n');
const engineImport=jsxTree.statements.find(statement=>ts.isImportDeclaration(statement)&&statement.moduleSpecifier.text==='../src').importClause.namedBindings.elements[0].name.text;
const jsxRegistered=[],jsxHooks=[[]];let jsxWrites={};const jsxStop={};
const jsxIt=(name,fn)=>jsxRegistered.push({name,fn,hooks:jsxHooks.flat()});
jsxIt.each=rows=>(name,fn)=>rows.forEach(row=>jsxIt(name,()=>fn(...(Array.isArray(row)?row:[row]))));jsxIt.skip=()=>{};
const jsxGlobals={
 describe(name,fn){if(name==='jsxCode')return;jsxHooks.push([]);fn();jsxHooks.pop();},
 it:jsxIt,beforeEach(fn){jsxHooks.at(-1).push(fn);},afterEach(){},expect(){throw new Error('An index fixture reached its assertions without indexing.');},
 path:{join:(...parts)=>parts.join('/'),dirname:path=>path.slice(0,path.lastIndexOf('/'))},os:{tmpdir:()=>'/fixture'},
 fs:{mkdtempSync:()=>'/fixture',mkdirSync(){},writeFileSync(path,source){jsxWrites[path.slice('/fixture/'.length)]=source;},rmSync(){}},
 [engineImport]:{async init(){throw jsxStop;}},
};
runInNewContext(ts.transpileModule(jsxProgram,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.None}}).outputText,jsxGlobals);
for(const test of jsxRegistered){jsxWrites={};for(const hook of test.hooks)await hook();try{await test.fn();}catch(error){if(error!==jsxStop)throw error;}const files={},assets={};for(const[path,source]of Object.entries(jsxWrites)){if(languageFor(path))files[path]=source;else assets[path]=source;}if(Object.keys(files).length)projects.push({id:`jsx-${projects.filter(project=>project.id.startsWith('jsx-')).length}`,origin:test.name,files,assets});}

// Capture real setup writes from the reference's cross-file React routing suites.
for(const testFile of ['react-router.test.ts','react-router-call-tables.test.ts','react-router-returned-tables.test.ts','react-router-nested-lazy.test.ts','react-router-lazy-barrel.test.ts','react-router-lazy-member.test.ts','react-router-outlet-routes.test.ts','react-router-config-href.test.ts','react-router-barrel-imports.test.ts']){
 const text=readFileSync(join(upstream,'__tests__',testFile),'utf8');const tree=ts.createSourceFile(testFile,text,ts.ScriptTarget.Latest,true);
 const imported=tree.statements.find(n=>ts.isImportDeclaration(n)&&n.moduleSpecifier.text==='../src')?.importClause?.namedBindings?.elements?.[0]?.name?.text;if(!imported)throw Error(`missing engine import ${testFile}`);
 const hooks=[],tests=[],before=[[]],writes=new Map();let serial=0;
 const fakeFs={mkdirSync(){},mkdtempSync(){return `/react-fixture/${serial++}`;},writeFileSync(path,source){writes.set(path,source);},rmSync(){},readFileSync(path){if(!writes.has(path))throw Error(`uncaptured read ${path}`);return writes.get(path);}};
 const stop={};const makeCase=root=>{const files={},assets={};for(const[path,source]of writes){if(!path.startsWith(root+'/'))continue;const relative=path.slice(root.length+1);if(languageFor(relative))files[relative]=source;else assets[relative]=source;}if(Object.keys(files).length&&!projects.some(project=>project.origin===testFile&&JSON.stringify(project.files)===JSON.stringify(files)))projects.push({id:`react-tables-${projects.filter(project=>project.id.startsWith('react-tables-')).length}`,origin:testFile,files,assets,routeResolvers:['react'],reactFinalize:true});};
 const noop=new Proxy(()=>noop,{get:(_target,key)=>key==='then'?undefined:noop});
 const test=(name,fn)=>tests.push({name,fn,before:before.flat()});test.each=rows=>(name,fn)=>rows.forEach(row=>test(name,()=>fn(...(Array.isArray(row)?row:[row]))));test.skip=()=>{};
 const globals={describe(_name,fn){before.push([]);fn();before.pop();},it:test,test,beforeEach(fn){before.at(-1).push(fn);},beforeAll(fn){hooks.push(fn);},afterEach(){},afterAll(){},expect:()=>noop,URLSearchParams,fs:fakeFs,path:require('node:path'),os:require('node:os'),[imported]:{async init(root){makeCase(root);throw stop;},initSync(root){makeCase(root);throw stop;}}};
 for(const declaration of tree.statements.filter(n=>ts.isImportDeclaration(n))){const spec=declaration.moduleSpecifier.text;if(spec.startsWith('../src/')&&declaration.importClause?.namedBindings?.elements){const module=require('./dist/'+spec.slice('../src/'.length)+'.js');for(const binding of declaration.importClause.namedBindings.elements)globals[binding.name.text]=module[binding.propertyName?.text??binding.name.text];}}
 globals.initGrammars=async()=>{};globals.loadAllGrammars=async()=>{};
 const program=tree.statements.filter(n=>!ts.isImportDeclaration(n)).map(n=>n.getText(tree)).join('\n');runInNewContext(ts.transpileModule(program,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.None}}).outputText,globals);
 for(const hook of hooks){try{await hook();}catch(error){if(error!==stop)throw error;}}
 for(const test of tests){try{for(const hook of test.before)await hook();await test.fn();}catch(error){if(error!==stop&&!(error instanceof TypeError)&&error?.name!=='TypeError')throw error;}}
}

projects.push({id:'react-tables-unicode-scopes',origin:'Unicode comments before a lexical route-table scope',assets:{'package.json':JSON.stringify({dependencies:{react:'18','react-router-dom':'6'}})},routeResolvers:['react'],reactFinalize:true,files:{'App.tsx':`import { useRoutes } from 'react-router-dom';
function Page() { return <main />; }
const routes = [{path:'/global',element:<Page/>}];
/* ${'🎈'.repeat(80)} */ export function App() { const routes = [{path:'/local',element:<Page/>}]; return useRoutes(routes); }
`}});

const cases = [];
for (const project of projects) {
  moduleAnswers=[];
  const root = mkdtempSync(join(tmpdir(),'prometeu-synthesis-'));
  const connection = DatabaseConnection.initialize(join(root,'reference.sqlite'));
  try {
    const queries = new QueryBuilder(connection.getDb());
    const projectReferences=[];
    for(const [path,source] of Object.entries(project.assets??{})) { mkdirSync(dirname(join(root,path)),{recursive:true});writeFileSync(join(root,path),source); }
    for (const [path,source] of Object.entries(project.files)) {
      mkdirSync(dirname(join(root,path)),{recursive:true}); writeFileSync(join(root,path),source);
      const language = languageFor(path);
      if (language !== 'xml') await loadGrammarsForLanguages([language]);
      const result = extractFromSource(path,source,language,project.frameworks ?? []);
      for(const framework of project.routeResolvers??[]) {
        const name={react:'reactResolver',svelte:'svelteResolver',nextjs:'nextjsResolver','expo-router':'expoRouterResolver','tanstack-router':'tanstackRouterResolver','vue-router':'vueRouterResolver'}[framework];
        const extra=require('./dist/resolution/frameworks/'+framework+'.js')[name].extract(path,source);
        result.nodes.push(...extra.nodes);result.unresolvedReferences.push(...extra.references);
      }
      if(project.routeResolver==='goframe') {
        const {goframeResolver}=require('./dist/resolution/frameworks/goframe.js');
        const routed=goframeResolver.extract(path,source); result.nodes.push(...routed.nodes);
      }
      queries.insertNodes(result.nodes); queries.insertEdges(result.edges);
      const references=result.unresolvedReferences.map(ref=>({...ref,filePath:path,language}));
      projectReferences.push(...references);queries.insertUnresolvedRefsBatch(references);
      queries.upsertFile({path,contentHash:'reference',language,size:Buffer.byteLength(source),indexedAt:Date.now(),modifiedAt:Date.now(),nodeCount:result.nodes.length});
    }
    let finalization,inputNodes;
    if(project.reactFinalize){
      inputNodes=queries.getAllNodes();
      const reader=new ReferenceResolver(root,queries);reader.initialize();
      const framework=require('./dist/resolution/frameworks/react.js').reactResolver;
      const updates=framework.postExtract(reader.context);const extra=framework.crossFileNodes(reader.context);
      finalization={updates,nodes:extra.nodes,references:extra.references};
      queries.insertNodes(updates);queries.insertNodes(extra.nodes);queries.insertUnresolvedRefsBatch(extra.references);projectReferences.push(...extra.references);
      for(const node of [...inputNodes,...updates,...extra.nodes])delete node.updatedAt;
    }
    const resolver = new ReferenceResolver(root,queries);
    resolver.initialize();
    await resolver.resolveAndPersistBatched(undefined,5000,undefined,undefined,false);
    await resolver.resolveChainedCallsViaConformance();
    await resolver.resolveDeferredThisMemberRefs();
    const nodes = queries.getAllNodes();
    const baseEdges = nodes.flatMap(n=>queries.getOutgoingEdges(n.id));
    const unresolved = queries.getUnresolvedReferencesFromNodes(nodes.map(n=>n.id), ['extends']);
    const imports=[];
    for(const path of Object.keys(project.files)) {
      const language=languageFor(path);
      for(const binding of resolver.context.getImportMappings(path,language))imports.push({file:path,local_name:binding.localName,source:binding.source,imported_name:binding.exportedName,is_namespace:binding.isNamespace??false,is_default:binding.isDefault??false,mapped_path:binding.resolvedPath??null,resolved_path:resolveImportPath(binding.source,path,language,resolver.context)??null,external:resolver.context.isOutOfRepoImport?.(binding.source,path,language)===true});
    }
    const detected=require('./dist/resolution/frameworks/index.js').detectFrameworks(resolver.context);
    const activeFrameworks=detected.map(framework=>framework.name);
    const routing=detected.filter(framework=>['react-router','nextjs','sveltekit-router','expo-router','tanstack-router','vue-router'].includes(framework.name));
    const activeNavigation=routing.map(framework=>framework.name),navigation=[];
    for(const reference of projectReferences) {
      let resolved;for(const framework of routing){resolved=framework.resolve(reference,resolver.context);if(resolved)break;}
      navigation.push({reference,expected:resolved?.edgeKind==='navigates'?[{target:resolved.targetNodeId,metadata:resolved.metadata??{}},...(resolved.alsoTargets??[]).map(target=>({target:target.targetNodeId,metadata:target.metadata??{}}))]:[]});
    }
    const workspaceDirs = [...new Set(resolver.context.getWorkspacePackages?.()?.byName.values()??[])];
    const importAnswers=[];const resolveBound=resolver.context.resolveImport?.bind(resolver.context);
    if(resolveBound)resolver.context.resolveImport=(reference)=>{const result=resolveBound(reference);importAnswers.push({reference,result:result?.targetNodeId??null});return result;};
    await synthesizeCallbackEdges(queries,resolver.context);
    const edges = nodes.flatMap(n=>queries.getOutgoingEdges(n.id));
    for (const node of nodes) delete node.updatedAt;
    cases.push({...project,finalization,inputNodes,nodes,baseEdges,unresolved,workspaceDirs,modules:[...new Map(moduleAnswers.map(row=>[JSON.stringify(row),row])).values()],importAnswers,imports,activeFrameworks,activeNavigation,navigation,edges});
    console.log(`${project.id}: ${nodes.length} nodes, ${edges.length-baseEdges.length} derived edges`);
  } finally { connection.close(); rmSync(root,{recursive:true,force:true}); }
}
writeFileSync(join(dirname(fileURLToPath(import.meta.url)),'cases.json'),JSON.stringify({revision,cases},null,2)+'\n');

// Execute the original pure matcher tests against the actual helpers and record every call.
const proxyModule=require('./dist/resolution/frameworks/next-proxy.js');
const proxyCases={compile:[],covers:[],read:[],directories:[]}, matcherSources=new WeakMap();
const assertion=new Proxy(()=>assertion,{get:()=>assertion});
const proxyIt=(_name,fn)=>fn();proxyIt.each=rows=>(_name,fn)=>rows.forEach(row=>fn(...(Array.isArray(row)?row:[row])));
const proxyPure=proxyTree.statements.filter(statement=>!ts.isImportDeclaration(statement)&&statement.getStart(proxyTree)<proxyTestSource.indexOf("describe('a route and the proxy")).map(statement=>statement.getText(proxyTree)).join('\n');
runInNewContext(ts.transpileModule(proxyPure,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.None}}).outputText,{
  describe:(_name,fn)=>fn(),it:proxyIt,expect:()=>assertion,
  stripCommentsForRegex:require('./dist/resolution/strip-comments.js').stripCommentsForRegex,
  compileMatcher(source){const result=proxyModule.compileMatcher(source);proxyCases.compile.push({source,expected:result!==null});if(result)matcherSources.set(result,source);return result;},
  matcherCovers(matcher,path,file){const expected=proxyModule.matcherCovers(matcher,path,file);proxyCases.covers.push({source:matcherSources.get(matcher),path,file,expected});return expected;},
  readProxyMatcher(source){const result=proxyModule.readProxyMatcher(source);proxyCases.read.push({source,expected:result==='all'?null:result.map(entry=>({source:entry.source,at:Buffer.byteLength(source.slice(0,entry.index))}))});return result;},
  nextProxyDir(root,files){const expected=proxyModule.nextProxyDir(root,files);proxyCases.directories.push({root,files,expected});return expected;},
});
writeFileSync(join(dirname(fileURLToPath(import.meta.url)),'next-proxy.json'),JSON.stringify({revision,...proxyCases},null,2)+'\n');

// Capture the actual shared comment helper; expand blanked UTF-16 spans to original UTF-8 widths.
const {stripCommentsForRegex}=require('./dist/resolution/strip-comments.js');
const commentTree=ts.createSourceFile('comments.test.ts',readFileSync(join(upstream,'__tests__/strip-comments.test.ts'),'utf8'),ts.ScriptTarget.Latest,true);
const commentCases=[];
function addComment(id,language,source) {
  const expected=stripCommentsForRegex(source,language);let offset=0,byteExpected='';
  for(const character of source) {
    const masked=expected.slice(offset,offset+character.length);offset+=character.length;
    byteExpected+=masked===character?character:masked===' '.repeat(character.length)?' '.repeat(Buffer.byteLength(character)):masked;
  }
  commentCases.push({id,language,source,expected,byteExpected});
}
function commentWalk(node) {
 if(ts.isCallExpression(node) && node.expression.getText(commentTree)==='it' && node.arguments[1]?.body) {
  let source,language;
  function inspect(n) {
   if(ts.isVariableDeclaration(n) && n.name.getText(commentTree)==='src')source=literal(n.initializer);
   if(ts.isCallExpression(n) && n.expression.getText(commentTree)==='stripCommentsForRegex')language=literal(n.arguments[1]);
   ts.forEachChild(n,inspect);
  }
  inspect(node.arguments[1].body);
  if(source!==undefined&&language!==undefined)addComment(literal(node.arguments[0]),language,source);
 }
 if(ts.isVariableDeclaration(node)&&node.name.getText(commentTree)==='cases'&&ts.isArrayLiteralExpression(node.initializer)) {
  for(const row of node.initializer.elements)if(ts.isArrayLiteralExpression(row)){const [label,language,source]=row.elements.map(literal);if(label&&language&&source!==undefined)addComment(label,language,source);}
 }
 ts.forEachChild(node,commentWalk);
}
commentWalk(commentTree);
addComment('Erlang atoms and character literals','erlang',`call() -> "$%", 'quoted%atom', $%, $\\%, ok. % removed 🌍\n`);
writeFileSync(join(dirname(fileURLToPath(import.meta.url)),'comments.json'),JSON.stringify({revision,cases:commentCases},null,2)+'\n');

const {blankStringContents}=require('./dist/resolution/strip-comments.js');
const strings=[
 `const exports = { browser: "global" };`,
 `const regex = /["']/; const text = "browser";`,
 `return /\\"/; browser;`,
 `const text = \`browser \${globalThis}\`;`,
 `const division = value / count; "browser";`,
 `const text = "unterminated\nbrowser", next = 'ok';`,
 `const escaped = "before\\\\after";`,
 `const text = "🌍\n"; browser;`,
];
const blankCases=strings.map((source,id)=>{const expected=blankStringContents(source);let offset=0,byteExpected='';for(const ch of source){const masked=expected.slice(offset,offset+ch.length);offset+=ch.length;byteExpected+=masked===ch?ch:masked===' '.repeat(ch.length)?' '.repeat(Buffer.byteLength(ch)):masked;}return{id,source,expected,byteExpected};});
writeFileSync(join(dirname(fileURLToPath(import.meta.url)),'strings.json'),JSON.stringify({revision,cases:blankCases},null,2)+'\n');
