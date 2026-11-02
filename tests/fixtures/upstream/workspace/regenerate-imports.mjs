// Capture original import-binding scans and manifest ownership decisions.
import {createRequire,builtinModules} from 'node:module';
import {mkdtempSync,mkdirSync,writeFileSync,rmSync} from 'node:fs';
import {dirname,join,resolve} from 'node:path';
import {tmpdir} from 'node:os';
import {execFileSync} from 'node:child_process';
const upstream=resolve(process.argv[2]),revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['rev-parse','HEAD'],{cwd:upstream,encoding:'utf8'}).trim()!==revision)throw Error('Wrong revision');
const require=createRequire(join(upstream,'package.json'));
const {extractImportMappings}=require('./dist/resolution/import-resolver.js');
const {ReferenceResolver}=require('./dist/resolution/index.js');
const sources=[
'const Default = require("x").default; const {default: Named, default} = require("y");',
'import Default, {name, item as local, default as Other} from "pkg";',
'import type {Type, type Other, typeof Flow as F} from "typed";',
'import type from "types"; import defer * as ns from "deferred";',
'import typeof Thing from "flow"; import type\tThing from "types";',
'import*as ns from "x";import{a,b}from"y";',
'import {"a,b" as comma, "line\\nname" as line, "\\u{1f600}" as emoji, "\\ud83d\\ude00" as surrogate} from "strings";',
'import {"\\x41" as a, "\\q" as q, "\\u{110000}" as tooLarge, "\\u" as broken} from "escapes";',
'import {a /* ignore , */ as b, // comment\nc, "/*x*/" as string} from "lists";',
'/** @import {\n * A,\n * B as Other\n * } from "docs" */',
'const text = `import ${name} from "${source}"`; import actual from "module";',
'const text = "import Visible from \'text\'"; // import Comment from "comment";',
'import {名前 as 別名, π, $dollar, _under, é as accented} from "unicode";',
'import\uFEFFX\u00A0from\u2003"spaces"; import\u0085Bad from "control";',
'foo.import Nope from "property"; $import Nope from "identifier"; αimport Nope from "unicode";',
'import {type, type as t, typeof as f, type type, typeof Thing} from "inline";',
'import * as ns from "all"; export {thing} from "reexport"; import "side";',
'import { broken { nested } from "x"; import {ok} from "good";',
'const a = require("first"), b = require("second").member; var {x, y: z} = require("third");',
'const {x = 1, nested: local, ...rest} = require("destructure");',
'<script>import Name from "first";</script><template>Don\'t parse whole source as JS</template><script>import {Other} from "second";</script>',
'---\nimport Thing from "frontmatter";\n---\n<Thing />',
'import {"a\\\"b" as escaped, "slash\\\\name" as slash} from "escapes";',
];
const imports=sources.map(source=>({source,expected:extractImportMappings('file.ts',source,'typescript')}));
const packages=[];const pkg=JSON.stringify;
const cases=[
{name:'undeclared',files:{},from:['main.ts'],sources:['random','fs','fs/promises','node:sqlite','bun:test','https://host/x','@std/path','astro:content','$app/navigation','$env/static/private','$service-worker','@/alias','./local']},
{name:'ancestor-ownership',files:{'package.json':pkg({name:'self',dependencies:{external:'^1',local:'file:lib',space:' file:lib',collision:'^2'},devDependencies:{dev:'~2'},peerDependencies:{peer:null},optionalDependencies:{optional:3}}),'nested/package.json':pkg({name:'nested',dependencies:{collision:'workspace:*',self:'^2',other:'portal:lib'}})},from:['main.ts','nested/main.ts','nested/deep/main.ts'],sources:['self','nested','external/sub','local','space','collision','dev','peer','optional','other','@scope/unknown']},
{name:'deno-jsonc',files:{'deno.jsonc':'{// imports\n"imports":{"alias/":"npm:library@1/","@scope/pkg":"jsr:@scope/pkg@2","local":"./local.ts","remote":"https://host/x",},}','nested/deno.json':'{"imports":{"alias":"./own.ts","child":"npm:child"}}'},from:['main.ts','nested/main.ts'],sources:['alias','alias/sub','@scope/pkg/sub','local','remote','child','npm:pkg','http:thing']},
{name:'provider-repositories',files:{'package.json':pkg({name:'astro',workspaces:['framework']}),'framework/package.json':pkg({name:'@sveltejs/kit'})},from:['main.ts','framework/main.ts'],sources:['astro:content','$app/navigation','$env/x','$service-worker']},
{name:'malformed-and-array',files:{'package.json':'broken','nested/package.json':pkg({dependencies:['0','1']})},from:['main.ts','nested/main.ts'],sources:['0','1','2']},
];
for(const c of cases){const root=mkdtempSync(join(tmpdir(),'prometeu-imports-'));try{for(const[path,source]of Object.entries(c.files)){mkdirSync(dirname(join(root,path)),{recursive:true});writeFileSync(join(root,path),source);}const resolver=new ReferenceResolver(root,{});packages.push({...c,expected:c.from.flatMap(from=>c.sources.map(source=>({from,source,external:resolver.isDeclaredOutsidePackage(source,from)})))});}finally{rmSync(root,{recursive:true,force:true});}}
writeFileSync(new URL('./imports.json',import.meta.url),JSON.stringify({revision,builtins:builtinModules,imports,packages},null,2)+'\n');
