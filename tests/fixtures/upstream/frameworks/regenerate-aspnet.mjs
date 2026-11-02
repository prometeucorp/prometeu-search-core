// Development-only ASP.NET extraction/finalization reference capture.
import {createRequire} from 'node:module';
import {execFileSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdtempSync,rmSync} from 'node:fs';
import {join,resolve} from 'node:path';
import {tmpdir} from 'node:os';
const root=resolve(process.argv[2]),revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('Wrong reference revision');
const require=createRequire(join(root,'package.json')),ts=require('typescript');
const {aspnetResolver}=require('./dist/resolution/frameworks/csharp.js');
const {TreeSitterExtractor}=require('./dist/extraction/tree-sitter.js');
const {initGrammars,loadGrammarsForLanguages}=require('./dist/extraction/grammars.js');
const {DatabaseConnection}=require('./dist/db/index.js'),{QueryBuilder}=require('./dist/db/queries.js');
await initGrammars();await loadGrammarsForLanguages(['csharp']);
const cases=[],projects=[],seen=new Set(),original={};
const clean=node=>{delete node.updatedAt;return node;};
function add(path,source,origin){if(typeof path!=='string'||typeof source!=='string')return;const key=JSON.stringify([path,source]);if(seen.has(key))return;seen.add(key);const expected=aspnetResolver.extract(path,source);expected.nodes.forEach(clean);cases.push({id:`aspnet-${cases.length}`,path,source,origin,expected});}
for(const filename of ['frameworks.test.ts','fastendpoints-routes.test.ts']) {
 const tree=ts.createSourceFile(filename,readFileSync(join(root,'__tests__',filename),'utf8'),ts.ScriptTarget.Latest,true);
 function literal(node,depth=0){if(!node||depth>10)return undefined;if(ts.isStringLiteral(node)||ts.isNoSubstitutionTemplateLiteral(node))return node.text;if(ts.isIdentifier(node)){for(let block=node.parent;block;block=block.parent)for(const statement of block.statements??[]){if(statement.pos>node.pos)continue;if(ts.isVariableStatement(statement))for(const declaration of statement.declarationList.declarations)if(declaration.name.getText(tree)===node.text)return literal(declaration.initializer,depth+1);}}}
 function visit(node){if(ts.isPropertyAssignment(node)){const path=literal(node.name),source=literal(node.initializer);if(path?.endsWith('.cs')&&typeof source==='string'){add(path,source,filename);if(filename==='fastendpoints-routes.test.ts')original[path]=source;}}if(ts.isCallExpression(node)&&node.expression.getText(tree)==='aspnetResolver.extract!')add(literal(node.arguments[0]),literal(node.arguments[1]),filename);ts.forEachChild(node,visit);}visit(tree);
}
for(const source of [
 '[Route("api/[controller]")]\n[ApiController]\npublic sealed class Items {\n [HttpGet]\n [HttpPost("new", Name="n")]\n public async Task<Item[]> List() {}\n}\n',
 'class First { static string RoutePrefix = "custom/"; void Map(){ x.MapGet(Get); x.MapPut(Put, ""); x.MapDelete(Delete,"/{id}/"); } } class Second { void Map(){ x.MapPost(Create); }}',
 'app.MapGet("/café😀", NS.Handler); app.MapPost("x", () => {}); app.MapPut("", Empty); app.MapDelete("/x", handler(args));',
 'class Items : FastEndpoints.Endpoint<Request, Response> { public override void Configure(){ Get("/first", "/second"); Post(Request.Route); Put($"/{nameof(Item)}s"); } public Task HandleAsync() {} }',
 'class First : Ep.Req<Request>.Res<Response> { void Configure(){ Get("/first");\n }\n void Other(){ Post("/outside"); }\n public Task ExecuteAsync() {} }',
 '// [HttpGet("fake")]\n/* 😀 */ [HttpGet("/real")]\n public string Get() => "ok";\n',
 '[HttpGet]\n'+ ' '.repeat(580)+'public string Edge() {}\n[HttpPost]\n'+' '.repeat(610)+'public string TooFar() {}',
 'class First : EndpointWithoutResponse<Request> { void Configure(){ Get(Request.Route); Delete(unknown + dynamic); } Task ExecuteAsync() {} }',
 'class Ignore : Ordinary { void Configure(){ Get("no-endpoint"); } }\n[HttpOptions("/no")] void No() {}',
])add('Cases.cs',source,'additional boundaries, comments, Unicode and route forms');
add('Wrong.csx','[HttpGet("/x")] public string Get() {}','extension gate');
async function project(id,files){
 const raw=[];for(const [path,source]of Object.entries(files).sort(([a],[b])=>a.localeCompare(b))){if(!path.endsWith('.cs'))continue;raw.push(...new TreeSitterExtractor(path,source,'csharp').extract().nodes,...aspnetResolver.extract(path,source).nodes);}
 const byId=new Map(raw.map(node=>[node.id,clean(node)]));
 const temporary=mkdtempSync(join(tmpdir(),'prometeu-aspnet-oracle-'));let db;
 try {db=DatabaseConnection.initialize(join(temporary,'index.sqlite'));const queries=new QueryBuilder(db.getDb());queries.insertNodes([...byId.values()]);
  const context={getAllFiles:()=>Object.keys(files).sort(),readFile:path=>files[path]??null,fileExists:path=>Object.hasOwn(files,path),getNodesByKind:kind=>queries.getNodesByKind(kind).map(node=>byId.get(node.id)),getNodesByName:name=>queries.getNodesByName(name).map(node=>byId.get(node.id))};
  const expected=aspnetResolver.postExtract(context).map(clean);
  projects.push({id,files,nodes:[...byId.values()],expected});
 }finally{db?.close();rmSync(temporary,{recursive:true,force:true});}
}
await project('original-fastendpoints',original);
const groups={
 'Endpoints/Todos.cs':'public class Todos : IEndpointGroup { void Map(RouteGroupBuilder group){ group.MapGet(List); group.MapPut(Update,"{id}"); } public string List() => ""; public string Update() => ""; }',
 'Endpoints/Own.cs':'public class Own : IEndpointGroup { public string RoutePrefix => "/v2/own"; void Map(RouteGroupBuilder group){ group.MapPost(Create); } public string Create() => ""; }',
 'Extensions.cs':'var group = app.MapGroup($"/api/{groupName}");',
};
await project('group-prefix-and-override',groups);
await project('group-generic-prefix',{...groups,'Extensions.cs':'var group = app.MapGroup($"/v3/{anythingElse}");'});
await project('group-root-prefix',{...groups,'Extensions.cs':'var group = app.MapGroup($"/{groupName}");'});
await project('constant-variants',{
 'Request.cs':'public struct Request { public static readonly string Route = "items/"; }',
 'Endpoint.cs':'public class Endpoint : FastEndpoints.Endpoint<Request> { public override void Configure(){ Post(Request.Route); Get(Missing.Route); } public Task HandleAsync() {} }',
});
await project('first-owner-declaration',{
 'A.cs':'namespace A { public class Request { public static string Route = "/first"; } }',
 'B.cs':'namespace B { public class Request { public const string Route = "/second"; } }',
 'Endpoint.cs':'class Demo : Endpoint<Request> { void Configure(){ Get(Request.Route); } Task ExecuteAsync() {} }',
});
for(const count of [399,400]){const files={...groups};delete files['Extensions.cs'];for(let i=0;i<count;i++)files[`A${String(i).padStart(3,'0')}.cs`]='// MapGroup without a convention';files['Z.cs']='var group = app.MapGroup($"/api/{groupName}");';await project(`group-prefix-cap-${count}`,files);}
writeFileSync(new URL('./aspnet.json',import.meta.url),JSON.stringify({revision,cases,projects},null,2)+'\n');
console.log(cases.length,projects.length);
