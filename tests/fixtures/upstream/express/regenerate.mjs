// Development-only independent extraction and persistent project records.
import {execFileSync} from 'node:child_process';
import {createRequire} from 'node:module';
import {readFileSync,writeFileSync,mkdirSync,mkdtempSync,rmSync} from 'node:fs';
import {dirname,join,resolve} from 'node:path';
import {tmpdir} from 'node:os';
import {fileURLToPath} from 'node:url';
const root=resolve(process.argv[2]),revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('Wrong reference revision');
const require=createRequire(join(root,'package.json')),ts=require('typescript');
const {expressResolver:resolver}=require('./dist/resolution/frameworks/express.js');
const Project=Object.values(require('./dist/index.js')).find(value=>typeof value==='function'&&typeof value.init==='function'&&typeof value.open==='function');
const strip=node=>{const copy={...node};delete copy.updatedAt;return copy;};
const cases=[],seen=new Set(),projects=[];
function add(path,source,origin){const key=JSON.stringify([path,source]);if(seen.has(key))return;seen.add(key);const extracted=resolver.extract(path,source);cases.push({path,source,origin,expected:{nodes:extracted.nodes.map(strip),references:extracted.references}});}
for(const file of ['frameworks.test.ts','express-inline-function-handler.test.ts','express-heuristic-same-file.test.ts']){
 const tree=ts.createSourceFile(file,readFileSync(join(root,'__tests__',file),'utf8'),ts.ScriptTarget.Latest,true);const files={};
 function visit(node){if(ts.isNoSubstitutionTemplateLiteral(node)||ts.isStringLiteral(node)){
  if(/\b(?:app|router)\s*\.(get|post|put|patch|delete|use|route|all)\s*\(/.test(node.text))add(node.text.includes('<')?'server.tsx':'routes.ts',node.text,file);
  if(ts.isPropertyAssignment(node.parent)&&node.parent.initializer===node&&ts.isStringLiteral(node.parent.name)&&/\.[cm]?[jt]sx?$/.test(node.parent.name.text))files[node.parent.name.text]=node.text;
 }ts.forEachChild(node,visit);}visit(tree);
 if(file!=='frameworks.test.ts'){files['package.json']='{"dependencies":{"express":"*"}}';projects.push({name:file.replace('.test.ts',''),files});}
}
for(const path of ['routes.ts','routes.tsx','routes.js','routes.mjs','routes.cjs','routes.jsx','routes.cts','routes.mts'])add(path,"// 😀\r\napp.get('/café😀', (req, res) => { const text='😀'; svc.run(); res.status(200).json({text}); });\r\n",'UTF16 and extension applicability');
for(const source of ["router.route('/x')\n .get(validate, [before, handler], {cache: true})\n .post(function (req,res) { helper(); res.send('ok'); });", "app.use('skip', skip); app.get('/incomplete', (req,res) => broken( ", "app.get('/x', withErrors(function fail(e){report(e);}, async (req,res)=>{load();res.json('ok')}));", "app.get('/x', withErrors(function fail(e){report(e);}, ctrl.list));", "app.get('/x', wrap<Req, Result<{key:number}>>(async (req,res)=>{store();}));"] )add('routes.ts',source,'handler structure boundaries');
const base={'package.json':'{"dependencies":{"express":"*"}}','routes.ts':"export const router={}; function list() {}\nrouter.get('/items', list);\nrouter.post('/',()=>{list();});\n"};
for(const [name,main,extra]of [
 ['mount-esm',"import {router} from './routes'; const app={}; app.use('/api',router);",{}],
 ['mount-require',"const app={}; app.use('/api/',require('./routes').router);",{}],
 ['mount-ambiguous',"import {router} from './routes'; const app={}; app.use('/one',router); app.use('/two',router);",{}],
 ['mount-composed',"import {router} from './api'; const app={}; app.use('/v1',router);",{'api.ts':"import {router as items} from './routes'; export const router={}; router.use('/api',items);"}],
 ['mount-options',"import {router} from './routes'; const app={}; app.use('/api',router,{option:true});",{}],
 ['mount-alias-config',"import {router} from '#routes'; const app={}; app.use('/api',router);",{'tsconfig.json':'{"extends":"./config/paths.json"}','config/paths.json':'{"compilerOptions":{"baseUrl":"..","paths":{"#routes":["routes"]}}}'}],
 ['invalid-workspace-export',"import {router} from 'router-package'; const app={}; app.use('/api',router);",{'package.json':'{"dependencies":{"express":"*"},"workspaces":["packages/*"]}','packages/routes/package.json':'{"name":"router-package","exports":"../../routes.ts"}'}]
])projects.push({name,files:{...base,'main.ts':main,...extra}});
projects.find(project=>project.name==='invalid-workspace-export').explicit_uncertainty=true;
projects.push({name:'mount-workspace',files:{'package.json':'{"dependencies":{"express":"*"},"workspaces":["packages/*"]}','main.ts':"import {router} from 'router-package'; const app={}; app.use('/api',router);",'packages/routes/package.json':'{"name":"router-package","exports":"./index.ts"}','packages/routes/index.ts':base['routes.ts']}});
for(const project of projects){const dir=mkdtempSync(join(tmpdir(),'prometeu-express-'));let indexed;try{
 for(const [path,source]of Object.entries(project.files)){mkdirSync(dirname(join(dir,path)),{recursive:true});writeFileSync(join(dir,path),source);}
 indexed=await Project.init(dir,{silent:true});const result=await indexed.indexAll();if(!result.success)throw Error('Index failed');
 project.routes=indexed.queries.getNodesByKind('route').map(node=>({node:strip(node),targets:indexed.queries.getOutgoingEdges(node.id).map(edge=>({kind:edge.kind,node:strip(indexed.getNode(edge.target)),metadata:edge.metadata}))}));
 console.log(project.name,project.routes.length);
}finally{indexed?.close();rmSync(dir,{recursive:true,force:true});}}
const nodes=[
 ['foreign','validate','function','other/middleware/validate.ts'],['local','validate','function','app/user.ts'],['base','auth','constant','app/user.ts'],['base-foreign','auth','function','other/middleware/auth.ts'],['import','cors','import','app/user.ts'],['class','UserController','class','app/user.ts'],['method','show','method','app/user.ts'],['foreign-method','show','method','other/user.ts'],['service','run','function','app/user.ts']
].map(([id,name,kind,filePath])=>({id,name,kind,filePath,qualifiedName:`${filePath}::${name}`,startLine:1,endLine:1,startColumn:0,endColumn:1,language:'typescript',updatedAt:0}));
const context={getNodesByName:name=>nodes.filter(node=>node.name===name),getNodesInFile:path=>nodes.filter(node=>node.filePath===path),getNodeById:id=>nodes.find(node=>node.id===id)};
const resolutions=[];
for(const filePath of ['app/user.ts','other/user.ts','unknown.ts'])for(const referenceName of ['validate','authMiddleware','cors','UserController.show','userService.run','userHelper.run','userUtil.run','userUtils.run','UserController.missing','otherService.run']){
 const ref={fromNodeId:'caller',referenceName,referenceKind:'calls',line:1,column:0,filePath,language:'typescript'};const result=resolver.resolve(ref,context);resolutions.push({reference:ref,expected:result?{id:result.targetNodeId,confidence:result.confidence}:null});}
writeFileSync(join(dirname(fileURLToPath(import.meta.url)),'cases.json'),JSON.stringify({revision,cases,projects,resolver_nodes:nodes.map(strip),resolutions},null,2)+'\n');
console.log(cases.length,'extractions',resolutions.length,'resolutions');
