// Development-only framework records from the pinned reference implementation.
import {execFileSync} from 'node:child_process';
import {createRequire} from 'node:module';
import {readFileSync,writeFileSync} from 'node:fs';
import {dirname,join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
const root=resolve(process.argv[2]),revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('Wrong reference revision');
const require=createRequire(join(root,'package.json')),ts=require('typescript');
const {getFrameworkResolver}=require('./dist/resolution/frameworks/index.js');
const cases=[],seen=new Set();
const language=path=>({ts:'typescript',tsx:'tsx',js:'javascript',jsx:'jsx',m:'objc',mm:'objc',java:'java',kt:'kotlin',swift:'swift',py:'python',go:'go',rs:'rust',yml:'yaml',yaml:'yaml',properties:'properties',vue:'vue',astro:'astro',mjs:'javascript',cts:'typescript',mts:'typescript'})[path.split('.').at(-1)];
function add(framework,path,source,origin){if(!framework||typeof path!=='string'||typeof source!=='string'||!language(path))return;const key=JSON.stringify([framework,path,source]);if(seen.has(key))return;seen.add(key);const expected=getFrameworkResolver(framework).extract(path,source);for(const node of expected.nodes)delete node.updatedAt;cases.push({id:`framework-${cases.length}`,framework,path,source,language:language(path),origin,expected});}
for(const [filename,framework]of [['fabric-view.test.ts','fabric-view'],['expo-modules.test.ts','expo-modules'],['frameworks.test.ts',null],['flask-url-rules.test.ts','flask'],['goframe.test.ts','goframe'],['spring-route-handlers.test.ts','spring'],['vapor-route-handlers.test.ts','vapor'],['vapor-closure-route-body.test.ts','vapor'],['vapor-closure-routes.test.ts','vapor'],['rust-route-handler-line.test.ts','rust']]){
 const tree=ts.createSourceFile(filename,readFileSync(join(root,'__tests__',filename),'utf8'),ts.ScriptTarget.Latest,true);
 function literal(node,depth=0){if(!node||depth>10)return undefined;if(ts.isStringLiteral(node)||ts.isNoSubstitutionTemplateLiteral(node))return node.text;if(ts.isIdentifier(node)){for(let block=node.parent;block;block=block.parent){for(const statement of block.statements??[]){if(statement.pos>node.pos)continue;if(ts.isVariableStatement(statement))for(const declaration of statement.declarationList.declarations)if(declaration.name.getText(tree)===node.text)return literal(declaration.initializer,depth+1);}}}if(ts.isBinaryExpression(node)&&node.operatorToken.kind===ts.SyntaxKind.PlusToken){const a=literal(node.left,depth+1),b=literal(node.right,depth+1);if(typeof a==='string'&&typeof b==='string')return a+b;}}
 function visit(node){if(framework&&ts.isPropertyAssignment(node))add(framework,literal(node.name),literal(node.initializer),filename);if(ts.isCallExpression(node)){const name=node.expression.getText(tree).replace(/!$/, '');if(name.endsWith('.extract'))add(framework??({springResolver:'spring',vaporResolver:'vapor',rustResolver:'rust',goResolver:'go',djangoResolver:'django',flaskResolver:'flask',fastapiResolver:'fastapi'})[name.split('.')[0]],literal(node.arguments[0]),literal(node.arguments[1]),filename);if(name==='write')add(framework,literal(node.arguments[0]),literal(node.arguments[1]),filename);if(name==='fs.writeFileSync'){const path=node.arguments[0];if(ts.isCallExpression(path)){const parts=path.arguments.slice(1).map(p=>literal(p));if(parts.length&&parts.every(p=>p!==undefined))add(framework,parts.join('/'),literal(node.arguments[1]),filename);}}}ts.forEachChild(node,visit);}visit(tree);
}
add('fabric-view','Unicode.tsx',"// 😀\r\nexport interface NativeProps {\r\n  caption?: string;\r\n  skip?: () => void;\r\n}\r\nconst emoji = '😀'; export default codegenNativeComponent<NativeProps>('Café');\r\nexport default codegenNativeComponent<NativeProps>('View');\r\n",'unicode and invalid identifier');
add('fabric-view','Props.kt','class RCTDemoViewManager : ViewManager() {\n @ReactProp(name = "café😀") fun setValue() {}\n}\n','unicode property');
add('expo-modules','Unicode.swift','class Demo: Module {\n func definition() { Name("Demo"); let emoji = "😀"; AsyncFunction("run") {} }\n}\n','inline unicode');
add('expo-modules','Duplicate.kt','class Demo: Module {\n fun definition() { Function("run") {}; Function("run") {}; Constants("values") {} }\n}\n','same-line duplicates');
for(const [framework,source]of [
 ['django',`# path('/fake', hidden)
urlpatterns = [path('café😀/', views.Home.as_view()), path('api/', include('app.urls')), re_path(r'^x/$', views.x)]
router.register(r'^items/$', ItemViewSet)
router.register('not', Model)
`],
 ['flask',`@app.route('', methods=('POST', 'GET'))
@login_required
async def run(): pass
api.add_resource(View, '/one', '/two')
bp.add_url_rule('/x', None, ns.call, methods=['GET', 'POST'])
register_view(bp, routes=['/a', '/b'], view_func=View.as_view('name'))
`],
 ['fastapi',`# @app.get('/fake')
@app.get('/café😀')
async def view(): pass
@router.post('')
def make(): pass
`],
 ['flask',`bp.add_url_rule(rule='/x', view_func=Call.as_view('x'), methods=[])
helper(paths=['/one', '/two'], view_func=ns.fun)
`],
])add(framework,'app.py',source,'Unicode, decorator and imperative route edges');
for(const [path,source]of [
 ['application.yml','app:\n  cache-list: "ready"\n  cache_name: x\nother: { a: 1 }\n'],
 ['application-dev.properties','app.cache-list=development\n# ignored=value\nescaped\\:key:value\n'],
 ['Settings.java','@ConfigurationProperties(prefix="app.cache")\nclass Settings { @Value("${app.cache-list:default}") String cache; }\n'],
 ['Controller.kt','@RequestMapping("/items")\nclass Controller {\n @GetMapping(path = ["/{id}", "/all"], produces = ["application/json"])\n @get:Example("test")\n suspend fun <T> run(): T {}\n}\n'],
])add('spring',path,source,'configuration keys and annotated Kotlin routes');
for(const framework of ['nuxt','astro']) {
 const paths = framework==='nuxt' ? ['pages/index.vue','pages/blog/index.vue','pages/[id].vue','pages/[[id]].vue','pages/[...slug].vue','app/pages/a/[[b]]/c.vue','pages/_hidden.vue','server/api/index.ts','server/api/[id].get.ts','server/api/[...slug].js','server/api/blog/index.ts','middleware/auth.global.ts','middleware/nested/index.ts','pages/middleware/index.vue','app\\pages\\x.vue','components/index.vue','pages/index.ts'] : ['src/pages/index.astro','src/pages/blog/index.astro','src/pages/[id].astro','src/pages/[...slug].astro','src/pages/_ignored.astro','src/pages/a/_ignored/index.astro','src/pages/foo.config.ts','src/pages/api/index.ts','src/pages/api/post.js','app/src/pages/blog/index.astro','src\\pages\\about.astro','pages/index.astro'];
 for(const path of paths)add(framework,path,path.endsWith('.vue')?'<template><div /></template>':path.endsWith('.astro')?'<div />':"export const GET = () => 1;\nexport async function POST() {}\nexport function ALL() {}\n",'file routes and endpoint handlers');
 add(framework,framework==='nuxt'?'pages/😀[id].vue':'src/pages/😀[id].astro','', 'Unicode file routes');
}
const {declaredDependencies}=require('./dist/resolution/frameworks/package-deps.js');
const dependencies=[];
function dependencyCase(name,files,manifests){const context={getAllFiles:()=>files,fileExists:path=>Object.hasOwn(manifests,path),readFile:path=>manifests[path]??null};dependencies.push({name,files,manifests,expected:[...declaredDependencies(context)].sort()});}
for(const source of ['null','[]','"not an object"','{bad','{"dependencies":{"next":"x"},"devDependencies":{"expo-router":"x"},"peerDependencies":{"react":"x"},"optionalDependencies":{"ignored":"x"}}','{"dependencies":["react","next"]}'])dependencyCase('root-'+dependencies.length,[],{'package.json':source});
dependencyCase('shallow-and-deep',['frontend/app.ts','apps/web/src/app.vue','src/Web/ClientApp/ui.ts','src/Web/Server/server.java','a/node_modules-extra/code.ts'],{'frontend/package.json':'{"dependencies":{"react":"x"}}','apps/web/package.json':'{"devDependencies":{"next":"x"}}','src/Web/ClientApp/package.json':'{"peerDependencies":{"@angular/router":"x"}}','src/Web/Server/package.json':'{"dependencies":{"server-only":"x"}}','a/node_modules-extra/package.json':'{"dependencies":{"excluded":"x"}}'});
for(const count of [23,24,25,193]){const files=[],manifests={};for(let i=0;i<count;i++){files.push(`apps/member${i}/index.ts`);manifests[`apps/member${i}/package.json`]=JSON.stringify({dependencies:{[`package${i}`]:'x'}});}dependencyCase('manifest-cap-'+count,files,manifests);}
{const files=[],manifests={};for(let i=0;i<23;i++){files.push(`p${i}/index.ts`);manifests[`p${i}/package.json`]=JSON.stringify({dependencies:{[`p${i}`]:'x'}});}for(const dir of ['a/b/\uE000','a/b/\u{10000}']){files.push(dir+'/main.ts');manifests[dir+'/package.json']=JSON.stringify({dependencies:{[dir]:'x'}});}dependencyCase('UTF16-deep-order',files,manifests);}
const destination=dirname(fileURLToPath(import.meta.url));writeFileSync(join(destination,'cases.json'),JSON.stringify({revision,cases,dependencies},null,2)+'\n');writeFileSync(join(destination,'LICENSE'),readFileSync(join(root,'LICENSE')));console.log(cases.length);
