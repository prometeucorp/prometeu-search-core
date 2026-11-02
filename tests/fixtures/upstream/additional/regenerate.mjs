// Development-only reference capture; no JavaScript runtime is used by Rust.
import {execFileSync} from 'node:child_process';
import {readFileSync, writeFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import {dirname, join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import vm from 'node:vm';

const revision = '6aa358ccbae30fbf27318f79ee183212f726f9ba';
const upstream = resolve(process.argv[2] ?? '');
if (!process.argv[2]) throw new Error('Pass the prepared upstream checkout.');
if (execFileSync('git', ['-C', upstream, 'rev-parse', 'HEAD'], {encoding:'utf8'}).trim() !== revision) throw new Error('Incorrect upstream revision.');
const require = createRequire(join(upstream,'package.json'));
const ts = require('typescript');
const grammars = require('./dist/extraction/grammars.js');
const {TreeSitterExtractor} = require('./dist/extraction/tree-sitter.js');
await grammars.initGrammars();
await grammars.loadGrammarsForLanguages(['nix','erlang','terraform','cobol']);
const product = JSON.parse(readFileSync(join(upstream,'package.json'),'utf8')).name.split('/').at(-1);
const cases = [], seen = new Map();
let currentLanguage='', currentTest='';
function capture(filePath, source) {
  source = source.replace(new RegExp(product,'gi'),'prometeu');
  const key = currentLanguage + '\0' + filePath + '\0' + source;
  if (seen.has(key)) return seen.get(key);
  const id = `${currentLanguage}-${cases.length+1}`;
  // Reference memoization keys files by path. Isolate independent fixture files.
  const path = `oracle/${id}/${filePath}`;
  const expected = new TreeSitterExtractor(path,source,currentLanguage).extract();
  for (const node of expected.nodes) delete node.updatedAt;
  delete expected.durationMs;
  seen.set(key,expected);
  cases.push({id,language:currentLanguage,filePath:path,source,origin:currentTest,expected});
  return expected;
}
const expect = new Proxy(function(){return expect;}, {get(){return expect;},apply(){return expect;}});
const origin = '__tests__/extraction.test.ts';
const source = readFileSync(join(upstream,origin),'utf8');
const parsed = ts.createSourceFile(origin,source,ts.ScriptTarget.Latest,true);
for (const statement of parsed.statements) {
  if (!ts.isExpressionStatement(statement) || !ts.isCallExpression(statement.expression)) continue;
  const call = statement.expression;
  if (call.expression.getText(parsed)!=='describe' || !ts.isStringLiteral(call.arguments[0])) continue;
  const label=call.arguments[0].text;
  const language = new Map([['Nix Extraction','nix'],['COBOL Extraction','cobol'],['Erlang Extraction','erlang'],['Terraform Extraction','terraform']]).get(label);
  if (!language) continue;
  currentLanguage=language;
  const script=ts.transpileModule(statement.getText(parsed),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;
  const runTest=(name,body)=>{currentTest=`${origin}: ${name}`;body();};
  vm.runInNewContext(script,{...grammars,extractFromSource:capture,expect,describe:(_name,body)=>body(),it:runTest,test:runTest,console}, {timeout:60000});
}
const destination=dirname(fileURLToPath(import.meta.url));
for (const [language,filePath,source] of [
  ['nix','unicode.nix','{ label = "é😀"; run = arg: helper arg; imports = [ ./child.nix ]; }\n'],
  ['erlang','unicode.erl','-module(unicode). msg() -> <<"é😀">>. next(X) -> msg(), X.\n'],
  ['terraform','unicode.tf','variable "é😀" { default = "ok" }\noutput "answer" { value = var.input }\n'],
  ['cobol','unicode.cbl','IDENTIFICATION DIVISION.\nPROGRAM-ID. UNICODE-PROG.\nPROCEDURE DIVISION.\nMAIN-PARA.\n    DISPLAY "é😀"\n    CALL "OTHER-PROG".\n'],
  ['cobol','short.cob','    .'],
]) {
  currentLanguage=language; currentTest='focused Unicode/source-boundary regression';
  capture(filePath,source);
  if (source.includes('\n')) capture(filePath,source.replace(/\n/g,'\r\n'));
}
const identities=Object.fromEntries(['nix','erlang','terraform','cobol'].map(name=>{
  const language=grammars.getParser(name).language;
  return [name,{abiVersion:language.abiVersion,nodeKinds:Array.from({length:language.nodeTypeCount},(_,index)=>language.nodeTypeForId(index)),fieldNames:Array.from({length:language.fieldCount},(_,index)=>language.fieldNameForId(index+1))}];
}));
writeFileSync(join(destination,'cases.json'),JSON.stringify({revision,grammars:identities,cases},null,2)+'\n');
console.log(`Captured ${cases.length} independent cases: ${JSON.stringify(Object.groupBy(cases,c=>c.language),(_key,value)=>Array.isArray(value)?value.length:value)}`);
