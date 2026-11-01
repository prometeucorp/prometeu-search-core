// Development-only independent extraction and full project oracle.
import {execFileSync} from 'node:child_process';
import {createRequire} from 'node:module';
import {readFileSync,writeFileSync,mkdirSync,mkdtempSync,rmSync} from 'node:fs';
import {dirname,join,resolve} from 'node:path';
import {tmpdir} from 'node:os';
import {fileURLToPath} from 'node:url';
import vm from 'node:vm';
const root=resolve(process.argv[2]),revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('Wrong reference revision');
const require=createRequire(join(root,'package.json')),ts=require('typescript');
const {drupalResolver:resolver}=require('./dist/resolution/frameworks/drupal.js');
const {ReferenceResolver}=require('./dist/resolution/index.js');
const {extractFromSource}=require('./dist/extraction/tree-sitter.js');
const {initGrammars,loadAllGrammars}=require('./dist/extraction/grammars.js');await initGrammars();await loadAllGrammars();
const Project=Object.values(require('./dist/index.js')).find(value=>typeof value==='function'&&typeof value.init==='function'&&typeof value.open==='function');
const strip=node=>{const copy={...node};delete copy.updatedAt;return copy;};
const cases=[],seen=new Set(),projects=[],detections=[];
const claimNames=['hook_cron','drupal:service:x','drupal:class:a.php#A','\\Drupal\\shop\\Main','Main:run','Main::run','ns::Main::run',"container->get('shop.x').run","this->container->get('shop.x').run",'Drupal::service().run','\\Drupal::service().run','helper','comment.default','this->container->get().run'];
const claims=claimNames.map(name=>({name,expected:resolver.claimsReference(name)}));
function add(path,source,origin){const key=JSON.stringify([path,source]);if(seen.has(key))return;seen.add(key);const extracted=resolver.extract(path,source);cases.push({path,source,origin,expected:{nodes:extracted.nodes.map(strip),references:extracted.references}});}
for(const file of ['drupal.test.ts','drupal-services.test.ts']){
 const tree=ts.createSourceFile(file,readFileSync(join(root,'__tests__',file),'utf8'),ts.ScriptTarget.Latest,true);const files={};
 function visit(node){if(ts.isNoSubstitutionTemplateLiteral(node)||ts.isStringLiteral(node)){
  const source=node.text;
  if(source.includes('<?php')||source.includes('function '))for(const path of ['modules/mymodule/mymodule.module','src/Plugin/Block/Fixture.php','src/Hook/Fixture.php','tests/src/Plugin/Fixture.php','mymodule.module'])add(path,source,file);
  else if(/^services\s*:/m.test(source))add('modules/shop/shop.services.yml',source,file);
  else if(/^\s+path:\s/m.test(source))add('modules/mymodule/mymodule.routing.yml',source,file);
 }
 if(ts.isCallExpression(node)&&node.expression.getText(tree)==='write'&&node.arguments.length===2&&ts.isStringLiteral(node.arguments[0])){
   const path=node.arguments[0].text;if(!(path in files))files[path]=vm.runInNewContext(ts.transpile('('+node.arguments[1].getText(tree)+')'));
 }
 ts.forEachChild(node,visit);}visit(tree);
 if(Object.keys(files).length)projects.push({name:'original-services',files});
}
for(const [path,source] of [
 ['modules/shop/shop.routing.yml',"# 😀\r\nshop.first:\r\n  path: '/café😀'\r\n  defaults:\r\n    _controller: '\\Drupal\\shop\\Controller\\Main::show' # hi\r\n    _form: '\\Drupal\\shop\\Form\\Main'\r\n  methods: [get, POST, 'put']\r\nshop.empty:\n  path: ''\nshop.entity:\n  path: /entity\n  defaults:\n    _entity_form: node.edit\n"],
 ['modules/shop/shop.services.yml',"services:\n  'quoted:#id': { class: '\\Drupal\\shop\\Class', arguments: ['@a', '@@literal', '@?optional', '@a'] } # '@bad'\n  shop.self:\n    alias: '@shop.self'\n    arguments: ['@shop.self']\n  shop.other:\n    class: Plain\n    class: Repeated\n"],
 ['src/Plugin/Block/A.php',"<?php\n// 😀\n#[Block(id: 'a')]\n#[Block('a')]\n#[Hook(hook: 'cron', method: 'run')]\n/* trivia */ final readonly class A {\n  #[Hook('save')]\n  public static function &run() {}\n}\n"],
 ['src/Plugin/Block/A.php',"<?php\n/**\n * @Block(id = \"a\")\n */\n#[Block(id: 'a')]\nclass A {}\n"],
 ['src/Plugin/Block/A.php',"<?php\n#[Block(id: 'a'\nclass A {}\n"],
 ])add(path,source,'boundary and Unicode cases');
const routed={
 'composer.json':'{"require":{"drupal/core":"*"}}',
 'modules/shop/shop.info.yml':'name: Shop\ntype: module\n',
 'modules/shop/shop.routing.yml':"shop.show:\n  path: '/show'\n  defaults:\n    _controller: '\\Drupal\\shop\\Controller\\Main::show'\n  methods: [GET, POST]\nshop.fallback:\n  path: '/fallback'\n  defaults:\n    _controller: '\\Drupal\\shop\\Controller\\Main:missing'\nshop.form:\n  path: '/form'\n  defaults:\n    _form: '\\Drupal\\shop\\Form\\Main'\nshop.unknown:\n  path: '/unknown'\n  defaults:\n    _entity_form: node.edit\n",
 'modules/shop/src/Controller/Main.php':"<?php\nnamespace Drupal\\shop\\Controller;\nclass Main {\n  public function show() {}\n}\n",
 'modules/shop/src/Form/Main.php':"<?php\nnamespace Drupal\\shop\\Form;\nclass Main {}\n",
 'modules/shop/shop.module':"<?php\n/**\n * Implements hook_form_alter().\n */\nfunction shop_form_alter() {}\nfunction shop_cron() {}\n",
 'modules/another/another.module':"<?php\nfunction another_form_alter() {}\n",
 'core/form.api.php':"<?php\nfunction hook_form_alter() {}\n"
};
projects.push({name:'routes-and-hooks',files:routed});
const fallback={...routed};delete fallback['composer.json'];projects.push({name:'manifest-fallback',files:fallback});
const disabled={...routed,'composer.json':'{"require":{"other/package":"*"}}'};delete disabled['modules/shop/shop.info.yml'];projects.push({name:'detection-off',files:disabled});
for(const composer of [null,'{bad','null','{}','{"name":"drupal/shop"}','{"type":"drupal-module"}','{"require-dev":{"drupal/core":"*"}}','{"name":"other/shop","require":{"other/package":"*"}}'])for(const files of [[],['shop.info.yml'],['shop.info.yml','shop.routing.yml'],['shop.info.yml','shop.services.yml'],['shop.info.yml','shop.module']]){
 const manifests=composer===null?{}:{'composer.json':composer};detections.push({manifests,files,expected:resolver.detect({readFile:path=>manifests[path]??null,getAllFiles:()=>files})});
}
for(const project of projects){const dir=mkdtempSync(join(tmpdir(),'prometeu-drupal-'));let indexed;try{
 for(const [path,source]of Object.entries(project.files)){mkdirSync(dirname(join(dir,path)),{recursive:true});writeFileSync(join(dir,path),source);}
 indexed=await Project.init(dir,{silent:true});const report=await indexed.indexAll();if(!report.success)throw Error('Index failed');
 const queries=indexed.queries,context=new ReferenceResolver(dir,queries);context.initialize();
 const nodes=queries.getAllNodes(),edges=nodes.flatMap(node=>queries.getOutgoingEdges(node.id));
 project.nodes=nodes.map(strip);project.allEdges=edges;project.edges=edges.filter(edge=>edge.metadata?.framework==='drupal');
 project.superclasses=Object.fromEntries(nodes.filter(node=>node.kind==='class').map(node=>[node.id,(context.context.getSupertypeNodes?.(node.id)??[]).map(node=>node.id)]));
 project.resolutions=[];project.extracted=[];
 for(const [path,source]of Object.entries(project.files)){
  if(path.endsWith('.json'))continue;
  const extra=resolver.extract(path,source);project.extracted.push({path,expected:{nodes:extra.nodes.map(strip),references:extra.references}});
  const parsed=extractFromSource(path,source,path.endsWith('.yml')?'yaml':'php');
  const refs=[...parsed.unresolvedReferences.map(ref=>({...ref,filePath:path,language:parsed.nodes[0]?.language??'php'})),...extra.references];
  for(const reference of refs){const result=resolver.resolve(reference,context.context);project.resolutions.push({reference,expected:result?{id:result.targetNodeId,confidence:result.confidence}:null});}
 }
 for(const referenceName of ['drupal:service:shop.cart','drupal:fqcn:Drupal\\shop\\:Cart','drupal:fqcn:Drupal\\elsewhere\\:Prices','drupal:class:src/Cart.php#Cart','drupal:class:src/Cart.php#Missing','hook_form_alter','\\Drupal\\shop\\Controller\\Main:show','container->get(\'shop.cart\').clear','this->container->get(\'shop.cart\').clear','\\Drupal::service().total']){
  const reference={fromNodeId:'method:fixture:caller:1',referenceName,referenceKind:'calls',filePath:'src/Checkout.php',language:'php',line:5,column:0};const result=resolver.resolve(reference,context.context);project.resolutions.push({reference,expected:result?{id:result.targetNodeId,confidence:result.confidence}:null});
 }
 console.log(project.name,nodes.length,project.edges.length,project.resolutions.length);
}finally{indexed?.close();rmSync(dir,{recursive:true,force:true});}}
writeFileSync(join(dirname(fileURLToPath(import.meta.url)),'cases.json'),JSON.stringify({revision,cases,detections,claims,projects},null,2)+'\n');
console.log(cases.length,'extractions',detections.length,'detections');
