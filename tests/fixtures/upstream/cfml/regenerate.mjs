// Development-only TypeScript/WASM oracle, independent of Rust output.
import {execFileSync} from 'node:child_process';
import {readFileSync,writeFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import {dirname,join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
const revision='6aa358ccbae30fbf27318f79ee183212f726f9ba',upstream=resolve(process.argv[2]??'');
if(!process.argv[2]||execFileSync('git',['-C',upstream,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw new Error('Pass pinned prepared checkout.');
const require=createRequire(join(upstream,'package.json')),ts=require('typescript');
const {initGrammars,loadGrammarsForLanguages,getParser}=require('./dist/extraction/grammars.js');
const {CfmlExtractor}=require('./dist/extraction/cfml-extractor.js');
const {TreeSitterExtractor}=require('./dist/extraction/tree-sitter.js');
await initGrammars();await loadGrammarsForLanguages(['cfml']);
const cases=[],seen=new Set();const product=JSON.parse(readFileSync(join(upstream,'package.json'),'utf8')).name.split('/').at(-1);
function capture(source,origin,languages=['cfml','cfscript']){
 source=source.replace(new RegExp(product,'gi'),'prometeu');
 for(const language of languages){
  if(seen.has(language+source))continue;seen.add(language+source);
  const filePath=`src/Probe.${language==='cfscript'?'cfs':'cfc'}`;
  const expected=language==='cfquery'?new TreeSitterExtractor(filePath,source,language).extract():new CfmlExtractor(filePath,source,language).extract();
  for(const node of expected.nodes)delete node.updatedAt;delete expected.durationMs;
  cases.push({id:`${language}-${cases.length+1}`,language,filePath,origin,source,expected});
 }
}
function value(node){
 if(ts.isStringLiteral(node)||ts.isNoSubstitutionTemplateLiteral(node))return node.text;
 if(ts.isCallExpression(node)&&ts.isPropertyAccessExpression(node.expression)&&node.expression.name.text==='join'&&ts.isArrayLiteralExpression(node.expression.expression)){
  const parts=node.expression.expression.elements.map(value),separator=value(node.arguments[0]);if(parts.every(part=>part!==undefined)&&separator!==undefined)return parts.join(separator);
 }
}
for(const name of ['cfml-call-scope','cfml-create-object','cfml-embedded-parse-warning','cfml-include','cfml-inheritance-resolution','cfml-receiver-inference','cfml-tag-calls']){
 const origin=`__tests__/${name}.test.ts`,source=readFileSync(join(upstream,origin),'utf8'),tree=ts.createSourceFile(origin,source,ts.ScriptTarget.Latest,true);
 function visit(node){const source=value(node);if(source!==undefined&&source.length>20&&/<cf|\b(?:component|interface)\s*[{\s]|\bfunction\b|createObject\s*\(|\binclude\s*["']|cfinclude\s*\(/i.test(source))capture(source,origin);ts.forEachChild(node,visit);}visit(tree);
}
for(const source of [
 '/** Service. */\ncomponent extends="Base" implements="IFoo,IBar" {\n property name="user" type="model.User";\n property string title;\n public string function save(required model.User user, string note="x") { var local = make(); if (ok()) { local.run(); } return user.save(); }\n private function hidden() { variables.svc.work(); this.svc.done(); new a.b.Worker().run(); createObject("component", "a.b.Worker").run(); cfinclude(template="x.cfm"); }\n var count = build();\n}\nvar top = foo();\nvar work = function(x) { baz(); };\nvar arrow = (x)=>qux();\n',
 '\uFEFF /* 😀 */ component { public function first() { call(); } public function first() { other(); } }\r\n',
 '<cfcomponent extends="Base" implements="IFoo,IBar">\n<cffunction name="save" access="package" returntype="void"><cfset run()><cfscript>var value = load(); function inner(){nested();}</cfscript></cffunction>\n<cfscript>function configure() { setup(); }</cfscript>\n</cfcomponent>\n',
 '<cfinterface name="Service" extends="IBase, IOther"><cffunction name="save"/></cfinterface>\n',
 '<p>é😀 #load()#</p> <cfset x=make()> <cfinclude template="view.cfm">\r\n<cfquery>select * from people where id=#getCurrentUser().getId()#</cfquery>',
 '<cfscript>function same(){first();}</cfscript><cfscript>function another(){second();}</cfscript>',
])capture(source,'Additional complete language shapes');
for(const source of ['select * from users where id=#currentUser().id()#','select #new app.Model().value()# from foo','select #createObject("component","app.Model").read()# from foo','select broken ??? #next()#','select 1'])capture(source,'Embedded query dialect',['cfquery']);
const grammars={};for(const language of ['cfml','cfscript','cfquery']){const grammar=getParser(language).language;grammars[language]={abiVersion:grammar.abiVersion,nodeKindCount:grammar.nodeTypeCount,fieldCount:grammar.fieldCount,stateCount:grammar.stateCount,nodeKinds:Array.from({length:grammar.nodeTypeCount},(_,i)=>grammar.nodeTypeForId(i)),fieldNames:Array.from({length:grammar.fieldCount},(_,i)=>grammar.fieldNameForId(i+1))};}
writeFileSync(join(dirname(fileURLToPath(import.meta.url)),'cases.json'),JSON.stringify({revision,grammarRevision:'776f83d82440aeb05d163bc454e88890a8a99870',grammars,cases},null,2)+'\n');console.log(`Captured ${cases.length} CFML family cases.`);
