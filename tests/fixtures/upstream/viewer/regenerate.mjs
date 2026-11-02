import fs from 'node:fs';
import {execFileSync} from 'node:child_process';
import path from 'node:path';
import Module from 'node:module';
const root=path.resolve(process.argv[2]),revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('Wrong reference revision');
function inspect(file, extra) { const filename=path.join(root,'dist',file); const module=new Module(filename);module.filename=filename;module.paths=Module._nodeModulePaths(path.dirname(filename));module._compile(fs.readFileSync(filename,'utf8')+'\nObject.assign(exports, {'+extra+'});',filename);return module.exports; }
const effects=inspect('ui-server/api/effects.js','FAMILIES,RECEIVER_TYPE_RULES,READ_OPS,WRITE_OPS,MODEL_SUFFIX,STATUS_BY_NAME,EXCEPTION_RESPONSE,NOT_A_RESPONSE,SWIFT_NOT_AN_EFFECT');
const rules={rules:effects.EFFECT_RULES,families:effects.FAMILIES,receiverTypes:effects.RECEIVER_TYPE_RULES,read:effects.READ_OPS,write:effects.WRITE_OPS,modelSuffix:effects.MODEL_SUFFIX,statusNames:effects.STATUS_BY_NAME,exception:effects.EXCEPTION_RESPONSE,notResponse:effects.NOT_A_RESPONSE,swiftNotEffect:effects.SWIFT_NOT_AN_EFFECT};
const replacer=(_,value)=>value instanceof RegExp?{source:value.source,flags:value.flags}:value instanceof Set?[...value]:value;
fs.mkdirSync('src/viewer', {recursive:true});
fs.writeFileSync('src/viewer/effect-rules.json',JSON.stringify(rules,replacer,2)+'\n');
const branch=inspect('graph/branch-guards.js','RULES_BY_LANGUAGE');
const config=Object.fromEntries([...branch.RULES_BY_LANGUAGE].map(([language,rules])=>[language,Object.fromEntries(['boundaries','inlineFunctions','bindingParents','blocks'].map(key=>[key,[...rules[key]]]))]));
fs.writeFileSync('src/viewer/branch-rules.json',JSON.stringify(config,null,2)+'\n');
const examples=[
['typescript',`function test(ok, blocked, x) {\n if (blocked) return;\n if (ok) { hit(); } else { hit(); }\n ok && hit();\n ok || hit();\n x ? hit() : hit();\n switch(x) { case 1: hit(); break; default: hit(); }\n for (const item of x) { hit(); }\n try { hit(); } catch(e) { hit(); }\n}`],
['typescript',`function test(a,b) { if(a) { if(!b) throw new Error(); hit(); } }`],
['typescript',`function test(a) { if(a) { list.forEach(x => hit()); const f = () => hit(); } }`],
['python',`def test(ok, blocked, x):\n    if blocked:\n        return\n    if ok:\n        hit()\n    elif x:\n        hit()\n    else:\n        hit()\n    ok and hit()\n    ok or hit()\n    hit() if ok else hit()\n    for item in x:\n        hit()\n    try:\n        hit()\n    except Exception:\n        hit()\n`],
['go',`package main\nfunc test(ok bool, blocked bool, x int) {\n if blocked { return }\n if ok { hit() } else { hit() }\n switch x { case 1: hit(); default: hit() }\n for i:=0;i<4;i++ { hit() }\n}`],
['java',`class App { void test(boolean ok, boolean blocked, int x) {\n if (blocked) return;\n if (ok) { hit(); } else { hit(); }\n switch(x) { case 1: hit(); break; default: hit(); }\n for (int i=0;i<4;i++) { hit(); }\n try { hit(); } catch(Exception e) { hit(); }\n} }`],
['csharp',`class App { void Test(bool ok, bool blocked, int x) {\n if (blocked) return;\n if (ok) { hit(); } else { hit(); }\n switch(x) { case 1: hit(); break; default: hit(); break; }\n for(int i=0;i<4;i++) { hit(); }\n try { hit(); } catch(Exception e) { hit(); }\n} }`],
['c',`void test(int ok,int blocked,int x) {\n if(blocked) return;\n if(ok) { hit(); } else { hit(); }\n switch(x) {case 1: hit();break;default:hit();}\n for(int i=0;i<4;i++) { hit(); }\n}`],
['swift',`func test(ok: Bool, blocked: Bool, x: Int) {\n guard !blocked else { return }\n if ok { hit() } else { hit() }\n switch x { case 1: hit(); default: hit() }\n for i in 0..<4 { hit() }\n}`],
['kotlin',`fun test(ok: Boolean, blocked: Boolean, x: Int) {\n if (blocked) return\n if (ok) { hit() } else { hit() }\n when(x) { 1 -> hit(); else -> hit() }\n for(i in 0..4) { hit() }\n}`]
];
const cases=[];
for(const [language,source] of examples){const sites=[];for(const match of source.matchAll(/hit\(\)/g)){const before=source.slice(0,match.index);const line=before.split('\n').length;const column=before.slice(before.lastIndexOf('\n')+1).length;sites.push({line,column,guards:await branch.guardsInSource(source,language,line,column),loops:await branch.loopsInSource(source,language,line,column)});}cases.push({language,source,sites});}
fs.writeFileSync('tests/fixtures/upstream/viewer/branches.json',JSON.stringify(cases,null,2)+'\n');
const calls=['fetch','axios.get','prisma.user.findMany','prisma.user.create','db.users.insert','usersRepository.save','OwnerRepository.save','User.objects.filter','session.exec','res.status(404).json','res.json','ResponseEntity.notFound().build','NotFoundException','HTTPException','JSONResponse','Timer.scheduledTimer','Calendar.current','Sentry.captureException','Redis.get','redis.get','fs.readFile','open','Object.create','Math.max','child_process.exec','stripe.charges.create','queue.add','sendMail','client.get','Http::post','std::fs::read_to_string','keychain.delete','URLSession.shared.data','console.log','AppData.load'];
const inputs=[];for(const language of ['typescript','python','go','java','csharp','c','cpp','ruby','php','swift','rust',null]){for(const text of calls){for(const projectType of [false,true]){const input={text,kind:'calls',language,project:'api',projectType,args:'"users", status_code=404'};inputs.push({input,effect:effects.classifyEffect(input),status:effects.responseStatus(text,input.args),implicitStatus:effects.implicitResponseStatus(text)});}}}
for(const receiverType of ['OwnerRepository','HttpClient','JwtService','Queue','MailerService','Stripe','Redis','S3Client']) {const input={text:'owners.save',kind:'calls',language:'java',receiverType};inputs.push({input,effect:effects.classifyEffect(input),status:effects.responseStatus(input.text,input.args),implicitStatus:effects.implicitResponseStatus(input.text)});}
fs.writeFileSync('tests/fixtures/upstream/viewer/effects.json',JSON.stringify(inputs,null,2)+'\n');
const program=inspect('ui-server/api/program.js','');
const programs=[];
for(let c=0;c<cases.length;c++){const source=cases[c];const sites=source.sites.map((site,index)=>({step:'s'+index,link:'e'+index,at:{line:site.line,column:site.column,end:{line:site.line,column:site.column+5}},guards:site.guards,loops:site.loops}));const steps=Object.fromEntries(sites.map((site,index)=>[site.step,{reply:index%3===1,into:null}]));const input={name:source.language+'-'+c,root:'root',sites:{root:sites},nodes:{},steps};input.expected=program.buildProgram({root:input.root,sites:new Map(Object.entries(input.sites)),node:id=>input.nodes[id]??null,step:id=>input.steps[id]??null});programs.push(input);}
const input={name:'inline-and-deferred',root:'root',sites:{root:[{into:'helper',at:{line:1,column:0,end:{line:1,column:20}},guards:[]},{step:'a',trigger:{kind:'callback',name:'setTimeout'},at:{line:2,column:0,end:{line:2,column:20}},guards:[]},{step:'b',within:'Promise.all',at:{line:3,column:0,end:{line:3,column:20}},guards:[]},{into:'helper',at:{line:4,column:0,end:{line:4,column:20}},guards:[]}],helper:[{step:'a',at:{line:1,column:0,end:{line:1,column:20}},guards:[]}]},nodes:{helper:{id:'helper',name:'helper'}},steps:{a:{reply:false,into:null},b:{reply:true,into:null}}};input.expected=program.buildProgram({root:input.root,sites:new Map(Object.entries(input.sites)),node:id=>input.nodes[id]??null,step:id=>input.steps[id]??null});programs.push(input);
fs.writeFileSync('tests/fixtures/upstream/viewer/programs.json',JSON.stringify(programs,null,2)+'\n');
const callExamples=[
['typescript','class Api { constructor(private owners: OwnerRepository) {}\n run() { this.owners.save(user); Promise.all([fetch(url), fetch(other)]);\n res.status(202); res.json(user);\n return Response.json(user, {status: 201});\n } }',['this.owners.save','fetch(url)','fetch(other)','res.json','Response.json']],
['java','class Api { private OwnerRepository owners;\n void run() { owners.save(user); } }',['owners.save']],
['csharp','class Api { OwnerRepository owners;\n void Run() { owners.Save(user); } }',['owners.Save']],
['kotlin','class Api(val owners: OwnerRepository) {\n fun run() { owners.save(user) } }',['owners.save']],
['typescript','function run(ok) { if (ok) res.status(400); res.json(user);\n res.statusCode = 204; res.end(); }',['res.json','res.end']]
];
const callCases=[];
for (const [language,source,needles] of callExamples){const sites=[];for(const needle of needles){const offset=source.indexOf(needle),before=source.slice(0,offset),line=before.split('\n').length,column=before.slice(before.lastIndexOf('\n')+1).length;sites.push({line,column,call:await branch.callSiteInSource(source,language,line,column),members:Object.fromEntries(await branch.memberTypesInSource(source,language,line))});}callCases.push({language,source,sites});}
fs.writeFileSync('tests/fixtures/upstream/viewer/calls.json',JSON.stringify(callCases,null,2)+'\n');
