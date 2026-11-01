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
const {playResolver:resolver}=require('./dist/resolution/frameworks/play.js');
const Project=Object.values(require('./dist/index.js')).find(value=>typeof value==='function'&&typeof value.init==='function'&&typeof value.open==='function');
const {ReferenceResolver}=require('./dist/resolution/index.js');
const strip=node=>{const copy={...node};delete copy.updatedAt;return copy;};
const cases=[],seen=new Set(),projects=[];
function add(path,source,origin){const key=JSON.stringify([path,source]);if(seen.has(key))return;seen.add(key);const extracted=resolver.extract(path,source);cases.push({path,source,origin,expected:{nodes:extracted.nodes.map(strip),references:extracted.references}});}
function literal(node,tree,depth=0){if(!node||depth>20)return undefined;if(ts.isStringLiteral(node)||ts.isNoSubstitutionTemplateLiteral(node))return node.text;if(ts.isNumericLiteral(node))return Number(node.text);if(node.kind===ts.SyntaxKind.TrueKeyword)return true;if(node.kind===ts.SyntaxKind.FalseKeyword)return false;if(ts.isArrayLiteralExpression(node)){const values=node.elements.map(value=>literal(value,tree,depth+1));return values.every(value=>value!==undefined)?values:undefined;}if(ts.isObjectLiteralExpression(node)){const object={};for(const property of node.properties){if(ts.isSpreadAssignment(property)){const spread=literal(property.expression,tree,depth+1);if(!spread)return undefined;Object.assign(object,spread);continue;}if(ts.isShorthandPropertyAssignment(property)){const value=literal(property.name,tree,depth+1);if(value===undefined)return undefined;object[property.name.text]=value;continue;}if(!ts.isPropertyAssignment(property))return undefined;const key=ts.isIdentifier(property.name)?property.name.text:literal(ts.isComputedPropertyName(property.name)?property.name.expression:property.name,tree,depth+1);const value=literal(property.initializer,tree,depth+1);if(key===undefined||value===undefined)return undefined;object[key]=value;}return object;}if(ts.isBinaryExpression(node)&&node.operatorToken.kind===ts.SyntaxKind.PlusToken){const left=literal(node.left,tree,depth+1),right=literal(node.right,tree,depth+1);if(typeof left==='string'&&typeof right==='string')return left+right;}if(ts.isIdentifier(node)){for(let block=node.parent;block;block=block.parent){for(const statement of block.statements??[]){if(statement.pos>node.pos)continue;if(ts.isVariableStatement(statement))for(const declaration of statement.declarationList.declarations)if(declaration.name.getText(tree)===node.text)return literal(declaration.initializer,tree,depth+1);}}}if(ts.isCallExpression(node)&&node.expression.getText(tree)==='JSON.stringify'){const value=literal(node.arguments[0],tree,depth+1);if(value!==undefined)return JSON.stringify(value);}}const matcherModule={exports:{}};new Function('require','module','exports',readFileSync(join(root,'dist/resolution/name-matcher.js'),'utf8')+'\nmodule.exports.helperOracle={retargetSelfOverload,javaArgumentType,javaArgumentFit,javaParameters,javaBareType,splitCppTopLevel,cppParenListIn,javaTypeRef,isJavaOutsideImport};')(createRequire(join(root,'dist/resolution/name-matcher.js')),matcherModule,matcherModule.exports);const helpers=matcherModule.exports.helperOracle;
const {extractFromSource}=require('./dist/extraction/tree-sitter.js');
const {initGrammars,loadGrammarsForLanguages}=require('./dist/extraction/grammars.js');await initGrammars();await loadGrammarsForLanguages(['java','scala']);
for(const file of ['play-nested-projects.test.ts','java-overload-arguments.test.ts','self-overload-calls.test.ts','java-static-field-receivers.test.ts','java-supertype-reach.test.ts','frameworks.test.ts']){
 const tree=ts.createSourceFile(file,readFileSync(join(root,'__tests__',file),'utf8'),ts.ScriptTarget.Latest,true);
 function visit(node){if((ts.isNoSubstitutionTemplateLiteral(node)||ts.isStringLiteral(node))&&/\b(?:GET|POST)\s+\S+\s+controllers\./.test(node.text))add('conf/routes',node.text,file);
 if(file!=='frameworks.test.ts'&&ts.isVariableDeclaration(node)&&['files','FILES'].includes(node.name.getText(tree))){const files=literal(node.initializer,tree);if(files&&typeof files==='object'){const chosen=Object.fromEntries(Object.entries(files).filter(([path])=>/\.(java|scala)$/.test(path)||path.includes('/conf/routes')));if(Object.keys(chosen).length)projects.push({name:`${file.replace('.test.ts','')}-${projects.length}`,files:{'build.sbt':'enablePlugins(PlayJava)\n',...chosen}});}}ts.forEachChild(node,visit);}visit(tree);
}
for(const [path,source]of [['extra.routes',"# comment\n-> /api api.Routes\nGET /hello controllers.Home.index()\nPOST /é😀 controllers.Home . save\nPATCH /data controllers..Home..patch(value: Int)\nOPTIONS / ping\nget /lower controllers.Home.index\n"],['conf/routes',"GET /x controllers.Home.index(foo: String ?= \"(\")\r\nDELETE /x @controllers.Home.remove()\nHEAD / controllers.Home.head\n"],['routes','GET /x controllers.Home.index'],['app/Foo.scala','GET /x controllers.Home.index']])add(path,source,'line and path boundaries');
projects.push({name:'owner-language-and-overloads',files:{'build.sbt':'enablePlugins(PlayScala)\n','conf/routes':'GET /x controllers.Api.run\nGET /only interfaces.Static.run\n','a/Api.scala':'package controllers\nclass Api { def run(x: Int): Int = 1 }\nobject Api { def run(): Int = 2; def run(x: String): Int = 3 }\nobject Other { def run(): Int = 9; def caller() = Api.run() }\n','b/Api.java':'package controllers;\npublic class Api { public static int run(int value) { return 1; } public static int run(String value) { return 2; } public static int run() { return 0; } }\n','b/Use.java':'package controllers;\nclass Use { int call() { return Api.run("x"); } }\n','b/Outer.java':'package controllers;\nclass Outer { static class Inner { public static int run() { return 0; } } }\n','b/Outside.java':'package elsewhere;\nimport library.Api;\nclass Outside { int call() { return Api.run(); } }\n','b/Local.java':'package elsewhere;\nimport library.Api;\nclass Api { static int run() { return 1; } }\nclass Local { int call() { return Api.run(); } }\n','b/Static.java':'package interfaces;\ninterface Static { static int run() { return 1; } }\n'}});
projects.push({name:'typed-argument-calls',files:{
'build.sbt':'enablePlugins(PlayJava)\n',
'p/Model.java':`package p;
class Model {}
interface Marker {}
class Child extends Model implements Marker {}
`,
'p/Select.java':`package p;
class Select {
 static void pick(String value) {}
 static void pick(Model value) {}
 static void pick(Model[] value) {}
 static void pick(int value) {}
 static void pick(long value) {}
 static void pick(boolean value) {}
 static void pick(char value) {}
 static void pick(double value) {}
 static void spread(String label) {}
 static void spread(String label,Object... values) {}
 static void action(java.util.function.Consumer<Model> run) {}
 static void action(java.util.function.BiConsumer<Model,Model> run) {}
 static void inherited(Marker value) {}
 static void inherited(String value) {}
}
`,
'p/Factory.java':`package p;
class Parent { static Model VALUE = new Model(); static Model make() { return VALUE; } }
class Factory extends Parent {}
`,
'p/Entry.java':`package p;
class Entry extends Parent {
 Model value;
 Model[] values;
 void run(Model input, int integer, Object other) {
  Select.pick("text");
  Select.pick(input);
  Select.pick(value);
  Select.pick(this.value);
  Select.pick(values);
  Select.pick(values[0]);
  Select.pick(integer);
  Select.pick(1L);
  Select.pick(true);
  Select.pick('x');
  Select.pick(1.5);
  Select.pick(new Model());
  Select.pick(new Model[0]);
  Select.pick((Model) other);
  Select.pick(Factory.VALUE);
  Select.pick(Factory.make());
  Select.pick(null);
  Select.spread("one");
  Select.spread("many", input, integer);
  Select.action(x -> {});
  Select.action((x,y) -> {});
  Select.inherited(new Child());
 }
}
`,
's/Overloads.scala':`package s
object Overloads {
 def run(): Int = 0
 def run(x: Int, y: Int = 1): Int = x+y
 def many(x: Int*): Int = 0
 def calls(): Int = { Overloads.run(1,2); Overloads.run(); Overloads.many(1,2,3) }
}
`}});
const parameters=[],parameterSeen=new Set();const splitInputs=["a, f(b,c), Foo<X,Y>", "x < 0 ? a : b, c", "Foo<Bar<Baz>>, \"x,y\"", "int n, String... names", "void", "", "int[] a, double d = f(1,2)", "a, 'x,y', z", "Array<Map<A,B>> v, @Ann(k=1) final String text"];
const splits=splitInputs.map(source=>({source,expected:helpers.splitCppTopLevel(source)}));
const parens=[];for(const source of ['foo(a, b)','otherFoo(); foo<T>("x)", y)','😀 foo(\n1,\n2)','foo(unterminated','before foo()'])for(const column of [0,1,3,5])parens.push({source,column,name:'foo',expected:helpers.cppParenListIn(source,column,'foo')});
for(const project of projects){const dir=mkdtempSync(join(tmpdir(),'prometeu-play-'));let indexed;try{
 for(const [path,source]of Object.entries(project.files)){mkdirSync(dirname(join(dir,path)),{recursive:true});writeFileSync(join(dir,path),source);add(path,source,project.name);}
 indexed=await Project.init(dir,{silent:true});const result=await indexed.indexAll();if(!result.success)throw Error('Index failed');project.nodes=indexed.queries.getAllNodes().map(strip);project.edges=indexed.queries.getOutgoingEdgesFrom(project.nodes.map(n=>n.id)).filter(edge=>['extends','implements'].includes(edge.kind));
 project.frameworkEdges=indexed.queries.getOutgoingEdgesFrom(project.nodes.map(n=>n.id)).filter(edge=>edge.metadata?.framework==='play');project.routes=indexed.queries.getNodesByKind('route').map(node=>({node:strip(node),targets:indexed.queries.getOutgoingEdges(node.id).map(edge=>({kind:edge.kind,node:strip(indexed.getNode(edge.target)),metadata:edge.metadata}))}));const owner=new ReferenceResolver(dir,indexed.queries);owner.initialize();const context=owner.context;
 const refs=[];for(const[path,source]of Object.entries(project.files)){const language=path.endsWith('.java')?'java':path.endsWith('.scala')?'scala':null;if(language){const extraction=extractFromSource(path,source,language);refs.push(...extraction.unresolvedReferences.map(ref=>({...ref,filePath:path,language})));}else if(path.endsWith('routes'))refs.push(...resolver.extract(path,source).references);}
 project.resolutions=refs.map(reference=>{const result=resolver.resolve(reference,context);return {reference,claimed:resolver.claimsReference(reference.referenceName),expected:result?{id:result.targetNodeId,confidence:result.confidence}:null};});
 project.overloads=[];project.arguments=[];
 for(const ref of refs.filter(ref=>ref.referenceKind==='calls')){const simple=ref.referenceName.split('.').at(-1);const candidates=project.nodes.filter(node=>['method','function'].includes(node.kind)&&node.name===simple&&['java','scala'].includes(node.language));const used=new Set();for(const node of candidates){const key=node.filePath+'\0'+node.qualifiedName;if(used.has(key))continue;used.add(key);const input={original:ref,targetNodeId:node.id,confidence:.9,resolvedBy:'framework'};const expected=helpers.retargetSelfOverload(input,ref,context);project.overloads.push({target:node.id,reference:ref,expected:expected.targetNodeId});}if(ref.language==='java'){const args=helpers.cppParenListIn((context.getFileLines(ref.filePath)??[]).slice(ref.line-1,ref.line+11).join('\n'),ref.column,simple);if(args!==null)for(const argument of helpers.splitCppTopLevel(args))project.arguments.push({source:argument,reference:ref,expected:helpers.javaArgumentType(argument,ref,context)});}}
 for(const node of project.nodes){if(node.language==='java'&&node.signature&&!parameterSeen.has(node.signature)){parameterSeen.add(node.signature);parameters.push({signature:node.signature,expected:helpers.javaParameters(node.signature)});}}
 console.log(project.name,project.resolutions.length,project.overloads.length,project.arguments.length);
}finally{indexed?.close();rmSync(dir,{recursive:true,force:true});}}
const destination=dirname(fileURLToPath(import.meta.url));writeFileSync(join(destination,'cases.json'),JSON.stringify({revision,cases,projects,parameters,splits,parens},null,2)+'\n');writeFileSync(join(destination,'LICENSE'),readFileSync(join(root,'LICENSE')));console.log(cases.length,'extractions',projects.length,'projects');
