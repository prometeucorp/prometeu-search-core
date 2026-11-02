// Development-only oracle. Execute the pinned TypeScript/WASM reference independently.
import {readFileSync,readdirSync,writeFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import {join,dirname,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {execFileSync} from 'node:child_process';
const upstream=resolve(process.argv[2]);
const revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['-C',upstream,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('Wrong reference revision');
const require=createRequire(join(upstream,'package.json')), ts=require('typescript');
const {TreeSitterExtractor}=require('./dist/extraction/tree-sitter.js');
const {DfmExtractor}=require('./dist/extraction/dfm-extractor.js');
const {preParsePascal}=require('./dist/extraction/languages/pascal-preparse.js');
const {initGrammars,loadGrammarsForLanguages,getParser,detectLanguage}=require('./dist/extraction/grammars.js');
await initGrammars();await loadGrammarsForLanguages(['pascal']);
const cases=[],seen=new Set();
function add(source,origin,path){
 if(typeof source!=='string'||source.length<15||seen.has(source))return;
 if(!path&&!/\b(?:unit|program|library|procedure|function|type|const)\b/i.test(source))return;
 seen.add(source);path??=`case${cases.length}.pas`;
 const expected=/\.(?:dfm|fmx)$/i.test(path)?new DfmExtractor(path,source).extract():new TreeSitterExtractor(path,source,'pascal').extract();
 for(const n of expected.nodes)delete n.updatedAt;delete expected.durationMs;
 cases.push({id:`pascal-${cases.length}`,path,source,prepared:preParsePascal(source),origin,expected,detectedIncludeLanguage:detectLanguage("sample.inc",source)});
}
for(const filename of readdirSync(join(upstream,'__tests__')).filter(n=>/^pascal-.*\.test\.ts$/.test(n))){
 const text=readFileSync(join(upstream,'__tests__',filename),'utf8'),file=ts.createSourceFile(filename,text,ts.ScriptTarget.Latest,true);
 function lookup(name,node){let cursor=node.parent;while(cursor){const statements=cursor.statements;if(statements){for(const s of statements){if(ts.isFunctionDeclaration(s)&&s.name?.text===name)return s;if(s.pos>node.pos)continue;if(ts.isVariableStatement(s))for(const d of s.declarationList.declarations)if(d.name.getText(file)===name)return d.initializer;}}cursor=cursor.parent;}}
 function literal(n,env=new Map(),depth=0){if(!n||depth>15)return undefined;
  if(ts.isStringLiteral(n)||ts.isNoSubstitutionTemplateLiteral(n))return n.text;
  if(ts.isIdentifier(n)){if(env.has(n.text))return env.get(n.text);const bound=lookup(n.text,n);return bound&&literal(bound,env,depth+1);}
  if(ts.isParenthesizedExpression(n))return literal(n.expression,env,depth+1);
  if(ts.isTemplateExpression(n)){let value=n.head.text;for(const span of n.templateSpans){const part=literal(span.expression,env,depth+1);if(typeof part!=='string')return undefined;value+=part+span.literal.text;}return value;}
  if(ts.isBinaryExpression(n)&&n.operatorToken.kind===ts.SyntaxKind.PlusToken){const a=literal(n.left,env,depth+1),b=literal(n.right,env,depth+1);if(typeof a==='string'&&typeof b==='string')return a+b;}
  if(ts.isArrayLiteralExpression(n)){const a=n.elements.map(e=>literal(e,env,depth+1));if(a.every(v=>v!==undefined))return a;}
  if(ts.isCallExpression(n)){
   if(ts.isPropertyAccessExpression(n.expression)&&n.expression.name.text==='join'){const a=literal(n.expression.expression,env,depth+1),s=literal(n.arguments[0],env,depth+1);if(Array.isArray(a)&&typeof s==='string')return a.join(s);}
   if(ts.isIdentifier(n.expression)){const fn=lookup(n.expression.text,n);if(fn&&ts.isFunctionDeclaration(fn)){const scope=new Map(env);for(let i=0;i<fn.parameters.length;i++)scope.set(fn.parameters[i].name.getText(file),literal(n.arguments[i],env,depth+1));const ret=fn.body?.statements.find(ts.isReturnStatement);return literal(ret?.expression,scope,depth+1);}}
  }
 }
 function visit(n){if(ts.isVariableDeclaration(n)||ts.isPropertyAssignment(n)){const path=ts.isPropertyAssignment(n)?literal(n.name):undefined;add(literal(n.initializer),filename,typeof path==='string'&&/\.(?:pas|dfm|fmx|dpr|inc)$/.test(path)?path:undefined);}ts.forEachChild(n,visit);}visit(file);
}
add('object Main: TForm\n  object Go: TButton\n    OnClick = GoClick\n    Lines = (\n      object Fake: TButton\n    )\n  end\n  OnShow = FormShow\nend\n','supplemental','Main.dfm');
add('inherited Root: TWindow\r\n  inline Widget: TControl\r\n    OnClick = Click\r\n  end\r\nend\r\n','supplemental','Root.fmx');
add("unit Unicode; interface procedure Run(const [Attr('😀')] X: Integer); implementation procedure Run(const [Attr('😀')] X: Integer); begin Save(X); end; end.",'supplemental');
const destination=dirname(fileURLToPath(import.meta.url));writeFileSync(join(destination,'cases.json'),JSON.stringify({revision,grammar:{nodeKinds:Array.from({length:getParser("pascal").language.nodeTypeCount},(_,i)=>getParser("pascal").language.nodeTypeForId(i)??""),fieldNames:Array.from({length:getParser("pascal").language.fieldCount},(_,i)=>getParser("pascal").language.fieldNameForId(i+1)??"")},cases},null,2)+'\n');writeFileSync(join(destination,'LICENSE'),readFileSync(join(upstream,'LICENSE')));console.log(cases.length);
