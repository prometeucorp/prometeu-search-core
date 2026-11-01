// Reference SQLite execution and pure helpers from the pinned upstream checkout.
import {createRequire} from 'node:module';
import {mkdtempSync,rmSync,writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const root=resolve(process.argv[2]);
const revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['rev-parse','HEAD'],{cwd:root,encoding:'utf8'}).trim()!==revision)throw Error('Unexpected reference revision');
const require=createRequire(join(root,'package.json'));
const {DatabaseConnection}=require('./dist/db/index.js');
const {QueryBuilder}=require('./dist/db/queries.js');
const parser=require('./dist/search/query-parser.js');
const utils=require('./dist/search/query-utils.js');
const segments=require('./dist/search/identifier-segments.js');
const strings=['','a','GET','getUserName','HTTPServer2','HTMLParser','base64Encode','foo_bar','user-service.ts','caching errors handled getters carried entries processes class management','running eviction expression','NavMeshAgent类型','寻路服务.计算路径','ユーザー取得','로그인','état ÉtatMachine caféCafe','cafe\u0301API','áAuthΣΔ AbΓamma','𐐀𐐨Value','ⅠValue','mixedーwords々','١٢name١Name','\u0085abc\ufeffdef','help me with references and resolution','références résolution','atBottom tracking','classes boxes hashes quizzes patches caches lenses databases heroes shoes services machines cookies','getUser name:auth path:"src/my folder" kind:function lang:PYTHON','KIND:Function kind:function kind:unknown language:C# lang:csharp','name:"" path: name:"unfinished here','"word phrase" foo:bar TODO: kind:class','path:"a b"suffix next','path:""foo','foo foo foo','SuperBizAgent backend routes','test spec testsuite','a::b::run','and OR NOT NEAR','leaf(entry)','" OR * ()','foo\u0000bar'];
strings.push("😀😀ing", "😀😀er", ...require("./dist/types.js").LANGUAGES.map(l=>`lang:${l}`), ...require("./dist/types.js").NODE_KINDS.map(k=>`kind:${k}`));
const helpers=strings.map(text=>({text,parsed:parser.parseQuery(text),terms:utils.extractSearchTerms(text),baseTerms:utils.extractSearchTerms(text,{stems:false}),cjk:utils.hasCjk(text),switches:utils.splitAtCjkSwitches(text),floor:utils.meetsQueryWordFloor(text),name:utils.isQueryNameToken(text),blank:utils.blankNonAsciiWords(text),distinctive:utils.isDistinctiveIdentifier(text),stem:utils.getStemVariants(text),segments:segments.splitIdentifierSegments(text),normalized:segments.normalizeProseWord(text),prose:segments.extractProseCandidates(text),segmentWords:segments.extractSegmentSearchWords(text),variants:segments.segmentLookupVariants(text)}));
const scores=[];
for(const query of strings.filter(s=>s.length<70))for(const name of ['getUser','getUserName','CacheBuilder','build','Pod','PodGCControllerOptions','café','寻路服务',''])scores.push({query,name,bonus:utils.nameMatchBonus(name,query)});
const paths=['src/auth.ts','auth/user.ts','src/get_user_name.ts','examples/auth.ts','tests/auth.test.ts','pkg/src/main/kotlin/com/samples/auth.ts','SuperBizAgentFrontend/routes.ts','backend/routes.ts','/auth/','auth','README.md'];
const pathScores=[];
for(const query of ['auth','auth auth','getUserName','SuperBizAgent backend routes','SuperBizAgent','tests auth','the and','', 'café 用户'])for(const path of paths)for(const deprioritized of [false,true])pathScores.push({path,query,deprioritized,expected:utils.scorePathRelevance(path,query,new Set(['superbizagent']),deprioritized)});
const distances=[];for(const a of ['getUser','getUssr','abcd','abdc','foo','fóo','😀x','𐐀',''])for(const b of ['getUser','getUssr','abc','😀y','𐐨',''])for(const bound of [0,1,2])distances.push({a,b,bound,expected:parser.boundedEditDistance(a,b,bound)});
const nodes=[];
function node(name,kind='function',filePath='src/auth.ts',extra={}){const id=`${filePath}:${name}:${nodes.length}`;nodes.push({id,name,kind,filePath,qualifiedName:name,language:'typescript',startLine:1,endLine:2,startColumn:0,endColumn:0,updatedAt:0,...extra});}
node('auth');node('authenticate');node('Authentication','class');node('AuthService','interface');node('login','method','src/user.ts',{qualifiedName:'User.login',docstring:'Authenticate a user with password and session'});node('usage','function','examples/usage.ts');node('usageReport');node('auth','function','tests/auth.test.ts');node('auth','function','vendor/auth.ts');node('getUser');node('getUserDescriptor');node('User','class');node('get_user','function','src/users.py',{language:'python',signature:'def get_user(name: str) -> User'});node('leaf','function','src/tree.ts',{signature:'leaf(entry: Entry): boolean'});node('run','function','src/lib.rs',{qualifiedName:'stage_apply::run',language:'rust'});node('café','function','src/État.ts');node('Café','class','src/état.ts');node('ÉTAT','function','src/ÉTAT.ts');node('用户服务','class','src/用户.ts');node('foo_bar');node('foo%bar');node('dots.name');node('literal_underscore','constant');node('notes.ts','file','docs/notes.ts');node('backendRoutes','function','backend/routes.ts');node('routes','function','SuperBizAgentFrontend/main.ts');
for(let i=0;i<130;i++)node(`getBeanDescriptor${i}`,'function',`generated/bean${i}.ts`);
node('getBean','method','src/context.ts',{docstring:'getBean '.repeat(120)});
for(let i=0;i<60;i++)node(`getter${String(i).padStart(2,'0')}`,'function',i===59?'rare/place.ts':`generated/getter${i}.ts`);
const queries=['auth','auth kind:function','kind:class','lang:PYTHON','language:rust','path:"src/ÉTAT"','name:CAFÉ','path:src name:user','path:src path:vendor name:auth','kind:function kind:class auth','kind:Function','TODO:','getUssr','uthenti','leaf(entry)','stage_apply::run','getBean','getBean path:src','getterxy path:rare','%bar','foo_bar','name:%','name:_','用户服务','café','ÉTAT','usage','usageReport','usage test','SuperBizAgent backend routes','SuperBizAgent','','" OR * ()','foo\u0000bar'];
const temporary=mkdtempSync(join(tmpdir(),'prometeu-search-oracle-'));const db=DatabaseConnection.initialize(join(temporary,'oracle.sqlite'));
const ranked=[];
try{const q=new QueryBuilder(db.getDb());q.setProjectNameTokens(new Set(['superbizagent']));q.setDeprioritizedPathMatcher(path=>path.startsWith('vendor/')||path.startsWith('examples/'));
for(const path of new Set(nodes.map(n=>n.filePath)))q.upsertFile({path,contentHash:'fixture',language:'typescript',size:1,modifiedAt:0,indexedAt:0,nodeCount:0});q.insertNodes(nodes);
for(const query of queries)for(const limit of [1,5,20])ranked.push({query,limit,expected:q.searchNodes(query,{limit}).map(r=>({id:r.node.id,score:r.score}))});
for(const query of ['auth','kind:class','path:"src/ÉTAT"','name:CAFÉ','path:src name:user','getBean','getBean path:src','getterxy path:rare','getUssr','uthenti'])for(const limit of [1,5,100])for(const offset of [1,3,60,500])ranked.push({query,limit,offset,expected:q.searchNodes(query,{limit,offset}).map(r=>({id:r.node.id,score:r.score}))});
}finally{db.close();rmSync(temporary,{recursive:true});}

const queryPaths=require('./dist/search/query-paths.js');
const indexed=['src/routes/(private)/[id]/+page.svelte','lib/a.ts','lib/b.ts','lib/editorOptions.ts','ui/editorOptions.ts','a/models.py','b/models.py','c/models.py','d/models.py','compiler.py','lib/background-image-table.ts','styles/background-image-table.module.scss','hooks/pre-commit','a/foo-bar.ts','b/foo-bar.ts','c/foo-bar.ts','d/foo-bar.ts','src/État.ts','src/état.ts'];
const symbolMap={clampedInt:['lib/editorOptions.ts'],SQLCompiler:['a/models.py'],OtherThing:['b/models.py'],SomeVar:['a/foo-bar.ts']};
const pathQueries=['','  none  ','src/routes/(private)/[id]/+page.svelte scroll','`lib/a.ts:12-40` rest','compiler.py#L88-L120','compiler.py lines 900–1003','L900-L1003 of compiler.py','compiler.py lines 900 to 1003','lib/a.ts 10 lib/b.ts line 40','editorOptions.ts clampedInt()','editorOptions.ts render','models.py SQLCompiler','models.py','models.py SQLCompiler OtherThing','background-image-table','foo-bar SomeVar','foo-bar','(lib/a.ts)','[lib/a.ts','lib/a.ts).','././lib//a.ts/','/Users/me/dev/repo/lib/a.ts','C:\\repo\\lib\\a.ts','old/missing.ts stale','scripts/deploy and/or','pre-commit:8','src/ÉTAT.ts','lib/a.ts:0 lib/b.ts:1000001','lib/a.ts:0000000000000000000000012','lib/a.ts:40-12','foo.bar --flag snake_case-hyphen','app.isPackaged','editorOptions.ts clampedInt lib/ui/editorOptions.ts','lib/a.ts L10 L10','lines 10 through 20 lib/a.ts','compiler.py line 20..10','compiler.py lines 5 to 1000001','lib/a.ts unrelated 2026','lib/a.ts line 2, L4!'];
const pathCases=[];
for(const query of pathQueries)for(const mode of ['plain','symbols','disk','budget']){
const options=mode==='symbols'?{symbolFiles:s=>symbolMap[s]??[]}:mode==='disk'?{existsOnDisk:p=>p==='scripts/deploy'}:mode==='budget'?{maxPins:1,maxMatchesPerSpan:1}:{};
pathCases.push({query,indexed,mode,symbolMap,might:queryPaths.queryMightContainPaths(query),expected:queryPaths.extractQueryPaths(query,indexed,options)});
}

writeFileSync(new URL('./cases.json',import.meta.url),JSON.stringify({revision,helpers,scores,pathScores,distances,nodes,ranked,pathCases},null,2)+'\n');
