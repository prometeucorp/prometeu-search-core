// Development-only framework binding oracle from the pinned project and resolver APIs.
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
const {ReferenceResolver}=require('./dist/resolution/index.js');
const {getFrameworkResolver}=require('./dist/resolution/frameworks/index.js');
const projects=[
 {name:'vue-bindings',files:{
  'package.json':'{"dependencies":{"nuxt":"*","vue":"*"}}',
  'tsconfig.json':'{"compilerOptions":{"baseUrl":".","paths":{"#app/*":["src/local/*"],"@/*":["src/*"]}}}',
  'src/main.ts':'export function main() {}',
  'src/local/test.ts':'export const local = 1;',
  'src/layout/index.vue':'<template><div /></template>',
  'src/ui/Card.vue':'<template><div /></template>',
  'src/ui/Button.vue':'<template><button /></template>',
  'src/ui-kit/Button.vue':'<template><button /></template>',
  'src/own.ts':'export function clearError() {}\nexport function navigateTo() {}',
  'src/imported.ts':'import {navigateTo} from "#app"; export function main() {}',
  'src/foreign.py':'def QueryFilterBuilder(): pass',
  'src/QueryFilterBuilder.vue':'<template><div /></template>',
  'main.ts':'export function main() {}',
 },queries:[]},
 {name:'astro-bindings',files:{'package.json':'{"dependencies":{"astro":"*"}}','src/pages/index.astro':'<h1>Home</h1>','src/pages/Card.astro':'<h1>Card</h1>','src/pages/Button.astro':'<button/>','src/pages-kit/Button.astro':'<button/>','src/main.ts':'export function main() {}','main.astro':'<main/>'},queries:[]},
 {name:'svelte-bindings',files:{'package.json':'{"dependencies":{"svelte":"*"}}','src/views/Screen.svelte':'<h1>Home</h1>','src/views/Card.svelte':'<div/>','src/views-kit/Card.svelte':'<div/>','src/lib/store.ts':'export const count = 1;','src/lib/helpers/index.ts':'export function run() {}','src/main.ts':'export function main() {}','main.svelte':'<main/>'},queries:[]},
];
const add=(p,framework,path,name,kind='calls')=>p.queries.push({framework,path,name,kind});
const vue=projects[0],astro=projects[1],svelte=projects[2];
for(const name of ['defineProps','defineEmits','defineExpose','defineOptions','defineSlots','defineModel','withDefaults','useRoute','useRouter','abortNavigation','useFetch','useAsyncData','useLazyFetch','useLazyAsyncData','refreshNuxtData','useState','clearNuxtState','useHead','useSeoMeta','useServerSeoMeta','useRuntimeConfig','useAppConfig','useNuxtApp','useCookie','useError','createError','showError','clearError','definePageMeta','defineNuxtConfig','defineNuxtPlugin','defineNuxtRouteMiddleware','useRequestHeaders','useRequestEvent','useRequestFetch','useRequestURL'])add(vue,'vue','src/main.ts',name);
for(const name of ['#imports','#components/foo','#app','#build/foo','#head','#app/test','@/layout','@/missing'])add(vue,'vue','src/main.ts',name,'imports');
for(const [path,name] of [['src/own.ts','clearError'],['src/ui/Card.vue','Button'],['main.ts','Button'],['src/main.ts','QueryFilterBuilder'],['src/foreign.py','QueryFilterBuilder']])add(vue,'vue',path,name);
for(const path of ['src/main.ts','src/own.ts','src/imported.ts','src/foreign.py'])add(vue,'nuxt',path,'navigateTo');
for(const name of ['Astro','Astro.props','Astro.url','astro:content','astro:assets','astro:actions','astro:env/client','astro:i18n','astro:middleware','astro:transitions','astro:components','astro:schema','astro:unknown'])add(astro,'astro','src/pages/index.astro',name,name.startsWith('astro:')?'imports':'calls');
for(const [path,name,kind] of [['src/pages/Card.astro','Button','calls'],['src/pages/Card.astro','Button','references'],['src/main.ts','Button','calls'],['main.astro','Button','calls']])add(astro,'astro',path,name,kind);
for(const name of ['$state','$state.raw','$state.snapshot','$derived','$derived.by','$effect','$effect.pre','$effect.root','$effect.tracking','$props','$bindable','$inspect','$host','$count','$$count','$unknown']){add(svelte,'svelte','src/views/Screen.svelte',name);add(svelte,'svelte','src/main.ts',name);}
for(const name of ['$app/navigation','$app/stores','$app/environment','$app/forms','$app/paths','$env/static/private','$env/static/public','$env/dynamic/private','$env/dynamic/public','$lib/store','$lib/helpers','$lib/missing','$other'])add(svelte,'svelte','src/main.ts',name,'imports');
add(svelte,'svelte','src/views/Screen.svelte','Card');add(svelte,'svelte','main.svelte','Card');
for(const project of projects){const directory=mkdtempSync(join(tmpdir(),'prometeu-bindings-'));let indexed;try{
 for(const [path,source]of Object.entries(project.files)){mkdirSync(dirname(join(directory,path)),{recursive:true});writeFileSync(join(directory,path),source);}
 indexed=await Project.init(directory,{silent:true});await indexed.indexAll();
 const resolver=new ReferenceResolver(directory,indexed.queries);resolver.initialize();
 for(const query of project.queries){const origin=resolver.context.getNodesInFile(query.path).find(node=>node.kind==='file');if(!origin)throw Error('Missing origin '+query.path);const reference={fromNodeId:origin.id,referenceName:query.name,referenceKind:query.kind,line:1,column:0,filePath:query.path,language:origin.language};const framework=getFrameworkResolver(query.framework);const result=framework.resolve(reference,resolver.context);query.reference=reference;query.expected=result?{target:result.targetNodeId,confidence:result.confidence,resolvedBy:result.resolvedBy,metadata:result.metadata??{}}:null;query.owned=framework.ownsReference?.(reference,resolver.context)??false;}
 console.log(project.name,project.queries.length);
}finally{indexed?.close();rmSync(directory,{recursive:true,force:true});}}
writeFileSync(join(dirname(fileURLToPath(import.meta.url)),'components.json'),JSON.stringify({revision,projects},null,2)+'\n');
