// Capture indexed lexical results from the pinned project's public indexing pipeline.
import {createRequire} from 'node:module';
import {mkdirSync,mkdtempSync,rmSync,writeFileSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import {tmpdir} from 'node:os';
import {dirname,join,resolve} from 'node:path';
const upstream=resolve(process.argv[2]);const revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['rev-parse','HEAD'],{cwd:upstream,encoding:'utf8'}).trim()!==revision)throw Error('Unexpected revision');
const require=createRequire(join(upstream,'package.json'));
const Project=Object.values(require('./dist/index.js')).find(v=>typeof v==='function'&&typeof v.init==='function'&&typeof v.open==='function');
const projects=[{name:'typed-symbols',files:{'package.json':'{"name":"@scope/ShopBackend"}',
'src/auth.ts':'/** Authenticate a user and create a session. */\nexport function authenticate(user: string): boolean { return user.length > 0; }\nexport class AuthService { login(user: string) { return authenticate(user); } }\n',
'src/session.ts':'/** Track the active account. */\nexport interface Session { user: string; }\nexport function logout(session: Session) { return session.user; }\n',
'examples/auth.ts':'export function authenticate() { return false; }\n',
'src/user.py':'def get_user(name: str):\n    """Find a user account."""\n    return name\nclass UserStore:\n    def save(self, user):\n        return user\n',
'src/worker.rs':'pub mod stage_apply { pub fn run() {} }\n',
},queries:['auth','authenticate','kind:function auth','lang:python user','session','account','login','getUssr','Store','stage_apply::run','kind:interface','kind:file path:auth.ts','path:src name:auth','ShopBackend auth','" OR * ()']},
{name:'go-docs',files:{'go.mod':'module example.org/TaskEngine\n\ngo 1.22\n',
'worker.go':'package task\n// Worker processes requests.\ntype Worker struct { Active bool }\n// Process runs pending work.\nfunc (w Worker) Process(name string) bool { return w.Active }\nfunc Run() {}\n',
'worker_test.go':'package task\nfunc TestProcess() {}\n'},queries:['Worker','Process','requests','pending','lang:go kind:method','TaskEngine pending','test Process','Proces','kind:file']},
{name:'unicode-and-components',files:{
'src/café.ts':'export function café() { return "bonjour"; }\nexport class État { avancer() {} }\n',
'ui/Panel.vue':'<script setup lang="ts">\nfunction handleClick() {}\n</script>\n<template><button @click="handleClick">Go</button></template>\n',
'view/Widget.svelte':'<script>\nfunction showWidget() {}\n</script>\n<button on:click={showWidget}>Show</button>\n',
},queries:['café','État','name:CAFÉ','path:ÉTAT','handleClick','Panel','Widget','kind:component','lang:vue']}
];
const cases=[];
for(const project of projects){
 const root=mkdtempSync(join(tmpdir(),'prometeu-search-project-'));let indexed;
 try{for(const [file,source] of Object.entries(project.files)){mkdirSync(dirname(join(root,file)),{recursive:true});writeFileSync(join(root,file),source);}
 indexed=await Project.init(root,{silent:true});const result=await indexed.indexAll();if(!result.success)throw Error(`Failed ${project.name}`);
 const queries=[];for(const query of project.queries)for(const limit of [1,5,20])queries.push({query,limit,expected:indexed.queries.searchNodes(query,{limit}).map(r=>({id:r.node.id,name:r.node.name,kind:r.node.kind,path:r.node.filePath,startLine:r.node.startLine,score:r.score}))});
 cases.push({...project,queries});
 }finally{indexed?.close();rmSync(root,{recursive:true,force:true});}
}
writeFileSync(new URL('./projects.json',import.meta.url),JSON.stringify({revision,cases},null,2)+'\n');
