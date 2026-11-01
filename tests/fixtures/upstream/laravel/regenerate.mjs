// Development-only extraction and indexing oracle from the pinned reference.
import {execFileSync} from 'node:child_process';
import {createRequire} from 'node:module';
import {readFileSync,writeFileSync,mkdirSync,mkdtempSync,rmSync} from 'node:fs';
import {dirname,join,resolve} from 'node:path';
import {tmpdir} from 'node:os';
import {fileURLToPath} from 'node:url';
const root=resolve(process.argv[2]),revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('Wrong reference revision');
const require=createRequire(join(root,'package.json')),ts=require('typescript');
const {laravelResolver:resolver}=require('./dist/resolution/frameworks/laravel.js');
const Project=Object.values(require('./dist/index.js')).find(value=>typeof value==='function'&&typeof value.init==='function'&&typeof value.open==='function');
const {ReferenceResolver}=require('./dist/resolution/index.js');
const strip=node=>{const copy={...node};delete copy.updatedAt;return copy;};
const cases=[],seen=new Set(),projects=[];
function add(path,source,origin){const key=JSON.stringify([path,source]);if(seen.has(key))return;seen.add(key);const extracted=resolver.extract(path,source);cases.push({path,source,origin,expected:{nodes:extracted.nodes.map(strip),references:extracted.references}});}
function literal(node,tree,depth=0){if(!node||depth>20)return undefined;if(ts.isStringLiteral(node)||ts.isNoSubstitutionTemplateLiteral(node))return node.text;if(ts.isNumericLiteral(node))return Number(node.text);if(node.kind===ts.SyntaxKind.TrueKeyword)return true;if(node.kind===ts.SyntaxKind.FalseKeyword)return false;if(ts.isArrayLiteralExpression(node)){const values=node.elements.map(value=>literal(value,tree,depth+1));return values.every(value=>value!==undefined)?values:undefined;}if(ts.isObjectLiteralExpression(node)){const object={};for(const property of node.properties){if(ts.isSpreadAssignment(property)){const spread=literal(property.expression,tree,depth+1);if(!spread)return undefined;Object.assign(object,spread);continue;}if(ts.isShorthandPropertyAssignment(property)){const value=literal(property.name,tree,depth+1);if(value===undefined)return undefined;object[property.name.text]=value;continue;}if(!ts.isPropertyAssignment(property))return undefined;const key=ts.isIdentifier(property.name)?property.name.text:literal(ts.isComputedPropertyName(property.name)?property.name.expression:property.name,tree,depth+1);const value=literal(property.initializer,tree,depth+1);if(key===undefined||value===undefined)return undefined;object[key]=value;}return object;}if(ts.isBinaryExpression(node)&&node.operatorToken.kind===ts.SyntaxKind.PlusToken){const left=literal(node.left,tree,depth+1),right=literal(node.right,tree,depth+1);if(typeof left==='string'&&typeof right==='string')return left+right;}if(ts.isIdentifier(node)){for(let block=node.parent;block;block=block.parent){for(const statement of block.statements??[]){if(statement.pos>node.pos)continue;if(ts.isVariableStatement(statement))for(const declaration of statement.declarationList.declarations)if(declaration.name.getText(tree)===node.text)return literal(declaration.initializer,tree,depth+1);}}}if(ts.isCallExpression(node)&&node.expression.getText(tree)==='JSON.stringify'){const value=literal(node.arguments[0],tree,depth+1);if(value!==undefined)return JSON.stringify(value);}}
const base={'artisan':'#!/usr/bin/env php\n<?php\n','composer.json':'{"require":{"laravel/framework":"^11.0"}}','app/Http/Controllers/PageController.php':'<?php\nclass PageController { public function list() {} public function login() {} }\n'};
for(const file of ['frameworks.test.ts','laravel-route-paths.test.ts','laravel-controller-strings.test.ts']){
 const tree=ts.createSourceFile(file,readFileSync(join(root,'__tests__',file),'utf8'),ts.ScriptTarget.Latest,true);
 function visit(node){if((ts.isNoSubstitutionTemplateLiteral(node)||ts.isStringLiteral(node))&&/\bRoute::/.test(node.text))add('routes/web.php',node.text.startsWith('<?php')?node.text:'<?php\n'+node.text,file);
 if(ts.isCallExpression(node)&&node.expression.getText(tree)==='project'){const files=literal(node.arguments[0],tree);if(files)projects.push({name:`mount-${projects.length}`,origin:file,files:{...base,...files}});else throw Error('Uncaptured project '+node.getText(tree));}ts.forEachChild(node,visit);}visit(tree);
}
const controller=(namespace,name,methods)=>`<?php\nnamespace App\\Http\\Controllers\\${namespace};\nclass ${name} {\n${methods.map(method=>` public function ${method}() { return 1; }`).join('\n')}\n}\n`;
projects.push({name:'controller-strings',files:{...base,'routes/admin.php':"<?php\nRoute::get('uploads/{id}/inline', 'Common\\Uploads@inline');\nRoute::delete('uploads/{id}', 'Common\\Uploads@destroy');\nRoute::get('portal/uploads/{id}', 'Portal\\Uploads@inline');\nRoute::resource('companies', 'Common\\Companies', ['middleware'=>['dropzone']]);\n",'app/Http/Controllers/Common/Uploads.php':controller('Common','Uploads',['inline','destroy']),'app/Http/Controllers/Portal/Uploads.php':controller('Portal','Uploads',['inline']),'app/Http/Controllers/Common/Companies.php':controller('Common','Companies',['index','store'])}});
projects.push({name:'models-and-module-controllers',files:{...base,'routes/web.php':"<?php\nRoute::get('/settings', 'Modules\\Offline\\Http\\Controllers\\Settings@show');\nRoute::resource('settings','Modules\\Offline\\Http\\Controllers\\Settings');\n",'Modules/Offline/Http/Controllers/Settings.php':controller('Modules','Settings',['show']),'app/Models/User.php':'<?php class User { public static function find() {} }','app/Legacy.php':'<?php class Legacy {}','calls.php':'<?php function caller() { User::find(); Legacy::all(); }'}});
for(const [path,source]of [
 ['routes/web.php',"<?php /* 😀 */ Route::prefix('café😀')->group(function(){Route::get('/é', [X::class,'show']);});"],
 ['routes/web.php',"<?php Route::prefix(config('root'))->group(fn()=>Route::resource('/x', X::class));"],
 ['routes/web.php',"<?php Route::prefix('a')->prefix('b')->group('routes/other.php'); Route::any('x','\\App\\Http\\Controllers\\Foo@bar');"],
 ['routes/web.PHP',"Route::get('/x','Foo@bar');"],
 ['routes/web.php',"<?php Route::get('broken', [X::class,'run']; Route::group(['prefix'=>'x'], function(){"],
])add(path,source,'source boundaries and literal forms');
for(const project of projects){const dir=mkdtempSync(join(tmpdir(),'prometeu-php-'));let indexed;try{
 for(const [path,source]of Object.entries(project.files)){mkdirSync(dirname(join(dir,path)),{recursive:true});writeFileSync(join(dir,path),source);if(path.endsWith('.php'))add(path,source,project.name);}
 indexed=await Project.init(dir,{silent:true});const result=await indexed.indexAll();if(!result.success)throw Error('Index failed');
 project.routes=indexed.queries.getNodesByKind('route').map(node=>({node:strip(node),targets:indexed.queries.getOutgoingEdges(node.id).map(edge=>({kind:edge.kind,node:strip(indexed.getNode(edge.target)),metadata:edge.metadata}))}));
 const context=new ReferenceResolver(dir,indexed.queries);context.initialize();project.nodes=indexed.queries.getAllNodes().map(strip);project.resolutions=[];
 const origin=project.nodes.find(node=>node.kind==='file'&&node.language==='php');
 for(const name of ['PageController@list','Common\\Uploads@inline','Portal\\Uploads@inline','Missing\\Uploads@inline','Uploads@destroy','Modules\\Offline\\Http\\Controllers\\Settings@show','User::find','User::all','Legacy::all','Cache::get','route','Common\\Companies','Modules\\Offline\\Http\\Controllers\\Settings'])for(const resource of [false,true]){
 const reference={fromNodeId:resource?'route:RESOURCE:test':origin.id,referenceName:name,referenceKind:resource?'imports':'calls',line:1,column:0,filePath:origin.filePath,language:'php'};const resolved=resolver.resolve(reference,context.context);project.resolutions.push({reference,expected:resolved?{id:resolved.targetNodeId,confidence:resolved.confidence}:null});}
 console.log(project.name,project.routes.length);
}finally{indexed?.close();rmSync(dir,{recursive:true,force:true});}}
const destination=dirname(fileURLToPath(import.meta.url));writeFileSync(join(destination,'cases.json'),JSON.stringify({revision,cases,projects},null,2)+'\n');writeFileSync(join(destination,'LICENSE'),readFileSync(join(root,'LICENSE')));console.log(cases.length,'extractions',projects.length,'projects');
