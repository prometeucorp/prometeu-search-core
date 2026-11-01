// Development-only framework indexing oracle; expected outputs come from the pinned project API.
import {execFileSync} from 'node:child_process';
import {createRequire} from 'node:module';
import {mkdirSync,mkdtempSync,rmSync,writeFileSync} from 'node:fs';
import {dirname,join,resolve} from 'node:path';
import {tmpdir} from 'node:os';
import {fileURLToPath} from 'node:url';
const root=resolve(process.argv[2]),revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('Wrong reference revision');
const require=createRequire(join(root,'package.json'));
const Project=Object.values(require('./dist/index.js')).find(value=>typeof value==='function'&&typeof value.init==='function'&&typeof value.open==='function');
const projects=[
 {name:'nuxt-pages',files:{'package.json':'{"dependencies":{"nuxt":"*"}}','pages/index.vue':'<template><div>Home</div></template>','pages/admin.vue':'<template><div>Admin</div></template>','pages/orders/index.vue':'<template><div>Orders</div></template>','components/index.vue':'<template><div>Other</div></template>','middleware/auth.ts':'export default function auth() {}','server/api/[id].ts':'export default function handler() {}'}},
 {name:'astro-pages',files:{'package.json':'{"dependencies":{"astro":"*"}}','src/pages/index.astro':'<h1>Home</h1>','src/pages/blog/index.astro':'<h1>Blog</h1>','src/pages/_skip.astro':'<h1>Hidden</h1>','src/pages/api/hello.ts':'export const GET = async () => new Response("hi");\nexport async function POST() { return new Response("ok"); }','src/components/index.astro':'<h1>Other</h1>'}},
 {name:'fastapi-prefixes',files:{'main.py':`from fastapi import FastAPI
from api import api_router
app=FastAPI()
app.include_router(api_router,prefix="/api/v1")
`,'api.py':`from fastapi import APIRouter
from routes import items
api_router=APIRouter()
api_router.include_router(items.router)
`,'routes/__init__.py':'','routes/items.py':`from fastapi import APIRouter
router=APIRouter(prefix="/items")
@router.get("/{id}")
def read(): return 1
`}},
 {name:'fastapi-aliases',files:{'main.py':`from fastapi import FastAPI
from routes import router as items_router
app=FastAPI()
app.include_router(items_router,prefix="/api")
`,'routes.py':`from fastapi import APIRouter
router=APIRouter(prefix="/items")
@router.post("")
def create(): return 1
`}},
 {name:'fastapi-ambiguous-mount',files:{'main.py':`from fastapi import FastAPI
from routes import router
app=FastAPI()
app.include_router(router,prefix="/one")
app.include_router(router,prefix="/two")
`,'routes.py':`from fastapi import APIRouter
router=APIRouter(prefix="/items")
@router.get("")
def list_items(): return []
`}},
 {name:'fastapi-computed-prefix',files:{'main.py':`from fastapi import FastAPI
from routes import router
app=FastAPI()
app.include_router(router,prefix=settings.PREFIX)
`,'routes.py':`from fastapi import APIRouter
router=APIRouter(prefix="/items")
@router.get("")
def list_items(): return []
`}},
 {name:'spring',files:{'pom.xml':'<project><dependency>spring-boot</dependency></project>','Controller.java':`@RestController
@RequestMapping("/items")
class Controller {
 @GetMapping("/{id}")
 public String get() {return "";}
 @Value("\${app.cache-name}") String cache;
}
@ConfigurationProperties(prefix="app.cache")
class CacheProperties {}
`,'application.yml':'app:\n  cache-name: production\n  cache-size: 10\n','application-dev.properties':'app.cache-name=development\n'}},
 {name:'django',files:{'manage.py':'','urls.py':`from app import views\nfrom django.urls import path\nurlpatterns = [path('items/', views.list_items)]\n`,'app/views.py':'def list_items():\n    return []\n'}},
 {name:'flask',files:{'app.py':`from flask import Flask\napp=Flask(__name__)\n@app.route('/items', methods=['POST'])\ndef create(): return 1\napp.add_url_rule('/list', view_func=create)\n`}},
 {name:'fastapi',files:{'main.py':`from fastapi import FastAPI\napp=FastAPI()\n@app.get('/items')\nasync def list_items(): return []\n`}},
 {name:'go',files:{'go.mod':'module example.local/server\n','server.go':`package main\nimport "net/http"\nfunc list(w http.ResponseWriter, r *http.Request) {}\nfunc routes(){ http.HandleFunc("GET /items",list) }\n`}},
 {name:'rust',files:{'Cargo.toml':'[package]\nname="server"\nversion="0.1.0"\n','src/lib.rs':`async fn list() {}\nfn routes(){ Router::new().route("/items", get(list)); }\n`}},
 {name:'vapor-top-level',files:{'routes.swift':`import Vapor\nfunc list(_ req: Request) {}\nfunc routes(_ app: Application) {\n app.get("items", use: list)\n}\n`}},
 {name:'vapor-controllers',files:{'routes.swift':`import Vapor\nfunc routes(_ app: Application) {\n let a = FirstController()\n app.get("first", use: a.show)\n app.get("second", use: SecondController.show)\n}\n`,'First.swift':`struct FirstController {\n func show(_ req: Request) {}\n func show(_ value: String) {}\n}\n`,'Second.swift':`struct SecondController {\n static func show(_ req: Request) {}\n}\n`}},
 {name:'vapor-local-extension',files:{'Routes.swift':`import Vapor\nstruct Controller {\n func boot(routes: RoutesBuilder) {\n routes.get("local", use: self.index)\n }\n}\n`,'Extension.swift':`extension Controller {\n func index(_ req: Request) {}\n}\n`,'Other.swift':`struct Other {\n func index(_ req: Request) {}\n}\n`}},
 {name:'vapor-nested-extension',files:{'Routes.swift':`import Vapor\nfunc routes(_ app: Application) {\n app.get("nested", use: API.Controller.show)\n}\n`,'Extension.swift':`extension API.Controller {\n static func show(_ req: Request) {}\n}\n`,'Other.swift':`struct Controller {\n static func show(_ req: Request) {}\n}\n`}},
];
for(const project of projects){const dir=mkdtempSync(join(tmpdir(),'prometeu-framework-'));let indexed;try{
 for(const [path,source]of Object.entries(project.files)){mkdirSync(dirname(join(dir,path)),{recursive:true});writeFileSync(join(dir,path),source);}
 indexed=await Project.init(dir,{silent:true});const result=await indexed.indexAll();if(!result.success)throw Error('Index failed');
 const capture=node=>{delete node.updatedAt;return{node,targets:indexed.queries.getOutgoingEdges(node.id).map(edge=>({kind:edge.kind,node:indexed.getNode(edge.target),metadata:edge.metadata})).map(target=>{delete target.node.updatedAt;return target;})};};
 project.routes=indexed.queries.getNodesByKind('route').map(capture);
 project.bindings=indexed.queries.getNodesByKind('constant').filter(node=>node.id.startsWith('spring-value:')||node.id.startsWith('spring-cp:')).map(capture);
 for(const binding of project.bindings){
   const canonical=key=>key.toLowerCase().replace(/[-_]/g,'');const prefix=binding.node.id.startsWith('spring-cp:');const wanted=canonical(binding.node.name);
   let candidates=indexed.queries.getNodesByKind('constant').filter(node=>['yaml','properties'].includes(node.language)&&(prefix?canonical(node.qualifiedName).startsWith(wanted):canonical(node.qualifiedName)===wanted));
   if(prefix){const length=Math.min(...candidates.map(node=>canonical(node.qualifiedName).length));candidates=candidates.filter(node=>canonical(node.qualifiedName).length===length);}
   if(candidates.length>1)binding.uncertainTargets=candidates.map(node=>node.id).sort();
 }
 console.log(project.name,project.routes.length);
}finally{indexed?.close();rmSync(dir,{recursive:true,force:true});}}
writeFileSync(join(dirname(fileURLToPath(import.meta.url)),'projects.json'),JSON.stringify({revision,projects},null,2)+'\n');
