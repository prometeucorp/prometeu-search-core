// Independent per-file frontend declarations from the pinned reference modules.
import {execFileSync} from 'node:child_process';
import {createRequire} from 'node:module';
import {readFileSync,writeFileSync} from 'node:fs';
import {dirname,join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {runInNewContext} from 'node:vm';
const revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
const upstream=resolve(process.argv[2]??'');
if(!process.argv[2]||execFileSync('git',['-C',upstream,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('Pass the pinned prepared reference checkout.');
const require=createRequire(join(upstream,'package.json'));
const frameworks={svelte:require('./dist/resolution/frameworks/svelte.js').svelteResolver,nextjs:require('./dist/resolution/frameworks/nextjs.js').nextjsResolver,'expo-router':require('./dist/resolution/frameworks/expo-router.js').expoRouterResolver};
const paths=[
 'app/index.tsx','src/app/(group)/settings.tsx','packages/mobile/app/(tabs)/index.ios.tsx',
 'app/posts/[id].tsx','app/docs/[...slug].tsx','app/[[optional]].ts','app/_layout.tsx',
 'app/+not-found.tsx','app/+html.tsx','app/types.d.ts','app/User.test.tsx','app/User.stories.tsx',
 'app/__tests__/index.tsx','app/__mocks__/index.tsx','app/(group)/index+api.ts',
 'app/posts/[id]+api.ts','apps/mobile/src/app/images/[...path]+api.js',
 'app/page.tsx','src/app/(group)/users/[id]/page.tsx','apps/web/src/app/blog/[...path]/page.tsx',
 'app/docs/[[...path]]/page.mdx','app/@modal/page.tsx','app/(.)photo/page.tsx','app/(...)/page.tsx',
 'app/layout.tsx','app/api/users/route.ts','app/route.js','pages/index.tsx','pages/about.tsx',
 'src/pages/blog/[slug].jsx','apps/web/pages/api/[...route].ts','pages/_app.tsx','pages/a.d.ts',
 'pages/a.spec.ts','pages/a.config.ts','__mocks__/app/page.tsx','node_modules/demo/app/page.tsx',
 'src/routes/+page.svelte','src/routes/(group)/blog/[slug=word]/+page.svelte',
 'src/routes/[[lang]]/profile/[[optional]]/+page.svelte','apps/site/src/routes/[...rest]/+page.svelte',
 'src/routes/+layout.svelte','src/routes/+error.svelte','src/routes/+server.ts',
 'routes/+page.svelte','src\\routes\\blog\\[slug]\\+page.svelte',
];
const sources=[
 '/* 🌍 comment */\nexport default function Screen() { return null; }\n',
 'export async function GET(req) {}\nexport const POST = () => {};\nexport function GET() {}\n// export const DELETE = ignored;\n',
 'function Screen() {}\nexport default React.memo(Screen);\n',
 'class Screen {}\nexport {Screen as default};\n',
 'export default () => null;\n',
 'export default class Screen {}\n',
];
const cases=[];
for(const [framework,resolver] of Object.entries(frameworks))for(const path of paths)for(const [i,source] of sources.entries()){
 if(framework==='svelte'&&i>0)continue;
 const expected=resolver.extract(path,source);for(const node of expected.nodes)delete node.updatedAt;
 cases.push({id:`${framework}:${path}:${i}`,framework,path,source,expected});
}
const target=dirname(fileURLToPath(import.meta.url));
writeFileSync(join(target,'cases.json'),JSON.stringify({revision,cases},null,2)+'\n');
writeFileSync(join(target,'LICENSE'),readFileSync(join(upstream,'LICENSE')));
console.log(`${cases.length} complete frontend extraction cases`);

const ts=require('typescript');
const tanstack=require('./dist/resolution/frameworks/tanstack-router.js');
const tanSource=readFileSync(join(upstream,'__tests__/tanstack-router.test.ts'),'utf8');
const tanTree=ts.createSourceFile('tanstack-router.test.ts',tanSource,ts.ScriptTarget.Latest,true);
const pure=tanTree.statements.filter(statement=>!ts.isImportDeclaration(statement)&&statement.getStart(tanTree)<tanSource.indexOf("describe('tanstack: a routed app end to end'")).map(statement=>statement.getText(tanTree)).join('\n');
const tanCases={paths:[],parse:[],destinations:[],extraction:[]};
const assertion=new Proxy(()=>assertion,{get:()=>assertion});
const it=(_name,fn)=>fn();it.each=rows=>(_name,fn)=>rows.forEach(row=>fn(...(Array.isArray(row)?row:[row])));
runInNewContext(ts.transpileModule(pure,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.None}}).outputText,{
 describe:(_name,fn)=>fn(),it,expect:()=>assertion,tanstackNavVerb:tanstack.tanstackNavVerb,
 tanstackPath(source){const expected=tanstack.tanstackPath(source);tanCases.paths.push({source,expected});return expected;},
 parseTanstackRoutes(source){const expected=tanstack.parseTanstackRoutes(source);tanCases.parse.push({source,expected});return expected;},
 tanstackDestination(source){const expected=tanstack.tanstackDestination(source);tanCases.destinations.push({source,expected});return expected;},
});
const tanSources=[...new Set(tanCases.parse.map(row=>row.source)),
 "/* 🌍 */\r\nexport const Route = createLazyFileRoute('/(group)/users/$id')({ component: X }).lazy(() => import('./Lazy.tsx'));\r\n",
 "const root = createRootRoute({component:Root});\nconst a = createRoute({path:'a',getParentRoute:()=>b});\nconst b = createRoute({path:'b',getParentRoute:()=>a});\n",
 "const route=createRoute({path:'/first',path:'/second',component: First,component:Second});\n",
 "createFileRoute('/a')({component: First});\ncreateFileRoute('/a/')({component:Second});\n",
 "createFileRoute('/missing')({component: X\n",
];
for(const source of tanSources)for(const path of ['src/routes/posts.tsx','src/routes/posts.route.tsx','src/routes/posts.index.ts','src/routes/root.jsx','src/routes/a.mts']){
 const expected=tanstack.tanstackRouterResolver.extract(path,source);for(const node of expected.nodes)delete node.updatedAt;
 tanCases.extraction.push({path,source,expected});
}
writeFileSync(join(target,'tanstack.json'),JSON.stringify({revision,...tanCases},null,2)+'\n');
console.log(`${tanCases.extraction.length} complete TanStack extraction cases`);

const vue=require('./dist/resolution/frameworks/vue-router.js');
const vueSource=readFileSync(join(upstream,'__tests__/vue-router.test.ts'),'utf8');
const vueTree=ts.createSourceFile('vue-router.test.ts',vueSource,ts.ScriptTarget.Latest,true);
const vuePure=vueTree.statements.filter(statement=>!ts.isImportDeclaration(statement)&&statement.getStart(vueTree)<vueSource.indexOf("describe('vue-router: a routed app end to end'")).map(statement=>statement.getText(vueTree)).join('\n');
const vueCases={parse:[],names:[],extraction:[]};
runInNewContext(ts.transpileModule(vuePure,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.None}}).outputText,{
 describe:(_name,fn)=>fn(),it,expect:()=>assertion,vueNavVerb:vue.vueNavVerb,
 parseVueRoutes(source,file=''){const expected=vue.parseVueRoutes(source,file);vueCases.parse.push({source,file,expected});return expected;},
 routeNameInExpression(source){const expected=vue.routeNameInExpression(source);vueCases.names.push({source,expected});return expected;},
});
const vueSources=[...new Set(vueCases.parse.map(row=>row.source)),
 "import type {RouteRecordRaw} from 'vue-router';\nconst routes: RouteRecordRaw[] = [{path:'/table',component: Layout,redirect:'/table/main',children:[{path:'main',name:'Main',component:()=>import('#/views/main/index.vue')},{path:'',component:Index}]}];\n",
 "export const constantRoutes = [{path:'/',component:Root,children:[{path:'users/',name:'users',component:Users,children:[{path:'/absolute',component:Other}]}]}];\n",
 "const tableRouter = {path:'/same',name:'first',component:First}; const otherRouter = {path:'/same',name:'second',component:Second};\n",
 "const routes = [{path:'/one',path:'/two',name:'one',name:'two',component:First,component:Second}];\n",
 "/* 🌍 */\r\nconst routes = [{path:'/a',component:()=>import('./A.vue'),children:[{path:'b',component:B}]}];\r\n",
 "const routes = [{path:'/broken', component:()=>import('./A.vue')\n",
];
for(const source of vueSources)for(const path of ['src/router/index.js','src/routes/modules/route.ts','src/router.ts','src/plain.ts','src/router/View.vue']){
 const expected=vue.vueRouterResolver.extract(path,source);for(const node of expected.nodes)delete node.updatedAt;
 vueCases.extraction.push({path,source,expected});
}
writeFileSync(join(target,'vue-router.json'),JSON.stringify({revision,...vueCases},null,2)+'\n');
console.log(`${vueCases.extraction.length} complete Vue router extraction cases`);

const jsx=require('./dist/resolution/jsx-code.js');
const jsxSource=readFileSync(join(upstream,'__tests__/jsx-child-disambiguation.test.ts'),'utf8');
const jsxTree=ts.createSourceFile('jsx-child-disambiguation.test.ts',jsxSource,ts.ScriptTarget.Latest,true);
const jsxCases=[];
function addJsx(source){
 const expected=jsx.jsxCode(source);let at=0,byteExpected=expected===null?null:'';
 if(expected!==null)for(const ch of source){const part=expected.slice(at,at+ch.length);byteExpected+=part===ch?ch:(ch==='\r'||ch==='\n'?ch:' '.repeat(Buffer.byteLength(ch)));at+=ch.length;}
 jsxCases.push({source,expected,byteExpected});return expected;
}
const jsxPure=jsxTree.statements.filter(statement=>ts.isExpressionStatement(statement)&&ts.isCallExpression(statement.expression)&&statement.expression.expression.getText(jsxTree)==='describe'&&statement.expression.arguments[0]?.text==='jsxCode').map(statement=>statement.getText(jsxTree)).join('\n');
runInNewContext(ts.transpileModule(jsxPure,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.None}}).outputText,{describe:(_name,fn)=>fn(),it,expect:()=>assertion,jsxCode:addJsx});
function jsxLiteral(node){
 if(!node)return undefined;
 if(ts.isStringLiteral(node)||ts.isNoSubstitutionTemplateLiteral(node))return node.text;
 if(ts.isBinaryExpression(node)&&node.operatorToken.kind===ts.SyntaxKind.PlusToken){const a=jsxLiteral(node.left),b=jsxLiteral(node.right);if(a!==undefined&&b!==undefined)return a+b;}
 return undefined;
}
function jsxWrites(node){
 if(ts.isCallExpression(node)&&node.expression.getText(jsxTree)==='write'){const source=jsxLiteral(node.arguments[1]);if(source!==undefined)addJsx(source);}
 ts.forEachChild(node,jsxWrites);
}
jsxWrites(jsxTree);
for(const source of [
 '/* 🌍 */\r\nconst text = "🌍 <No/>";\r\nconst el = <Ärea title="🌍"><Real/></Ärea>;\r\n',
 'const x = `🌍${<A value="🌍"/>} and ${`nested${<B/>}`}`;\n',
 'const x = <A title="line\r\n<No/>"><B/>don\'t</A>;\n',
 'const id = <T,>(value:T)=>value;\nconst el = <List<Item>><Child/></List>;\n',
 'const x = /[\\/]<No\/>/g; const y = <Yes/>;\n',
 'const x = <A><B/></C>;\n',
])addJsx(source);
writeFileSync(join(target,'jsx-code.json'),JSON.stringify({revision,cases:jsxCases},null,2)+'\n');
console.log(`${jsxCases.length} original JSX scanner cases`);

// Exercise the original private loader reader through its actual route scanner,
// and expose unchanged helper functions for independent value-reader snapshots.
const loaderCaptured=[];
const reactSource=readFileSync(join(upstream,'src/resolution/frameworks/react.ts'),'utf8').replace('function lazyRouteReference(', 'function lazyRouteReferenceOriginal(')+`
function lazyRouteReference(loader, elementTag) {
 const tags=[];
 const expected=lazyRouteReferenceOriginal(loader, source=>{const value=elementTag(source);tags.push({source,expected:value??null});return value;});
 if(loader!==undefined)__capture({source:loader,expected:expected??null,tags});
 return expected;
}
export const helperOracle={loaderValue,loadedPage,functionParts,loaderOf,wrappedName,topLevelItems,objectEntries,initializerAt,statementEnd};
`;
const reactModule={exports:{}};
new Function('require','module','exports','__capture',ts.transpileModule(reactSource,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText)(createRequire(join(upstream,'dist/resolution/frameworks/react.js')),reactModule,reactModule.exports,row=>loaderCaptured.push(row));
const helpers=reactModule.exports.helperOracle;
const candidates=new Set();
const originalName=require('./package.json').name.split('/').at(-1);
const testNames=execFileSync('rg',['--files',join(upstream,'__tests__')],{encoding:'utf8'}).trim().split('\n').filter(path=>/(?:react|jsx|lazy|router|const-alias)/i.test(path)&&path.endsWith('.ts'));
for(const path of testNames){
 const tree=ts.createSourceFile(path,readFileSync(path,'utf8'),ts.ScriptTarget.Latest,true);
 function visit(node){const literal=jsxLiteral(node);if(literal!==undefined&&/\b(?:lazy|import|Component|Route|memo|forwardRef|styled|observer|loadable)\b/.test(literal))candidates.add(literal.replaceAll(originalName,'prometeu'));ts.forEachChild(node,visit);}
 visit(tree);
}
const helperInputs=new Set();
for(const source of candidates){
 reactModule.exports.reactResolver.extract('src/routes.tsx',source);
 const tree=ts.createSourceFile('embedded.tsx',source,ts.ScriptTarget.Latest,true,ts.ScriptKind.TSX);
 function visit(node){
  if(ts.isArrowFunction(node)||ts.isFunctionExpression(node)||ts.isCallExpression(node)||ts.isConditionalExpression(node)||ts.isObjectLiteralExpression(node))helperInputs.add(node.getText(tree));
  ts.forEachChild(node,visit);
 }
 visit(tree);
}
const focused=[
 `async () => {const [{Page},second] = await Promise.all([import('./x'),import('./y')]);return {Component:Page};}`,
 `() => import('./x').then(({Page:Renamed})=>({Component:Renamed}))`,
 `async () => {const module=await import('./x');const Alias=module;return {...Alias};}`,
 `() => ({Component:Outlet,loader:load})`, `() => ({loader:load})`,
 `() => ({element:<Wrap><Page/></Wrap>})`,
 `() => import('./x').then(module=>({default:module.Page}))`,
 `() => import('./x').then(module=>module['Page'])`,
 `async () => {const m = await import('./x');const props = {Component:m.Page};return props;}`,
 `lazy(() => import('./x'))`, `Loadable({loader:async()=> (await import('./x')).Page})`,
 `IS_DEV ? React.lazy(()=>import('./x')) : null`,
 `React.memo(Page, compare) as FC`, `connect(a,b)(Page)`, `withTranslation()(withRouter(Page))`,
];
for(const source of focused)helperInputs.add(source);
function plain(value){return JSON.parse(JSON.stringify(value,(_key,item)=>item instanceof Map?Object.fromEntries(item):item));}
function loadedShape(value){if(value?.kind==='object')return {kind:'object',text:value.text};if(value?.kind==='array')return {kind:'array',items:value.items.map(loadedShape)};return value;}
const helperCases=[...helperInputs].map(source=>{
 const value=helpers.loaderValue(source,new Map());
 const entries=helpers.objectEntries(source);
 return {source,function:helpers.functionParts(source),value:loadedShape(value),page:helpers.loadedPage(value),loader:helpers.loaderOf(source),wrapped:helpers.wrappedName(source),items:helpers.topLevelItems(source),entries:plain(entries),initializer:helpers.initializerAt(source,0),end:helpers.statementEnd(source,0),commaEnd:helpers.statementEnd(source,0,true)};
});
const uniqueLoads=[...new Map(loaderCaptured.map(row=>[JSON.stringify(row),row])).values()];
writeFileSync(join(target,'react-loader.json'),JSON.stringify({revision,cases:helperCases,lazy:uniqueLoads},null,2)+'\n');
console.log(`${helperCases.length} original React loader helper inputs, ${uniqueLoads.length} actual lazy route calls`);

const exportReader=require('./dist/resolution/alias-binding.js');
const reexportReader=require('./dist/resolution/import-resolver.js');
const exportInputs=[...candidates,
 'export { Card as default };\nexport type { Card } from "./types";\n',
 'export { "named value" as Page } from "./file"; export * as ns from "./other";\n',
 'export type*from "./types"; export {type Foo, type as Type} from "./other";\n',
 'exports.Card = function RealCard() {};\nmodule.exports = {Page, App: Root};\n',
];
writeFileSync(join(target,'react-exports.json'),JSON.stringify({revision,cases:exportInputs.map(source=>({source,aliases:exportReader.extractLocalExportAliases(source),default:exportReader.defaultExportBinding(source),reexports:reexportReader.extractReExports(source,'tsx')}))},null,2)+'\n');
const callbackSource=readFileSync(join(upstream,'src/resolution/callback-synthesizer.ts'),'utf8')+'\nexport const jsxHelpers={destructures,topLevelPatterns,patternsBind};';
const callbackModule={exports:{}};
new Function('require','module','exports',ts.transpileModule(callbackSource,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText)(createRequire(join(upstream,'dist/resolution/callback-synthesizer.js')),callbackModule,callbackModule.exports);
const scopes=callbackModule.exports.jsxHelpers;
const nameMatcher=require('./dist/resolution/name-matcher.js');
const scopeCases=[];
for(const original of jsxCases){
 const source=original.expected??original.source;const names=[...new Set([...source.matchAll(/<([A-Z][A-Za-z0-9_]*)(?=[\s/><])/g)].map(match=>match[1]))];
 for(const name of names){const patterns=scopes.topLevelPatterns(source);const binds=nameMatcher.jsCodeBindsName(name,{startLine:1,endLine:source.split('\n').length},'source.tsx',source.split('\n').length,{readFile:()=>source});scopeCases.push({source,name,patterns,top:scopes.patternsBind(source,patterns,name),destructures:scopes.destructures(source,name,source.length),binds});}
}
writeFileSync(join(target,'jsx-scope.json'),JSON.stringify({revision,cases:scopeCases},null,2)+'\n');
console.log(`${exportInputs.length} original export-reader inputs, ${scopeCases.length} original JSX scope cases`);

const reactCases=[];
for(const source of candidates)if(/\b(?:Route|createBrowserRouter|createHashRouter|createMemoryRouter|createRoutesFromElements|useRoutes)\b/.test(source)){
 for(const path of ['src/routes.tsx','src/routes.ts']){const expected=reactModule.exports.reactResolver.extract(path,source);for(const node of expected.nodes)delete node.updatedAt;reactCases.push({source,path,expected});}
}
const routeExposed=readFileSync(join(upstream,'src/resolution/frameworks/react.ts'),'utf8')+'\nexport const routeOracle={scanRoutes,elementTags};';const routeModule={exports:{}};
new Function('require','module','exports',ts.transpileModule(routeExposed,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText)(createRequire(join(upstream,'dist/resolution/frameworks/react.js')),routeModule,routeModule.exports);
function byteOffsets(value,source){if(Array.isArray(value))return value.map(item=>byteOffsets(item,source));if(value&&typeof value==='object')return Object.fromEntries(Object.entries(value).map(([key,item])=>[key,['at','literal'].includes(key)&&typeof item==='number'?Buffer.byteLength(source.slice(0,item)):byteOffsets(item,source)]));return value;}
const scanCases=[];
for(const source of new Set(reactCases.map(case_=>case_.source))){
 const scan=routeModule.exports.routeOracle.scanRoutes(source,true,{objects:true,uses:true});scanCases.push({source,expected:byteOffsets(scan,source)});
}
writeFileSync(join(target,'react-routes.json'),JSON.stringify({revision,extraction:reactCases,scans:scanCases},null,2)+'\n');
console.log(`${reactCases.length} original React extraction cases, ${scanCases.length} route scans`);
