// Capture full upstream pass records and local-declaration answers; no Rust outputs involved.
import {createRequire} from 'node:module';
import {execFileSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdirSync,mkdtempSync,rmSync} from 'node:fs';
import {join,dirname,resolve} from 'node:path';
import {tmpdir} from 'node:os';
import {runInNewContext} from 'node:vm';
const root=resolve(process.argv[2]),revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('Wrong reference revision');
const require=createRequire(join(root,'package.json')),ts=require('typescript');
const {extractImportMappings}=require('./dist/resolution/import-resolver.js');
const {goFuncFieldEdges}=require('./dist/resolution/go-func-field-synthesizer.js');
const {goLocalDeclarationAt,blankGoCode}=require('./dist/resolution/name-matcher.js');
const {synthesizeCallbackEdges}=require('./dist/resolution/callback-synthesizer.js');
const {DatabaseConnection}=require('./dist/db/index.js'),{QueryBuilder}=require('./dist/db/queries.js');
const {ReferenceResolver}=require('./dist/resolution/index.js');
const {extractFromSource}=require('./dist/extraction/tree-sitter.js');
const {initGrammars,loadGrammarsForLanguages}=require('./dist/extraction/grammars.js');
await initGrammars();await loadGrammarsForLanguages(['go']);
const projects=[];
for(const filename of ['go-func-field-dispatch.test.ts','go-local-shadows-function.test.ts','go-asserted-locals.test.ts']) {
 const source=readFileSync(join(root,'__tests__',filename),'utf8'),tree=ts.createSourceFile(filename,source,ts.ScriptTarget.Latest,true);
 function visit(node){if(ts.isVariableDeclaration(node)&&node.name.getText(tree)==='FILES'&&node.initializer){const files=runInNewContext(`(${node.initializer.getText(tree)})`,{}, {timeout:1000});projects.push({id:filename,files});}ts.forEachChild(node,visit);}visit(tree);
}
const go=(source)=>`package p\n${source}\n`;
for(const [id,source] of Object.entries({
 'typed-aliases':`type Handler func(int)\ntype Alias = Options\ntype Options struct { F Handler; Ptr *Handler; G func() }\nfunc h(int) {}\nfunc wire() { o:=Alias{F:h}; o.F(1); var x Options; x.F=h; x.F(2) }`,
 'embedded-shadowing':`type Inner struct { F func() }\ntype Outer struct { Inner }\ntype Shadow struct { Inner; F int }\ntype Other struct { F func() }\ntype Both struct { Inner; Other }\nfunc work() {}\nfunc wire() { i:=Inner{F:work}; o:=Outer{i}; o.F(); s:=Shadow{}; s.F(); b:=Both{}; b.F() }`,
 'selector-hop':`type Config struct { F func() }\ntype Server struct { opts Config }\nfunc work() {}\nfunc wire() { c:=Config{F:work}; s:=Server{opts:c}; s.opts.F() }`,
 'scope-masks':`type S struct { F func() }\nfunc work() {}\nfunc fire(s S) {\n s.F()\n { s := 3; s.F() }\n if s := 3; s>0 {\n s.F()\n } else {\n s.F()\n }\n s.F()\n text := "s.F() 😀"\n _ = text\n}\nfunc wire() { _=S{F:work} }`,
 'result-inference':`type S struct { F func() }\nfunc work() {}\nfunc makeS() (s S, err error) { return S{F:work},nil }\nfunc call() {\n a,err:=makeS(); a.F(); _=err\n var b = new(S); b.F=work; b.F()\n var any interface{}; c,ok:=any.(*S); c.F(); _=ok\n}`,
 'func-conversion':`type Handler func()\ntype Other int\ntype S struct { F Handler }\nfunc work() {}\nfunc call() { a:=S{F:Handler(work)}; a.F(); b:=S{F:Other(work)}; b.F() }`,
 'copy-depth':`type S struct { A func(); B func(); C func() }\nfunc work() {}\nfunc wire(s S) { s.A=work; s.B=s.A; s.C=s.B; s.A(); s.B(); s.C() }`,
 'parameter-groups':`type S struct { F func() }\nfunc work() {}\nfunc call(a,b S,c ...S) (out S) { a.F();b.F();c.F();out.F();return }\nfunc wire(){_=S{F:work}}`,
 'case-scopes':`type S struct { F func() }\nfunc work() {}\nfunc call(s S,ch chan S) {\n select {\n case s:=<-ch:\n s.F()\n default:\n s.F()\n }\n switch k:=1;k {\n case 1:\n var s int\n s.F()\n default:\n s.F()\n }\n s.F()\n}\nfunc wire(){_=S{F:work}}`,
 'generated-files':`// a regular source\ntype S struct { F func() }\nfunc work() {}\nfunc call(s S){s.F()}\nfunc wire(){_=S{F:work}}`
}))projects.push({id,files:{'go.mod':'module example.com/p\n','main.go':go(source)}});
projects.push({id:'cap-and-order',maxPerChannel:2,files:{'go.mod':'module example.com/p\n','main.go':go(`type S struct { F func() }\nfunc a() {}\nfunc b() {}\nfunc c() {}\nfunc call(s S){s.F()}\nfunc wire(){_=S{F:a};_=S{F:b};_=S{F:c}}`),'early_test.go':go('func TestWire(){_=S{F:c}}')}});
const cases=[];
for(const project of projects){const temporary=mkdtempSync(join(tmpdir(),'prometeu-go-field-oracle-'));const db=DatabaseConnection.initialize(join(temporary,'index.sqlite'));try{
 const queries=new QueryBuilder(db.getDb());
 for(const [path,source]of Object.entries(project.files)){mkdirSync(dirname(join(temporary,path)),{recursive:true});writeFileSync(join(temporary,path),source);if(!path.endsWith('.go'))continue;const result=extractFromSource(path,source,'go');queries.insertNodes(result.nodes);queries.insertEdges(result.edges);queries.insertUnresolvedRefsBatch(result.unresolvedReferences.map(ref=>({...ref,filePath:path,language:'go'})));queries.upsertFile({path,contentHash:'oracle',language:'go',size:Buffer.byteLength(source),indexedAt:0,modifiedAt:0,nodeCount:result.nodes.length});}
 const resolver=new ReferenceResolver(temporary,queries);resolver.initialize();await resolver.resolveAndPersistBatched(undefined,5000,undefined,undefined,false);
 await synthesizeCallbackEdges(queries,resolver.context); // Establish real containment and interface prerequisites.
 const ctx=resolver.context,nodes=queries.getAllNodes(),baseEdges=nodes.flatMap(node=>queries.getOutgoingEdges(node.id)).filter(e=>!e.metadata?.synthesizedBy||['go-method-contains','go-implements'].includes(e.metadata.synthesizedBy)),imports=[],packages=[];
 for(const file of Object.keys(project.files).filter(p=>p.endsWith('.go')))for(const binding of ctx.getImportMappings(file,'go')){imports.push({file,local_name:binding.localName,source:binding.source,imported_name:binding.exportedName??'',is_default:binding.isDefault,is_namespace:binding.isNamespace,resolved_path:null,mapped_path:null,external:false});packages.push({file,source:binding.source,dir:ctx.getGoPackageDir(binding.source,file)??null});}
 const expected=await goFuncFieldEdges(queries,ctx,async()=>{},project.maxPerChannel??40),scopes=[];
 for(const [file,source]of Object.entries(project.files).filter(([p])=>p.endsWith('.go'))){const blank=blankGoCode(source);for(const match of blank.matchAll(/[A-Za-z_]\w*/g)){const at=match.index;const before=source.slice(0,at);const line=before.split('\n').length,column=at-(before.lastIndexOf('\n')+1);const hit=goLocalDeclarationAt(match[0],{filePath:file,line,column},ctx);scopes.push({file,name:match[0],offset:Buffer.byteLength(before),expected:hit??null});}}
 for(const node of nodes)delete node.updatedAt;
 cases.push({...project,nodes,baseEdges,imports,packages,scopes,expected});console.log(project.id,expected.length,scopes.length);
}finally{db.close();rmSync(temporary,{recursive:true,force:true});}}
const importCases=[
`package p
import (
 "github.com/pb/v3"
 v3 "example.com/client/v3"
 . "fmt"
 _ "os"
 "gopkg.in/yaml.v3"
 "github.com/mattn/go-sqlite3"
)
func f() { text := \`import "fake"\` }`,
`package p
import "x/foo/v2"
import "y/foo/v3"
import "z/go-über"
`,
`package p
/*
func fake(){}
*/
import ("fmt" // a trailing comment
 "os")
var x = "import fake"`,
`package p
import custom "example.com/path"
import "custom"
import . "dot/pkg"
`,
];
const imports=importCases.map(source=>({source,expected:extractImportMappings('main.go',source,'go')}));
const sites=['a.go:2','a.go:10','a.go:0002','A.go:2','_a.go:2','-a.go:2','a_b.go:2','a-b.go:2','a b.go:2','á.go:2','á.go:2','æ.go:2','ä.go:2','😀.go:2','𐐀.go:2','𐐨.go:2','file٢.go:2','file2.go:2','file१०.go:2','file9.go:2','東京.go:2','!file.go:2'];
const ordering=sites.flatMap(left=>sites.map(right=>({left,right,expected:Math.sign(left.localeCompare(right,'en',{numeric:true}))})));
writeFileSync(new URL('./go-func-field.json',import.meta.url),JSON.stringify({revision,cases,imports,ordering},null,2)+'\n');
