// Development-only oracle: original test inputs, independently executed upstream.
import {readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import {join, dirname, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {execFileSync} from 'node:child_process';
const upstream = resolve(process.argv[2]);
const revision = '6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['-C',upstream,'rev-parse','HEAD'],{encoding:'utf8'}).trim() !== revision) throw Error('Wrong reference revision');
const require = createRequire(join(upstream,'package.json'));
const ts = require('typescript');
const {extractFromSource} = require('./dist/extraction/tree-sitter.js');
const {initGrammars,loadGrammarsForLanguages,getParser} = require('./dist/extraction/grammars.js');
await initGrammars(); await loadGrammarsForLanguages(['arkts','objc','solidity','vbnet']);
const cases = [], seen = new Set();
const languageOf = path => ({ets:'arkts',m:'objc',mm:'objc',h:'objc',sol:'solidity',vb:'vbnet'})[path.split('.').at(-1)];
function literal(node) {
  if(!node)return undefined;
  if(ts.isStringLiteral(node)||ts.isNoSubstitutionTemplateLiteral(node)) return node.text;
  if(ts.isParenthesizedExpression(node))return literal(node.expression);
  if(ts.isBinaryExpression(node)&&node.operatorToken.kind===ts.SyntaxKind.PlusToken) {
    const a=literal(node.left), b=literal(node.right); if(a!==undefined&&b!==undefined)return a+b;
  }
  if(ts.isArrayLiteralExpression(node)){const a=node.elements.map(literal); if(a.every(x=>x!==undefined))return a;}
  if(ts.isCallExpression(node)&&ts.isPropertyAccessExpression(node.expression)&&node.expression.name.text==='join'){
    const values=literal(node.expression.expression), separator=literal(node.arguments[0]);
    if(Array.isArray(values)&&typeof separator==='string')return values.join(separator);
  }
}
function add(path,source,origin) {
  const language=languageOf(path); if(!language||typeof source!=='string'||source.length<12)return;
  const key=language+'\0'+source; if(seen.has(key))return;seen.add(key);
  const expected=extractFromSource(path,source,language);
  for(const node of expected.nodes)delete node.updatedAt;delete expected.durationMs;
  const input=language==='objc'?require('./dist/extraction/languages/objc.js').objcExtractor.preParse(source):source;
  const parser=getParser(language);const tree=parser.parse(language==='vbnet'&&!input.endsWith('\n')?input+'\n':input);
  const clean=!tree.rootNode.hasError;tree.delete();
  cases.push({id:language+'-'+cases.filter(c=>c.language===language).length,path,language,source,input,origin,clean,expected});
}
for(const filename of readdirSync(join(upstream,'__tests__')).filter(n=>n==='extraction.test.ts'||/^(arkts|objc|solidity|vbnet)-.*\.test\.ts$/.test(n))){
 const source=readFileSync(join(upstream,'__tests__',filename),'utf8');
 const file=ts.createSourceFile(filename,source,ts.ScriptTarget.Latest,true);
 const strings=node=>{const out=[];const visit=n=>{if(ts.isStringLiteral(n))out.push(n.text);ts.forEachChild(n,visit)};visit(node);return out;};
 const visit=node=>{
  if(ts.isPropertyAssignment(node)){const path=literal(node.name);if(typeof path==='string')add(path,literal(node.initializer),filename);}
  if(ts.isCallExpression(node)&&node.expression.getText(file).endsWith('writeFileSync')){
   const path=strings(node.arguments[0]).findLast(s=>languageOf(s));if(path)add(path,literal(node.arguments[1]),filename);
  }
  if(ts.isCallExpression(node)&&node.expression.getText(file)==='extractFromSource'){
   const path=literal(node.arguments[0]);let value=literal(node.arguments[1]);
   if(value===undefined&&node.arguments[1]&&ts.isIdentifier(node.arguments[1])){
    const name=node.arguments[1].text;let cursor=node.parent;
    while(cursor&&value===undefined){if(ts.isBlock(cursor)){for(const statement of cursor.statements){if(statement.pos>node.pos)break;if(ts.isVariableStatement(statement))for(const d of statement.declarationList.declarations)if(d.name.getText(file)===name)value=literal(d.initializer);}}cursor=cursor.parent;}
   }
   if(typeof path==='string')add(path,value,filename);
  }
  ts.forEachChild(node,visit);
 };visit(file);
}
add('Extra.sol',`pragma solidity ^0.8.20;
type Price is uint128;
/// A global function.
function twice(uint value) pure returns (uint) { return value * 2; }
contract Other { constructor(uint value) {} }
contract Store {
    event Changed(uint value);
    error Failed(uint value);
    /// Stored price.
    Price private price;
    constructor() { Other other = new Other(2); }
    fallback() external payable { emit Changed(twice(2)); }
    receive() external payable { revert Failed(1); }
}
`,'focused-syntax');
add('Selectors.m',`@protocol Delegate
- (void)handle:(id)value;
@end
@interface Widget : Base <Delegate>
@property (nonatomic) Widget *child;
@end
@implementation Widget
/// Factory result.
+ (nullable Widget *)make { return nil; }
- (void)handle:(id)value { registerHandler(@selector(handle:)); }
- (void)refresh { registerHandler(@selector(refresh)); }
- (void)run { [[Widget make] handle:nil]; }
@end
Widget *factory(void) { return [Widget make]; }
`,'focused-syntax');
add('Callbacks.m',`struct Callbacks { void (*run)(void); };
struct Callbacks *makeCallbacks(void);
void invoke(void) { makeCallbacks()->run(); }
`,'focused-syntax');
const directory=dirname(fileURLToPath(import.meta.url));
const grammars = Object.fromEntries(['arkts','objc','solidity','vbnet'].map(name=>{
 const grammar=getParser(name).language;
 return [name,{abiVersion:grammar.abiVersion,nodeKinds:Array.from({length:grammar.nodeTypeCount},(_,i)=>grammar.nodeTypeForId(i)||''),fieldNames:Array.from({length:grammar.fieldCount},(_,i)=>grammar.fieldNameForId(i+1)||'')}];
}));
writeFileSync(join(directory,'cases.json'),JSON.stringify({revision,grammars,cases},null,2)+'\n');
writeFileSync(join(directory,'LICENSE'),readFileSync(join(upstream,'LICENSE')));
console.log(JSON.stringify(Object.fromEntries(['arkts','objc','solidity','vbnet'].map(language=>[language,{cases:cases.filter(c=>c.language===language).length,clean:cases.filter(c=>c.language===language&&c.clean).length}]))));
