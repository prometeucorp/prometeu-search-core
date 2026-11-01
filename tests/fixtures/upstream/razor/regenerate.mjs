// Development-only independent TypeScript/WASM oracle.
import {execFileSync} from 'node:child_process';
import {readFileSync,writeFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import {dirname,join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
const revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
const upstream=resolve(process.argv[2]??'');
if(!process.argv[2]||execFileSync('git',['-C',upstream,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw new Error('Pass the prepared pinned checkout.');
const require=createRequire(join(upstream,'package.json'));
const ts=require('typescript');
const {initGrammars,loadGrammarsForLanguages}=require('./dist/extraction/grammars.js');
const {RazorExtractor}=require('./dist/extraction/razor-extractor.js');
const {blankRazorInScript,blankRazorComments,isJavaScriptBlock}=require('./dist/extraction/razor-script.js');
await initGrammars();await loadGrammarsForLanguages(['csharp','javascript','typescript']);
const cases=[],scripts=[],seen=new Set(),scriptSeen=new Set();
const product=JSON.parse(readFileSync(join(upstream,'package.json'),'utf8')).name.split('/').at(-1);
function capture(source,origin){
 source=source.replace(new RegExp(product,'gi'),'prometeu');
 if(seen.has(source))return;seen.add(source);
 for(const ext of ['razor','cshtml']){
  const filePath=`Views/Probe.${ext}`;
  const expected=new RazorExtractor(filePath,source).extract();
  for(const node of expected.nodes)delete node.updatedAt;delete expected.durationMs;
  cases.push({id:`${ext}-${cases.length+1}`,origin,filePath,source,expected});
 }
}
function value(node){
 if(ts.isStringLiteral(node)||ts.isNoSubstitutionTemplateLiteral(node))return node.text;
 if(ts.isCallExpression(node)&&ts.isPropertyAccessExpression(node.expression)&&node.expression.name.text==='join'&&ts.isArrayLiteralExpression(node.expression.expression)){
  const parts=node.expression.expression.elements.map(value);const separator=value(node.arguments[0]);
  if(parts.every(part=>part!==undefined)&&separator!==undefined)return parts.join(separator);
 }
}
for(const name of ['razor-script','extraction','csharp-declared-receivers']){
 const origin=`__tests__/${name}.test.ts`,source=readFileSync(join(upstream,origin),'utf8');
 const tree=ts.createSourceFile(origin,source,ts.ScriptTarget.Latest,true);
 function visit(node){
  const text=value(node);
  const inRange=name!=='extraction'||(tree.getLineAndCharacterOfPosition(node.pos).line>=7300&&tree.getLineAndCharacterOfPosition(node.pos).line<7525);
  if(inRange&&text!==undefined){
   if(name==='razor-script'&&text.includes('@')&&!scriptSeen.has(text)){
    scriptSeen.add(text);scripts.push({source:text,prepared:blankRazorInScript(text),comments:blankRazorComments(text)});
   }
   if(/<script(?:\s|>)|@(?:model|inherits|inject|typeof|code|functions)\b|@\{/.test(text))capture(text,origin);
  }
  ts.forEachChild(node,visit);
 }
 visit(tree);
}
capture('@model App.Models.ViewModel<Item>\n@inject IService svc\n@inherits Base<One, Two>\n<p>é😀 @typeof(App.MainLayout)</p>\n<Router/><Grid TItem="App.CatalogItem"/><InputText/>\n@code { void Load() { var svc = new UserService(); svc.Load(); } }\n<script>var id = @Model.Id; toast(id);</script>\n','directives, framework components, C# and script');
for(const source of ['const x = "😀"; use(@Model.Id);\r\n','@if (ok) { <text>call(@value); @if (other) { <text>nested();</text> }</text> }']){
 scripts.push({source,prepared:blankRazorInScript(source),comments:blankRazorComments(source)});
 capture(`<script>${source}</script>`,'Unicode and nested controls');
}
const attributes=['',' type=""',' type="module"',' TYPE="application/javascript"',' type="text/x-babel"',' type="application/json"',' type="text/html"',' type="application/typescript"',' type="unknown"'];
writeFileSync(join(dirname(fileURLToPath(import.meta.url)),'cases.json'),JSON.stringify({revision,cases,scripts,attributes:attributes.map(source=>({source,javascript:isJavaScriptBlock(source)}))},null,2)+'\n');
console.log(`Captured ${cases.length} complete Razor records, ${scripts.length} scanner cases.`);
