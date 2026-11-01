// Original Angular project inputs and independently indexed canonical records.
import {execFileSync} from 'node:child_process';import{createRequire}from'node:module';import{readFileSync,writeFileSync,mkdirSync,mkdtempSync,rmSync}from'node:fs';import{join,dirname,resolve}from'node:path';import{tmpdir}from'node:os';import{runInNewContext}from'node:vm';
const root=resolve(process.argv[2]),revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';if(execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('Wrong reference revision');const require=createRequire(join(root,'package.json')),ts=require('typescript');
const reader=require('./dist/resolution/import-resolver.js'),originalResolve=reader.resolveImportPath;let answers=[];reader.resolveImportPath=(specifier,from,language,context)=>{const result=originalResolve(specifier,from,language,context);answers.push({specifier,from,language,result});return result;};
const api=require('./dist/resolution/frameworks/angular-router.js'),template=require('./dist/resolution/angular-template-synthesizer.js'),{ReferenceResolver}=require('./dist/resolution/index.js');
const {extractFromSource}=require('./dist/extraction/tree-sitter.js');
const {initGrammars,loadAllGrammars}=require('./dist/extraction/grammars.js');await initGrammars();await loadAllGrammars();
const Project=Object.values(require('./dist/index.js')).find(value=>typeof value==='function'&&typeof value.init==='function'&&typeof value.open==='function');
const source=readFileSync(join(root,'__tests__/angular-router.test.ts'),'utf8'),tree=ts.createSourceFile('angular-router.test.ts',source,ts.ScriptTarget.Latest,true);const projects=[];
const transpile=source=>ts.transpileModule(source,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;
function componentAt(node){for(let scope=node.parent;scope;scope=scope.parent){for(const statement of [...(scope.statements??[])].reverse()){if(statement.pos>node.pos)continue;if(ts.isVariableStatement(statement))for(const declaration of statement.declarationList.declarations)if(declaration.name.getText(tree)==='component')return declaration.getText(tree);}}throw Error('component helper missing');}
function visit(node){
 if(ts.isVariableDeclaration(node)&&node.name.getText(tree)==='files'&&node.initializer&&ts.isObjectLiteralExpression(node.initializer)){
  let script=`const ${componentAt(node)};const files=${node.initializer.getText(tree)};\n`;const statements=node.parent.parent.parent.statements??[];for(const statement of statements){if(statement.pos<=node.pos)continue;if(ts.isForOfStatement(statement))break;if(ts.isExpressionStatement(statement)&&statement.getText(tree).startsWith('files['))script+=statement.getText(tree)+'\n';}
  const files=runInNewContext(transpile(script+'files;'),{}, {timeout:1000});projects.push({id:'original-'+projects.length,files});
 }
 if(ts.isCallExpression(node)&&node.expression.getText(tree)==='project'&&ts.isObjectLiteralExpression(node.arguments[0])){const files=runInNewContext(transpile(`const ${componentAt(node)};(${node.arguments[0].getText(tree)});`),{}, {timeout:1000});projects.push({id:'original-'+projects.length,files});}
 ts.forEachChild(node,visit);
}visit(tree);
const componentSource=(name,selector,template,body='')=>`import {Component} from '@angular/core';
@Component({selector:${JSON.stringify(selector)},template:\`${template}\`})
export class ${name} {${body}}
`;
const dependency=JSON.stringify({dependencies:{'@angular/core':'*','@angular/router':'*'}});
projects.push({id:'router-only',files:{'package.json':JSON.stringify({dependencies:{'@angular/router':'*'}}),'src/routes.ts':"import {Routes} from '@angular/router'; import {Page} from './page'; export const routes:Routes=[{path:'a',component:Page}];",'src/page.ts':componentSource('Page','app-page','<button (click)="save()">Save</button>','save(){}')}});
{
 const files={'package.json':dependency};for(const app of ['one','two']){files[`apps/${app}/src/routes.ts`]=`import {Routes} from '@angular/router'; import {Page} from './page'; export const routes:Routes=[{path:'${app}',component:Page}];`;files[`apps/${app}/src/page.ts`]=componentSource('Page','app-page',`<child-card></child-card><a routerLink="/${app}">Here</a><a routerLink="/unknown">Unknown</a>`);files[`apps/${app}/src/child.ts`]=componentSource('Child','child-card','<b>Child</b>');}files['apps/one/src/duplicate.ts']=componentSource('Duplicate','child-card','<b>Duplicate</b>');projects.push({id:'selector-app-ambiguity',files});
}
{
 const files={'package.json':dependency};let template='';for(let n=0;n<42;n++){files[`src/child${n}.ts`]=componentSource(`Child${n}`,`child-${n}`,'<b />');template+=`<child-${n}></child-${n}>`;}files['src/page.ts']=componentSource('Page','app-page',template);files['src/routes.ts']="import {Routes} from '@angular/router'; import {Page} from './page'; export const routes:Routes=[{path:'home',component:Page}];";projects.push({id:'template-child-cap',files});
}
{
 const files={'package.json':dependency};let template='';for(let n=0;n<26;n++)template+=`<a routerLink="/screen${n}">Link</a>`;files['src/page.ts']=componentSource('Page','app-page',template);files['src/routes.ts']="import {Routes} from '@angular/router'; import {Page} from './page'; export const routes:Routes=["+Array.from({length:26},(_,n)=>`{path:'screen${n}',component:Page}`).join(',')+"];";projects.push({id:'template-link-cap',files});
}

const clean=node=>{const value={...node};delete value.updatedAt;return value};
for(const project of projects){const dir=mkdtempSync(join(tmpdir(),'prometeu-angular-'));let indexed;try{
 for(const[path,source]of Object.entries(project.files)){mkdirSync(dirname(join(dir,path)),{recursive:true});writeFileSync(join(dir,path),source);}
 answers=[];indexed=await Project.init(dir,{silent:true});await indexed.indexAll();const queries=indexed.queries;const resolver=new ReferenceResolver(dir,queries);resolver.initialize();const context=resolver.context;
 const nodes=queries.getAllNodes();project.nodes=nodes.map(clean);project.edges=nodes.flatMap(node=>queries.getOutgoingEdges(node.id));
 project.rawRoutes=[];project.references=[];
 for(const[path,source]of Object.entries(project.files)){if(!/\.[cm]?ts$/.test(path))continue;const framework=api.angularRouterResolver.extract(path,source);project.rawRoutes.push(...framework.nodes.map(clean));const extracted=extractFromSource(path,source,'typescript');project.references.push(...extracted.unresolvedReferences.map(ref=>({...ref,filePath:path,language:'typescript'})),...framework.references);}
 project.resolutions=project.references.map(reference=>{const result=api.angularRouterResolver.resolve(reference,context);return{reference,expected:result?{target:result.targetNodeId,kind:result.edgeKind??null,metadata:result.metadata??null,alsoTargets:result.alsoTargets??[],confidence:result.confidence}:null}});
 project.constants=[];for(const[file,source]of Object.entries(project.files)){if(!/\.[cm]?ts$/.test(file))continue;const expressions=new Set([...source.matchAll(/[A-Za-z_$][\w$]*(?:\s*\.\s*[A-Za-z_$][\w$]*)+/g)].map(match=>match[0].replace(/\s+/g,'')));for(const expression of expressions)project.constants.push({file,expression,expected:api.constantText(expression,file,context)});}
 const table=api.angularRouteTable(context);project.table=Object.fromEntries([...table.byRoot].map(([root,table])=>[root,Object.fromEntries([...table.exact].map(([path,node])=>[path,node.id]))]));
 project.templateEdges=await template.angularTemplateEdges(context,async()=>{});project.moduleAnswers=[...new Map(answers.map(answer=>[JSON.stringify([answer.specifier,answer.from,answer.language]),answer])).values()];console.log(project.id,nodes.length,project.rawRoutes.length,project.templateEdges.length);
}finally{indexed?.close();rmSync(dir,{recursive:true,force:true});}}
writeFileSync(new URL('./projects.json',import.meta.url),JSON.stringify({revision,projects},null,2)+'\n');
