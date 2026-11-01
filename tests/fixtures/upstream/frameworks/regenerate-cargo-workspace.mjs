// Independent Cargo workspace answers from the pinned JavaScript runtime.
import {createRequire} from 'node:module';
import {execFileSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdirSync,mkdtempSync,rmSync} from 'node:fs';
import {join,resolve,dirname,posix} from 'node:path';
const root=resolve(process.argv[2]),revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('Wrong reference revision');
const require=createRequire(join(root,'package.json')),pm=require('picomatch');
const cargo=require('./dist/resolution/frameworks/cargo-workspace.js');
const rust=Object.values(require('./dist/resolution/frameworks/rust.js')).find(v=>v?.name==='rust');
const clean=value=>JSON.parse(JSON.stringify(value,(key,value)=>key==='updatedAt'?undefined:value));
const segments=['*','**','?','??','[ab]','[a-z]','[^b]','[!b]','[[:alpha:]]','[[:alnum:]]','[[:blank:]]','[[:cntrl:]]','[[:digit:]]','[[:graph:]]','[[:lower:]]','[[:print:]]','[[:punct:]]','[[:space:]]','[[:upper:]]','[[:word:]]','[[:xdigit:]]','{a,b}','{a,{b,c}}','{1..3}','{1..10}','{10..12}','{c..a}','{1..10..2}','{01..03}','{é..ê}','@(a|b)','!(a|b)','+(a|b)','?(a|b)','*(a|b)','a*','a?','a.b','a+b','(a|b)','\\*','[','[]','[a','{','{a,b','{a,{b,c}','!a','!!a','.*','*.*','😀','??x','a\\b'];
const patterns=[...segments,...segments.flatMap(s=>['crates/'+s,'crates/**/'+s]),'**/*','**/src/**','crates/{a,b}/**/src','crates/!(a)/src','!crates/**','./crates/*','crates/**/','crates/(a|b)','crates/{a,b}/src/{c,d}'];
const paths=[' ', '\t', '_', ':', '#', 'Z', 'F', 'ff', 'a','b','c','aa','ab','1','2','3','01','02','10','11','12','😀','😀x','é','ê','*','.a','..','a.b','a+b','[a','{a,b','{a,b}','a/b',...['a','b','c','abc','abc1','a.b','a+b','😀','😀x','é','.a','[a','{a,b','a/b','a/src','a/x/src','a/src/c','a/src/d'].flatMap(s=>['crates/'+s,'crates/x/'+s])];
const globs=patterns.map(pattern=>{let matcher;try{matcher=pm(pattern,{dot:false})}catch{}return{pattern,answers:paths.map(path=>({path,expected:matcher?matcher(path):null}))}});
const packageSources=['','[package]\nname="foo-bar"',' [package] \r\n categories=["rust"]\nname = \'a\'\n','[package]\n# name="comment"\nname="real"','[workspace]\nname="wrong"\n[package]\nname="right"','[package]\n[[bin]]\nname="bin"','[package]\n[lib]\nname="no"','[package]\nxname="suffix"','[package]\nname="   "','[package]\nname="é😀"','[package]\nname="bad\nnext"','[package]\n[]\nname="after-empty"'];
const packages=packageSources.map(source=>({source,expected:cargo.parsePackageName(source)}));
const maps=[];
const standardDirs=['crates','crates/a','crates/b','crates/c','crates/.hidden','crates/build','crates/a/nested','deep','deep/one','deep/one/two','deep/one/two/three','deep/one/two/three/four','deep/one/two/three/four/five','deep/one/two/three/four/five/six','deep/one/two/three/four/five/six/seven','é','é/😀','é/ab'];
function captureMap(id,source,directories=standardDirs,overrides={}){
 const manifests={'Cargo.toml':source,...Object.fromEntries(directories.map((dir,i)=>[dir+'/Cargo.toml','[package]\nname="crate-'+i+'"\n'])),...overrides};
 const children=new Map();for(const dir of directories){const at=dir.lastIndexOf('/'),parent=at<0?'':dir.slice(0,at),child=dir.slice(at+1);if(!children.has(parent))children.set(parent,[]);children.get(parent).push(child)}for(const list of children.values())list.sort();
 const reads=[],lists=[];const context={readFile(path){reads.push(path);return manifests[posix.normalize(path)]??null},listDirectories(path){lists.push(path);return children.get(posix.normalize(path)==='.'?'':posix.normalize(path))??[]}};
 const expected=Object.fromEntries(cargo.getCargoWorkspaceCrateMap(context));maps.push({id,manifests,directories,expected,reads,lists});
}
for(const [id,members]of [['literal',['crates/a','crates/b']],['glob',['crates/*']],['recursive',['**']],['braces',['crates/{a,c}']],['class',['crates/[ab]']],['negative-class',['crates/[!b]']],['extglob',['crates/!(b)']],['depth',['deep/**']],['unicode',['é/??']],['outside',['../outside','/tmp/outside','a/../../outside']],['normalized',['./crates/a','crates/a/../b','crates/c/']],['root',['.']],['explicit-hidden',['crates/.hidden','crates/build']],['duplicate',['crates/*','crates/a']],['deep-literal',['deep/one/two/three/four/five/six/seven']]])captureMap(id,'[workspace]\nmembers = '+JSON.stringify(members)+'\n');
captureMap('collision','[workspace]\nmembers=["crates/a","crates/b"]',standardDirs,{'crates/a/Cargo.toml':'[package]\nname="same-name"','crates/b/Cargo.toml':'[package]\nname="same_name"'});
for(const source of ['[workspace]\nmembers=["crates/a", # "crates/b"\n "crates/c"]','[workspace]\nmembers=[["crates/a"],"crates/b"]','[workspace]\nmembers="crates/a"','[workspace]\nmembers=["crates/a"','[workspace]\nxmembers=["crates/a"]','[workspace]\nmembers=["crates/a"]\n[other]\nmembers=["crates/b"]'])captureMap('syntax-'+maps.length,source);
const Project=Object.values(require('./dist/index.js')).find(v=>typeof v==='function'&&typeof v.init==='function'&&typeof v.open==='function');
const projects=[];
const core='pub fn build() -> usize { 1 }\n',main='use core_api::build;\npub fn run() -> usize { build() }\n';
const inputs=[
 {id:'workspace-inline-module',files:{'Cargo.toml':'[workspace]\nmembers=["crates/core"]','crates/core/Cargo.toml':'[package]\nname="core-api"','crates/core/src/lib.rs':'pub mod api { pub fn build() {} }\n','src/main.rs':'pub fn run(){ core_api(); }\n'}},
 {id:'workspace-literal',files:{'Cargo.toml':'[workspace]\nmembers=["crates/core"]','crates/core/Cargo.toml':'[package]\nname="core-api"','crates/core/src/lib.rs':core,'src/main.rs':main}},
 {id:'workspace-main',files:{'Cargo.toml':'[workspace]\nmembers=["crates/core"]','crates/core/Cargo.toml':'[package]\nname="core-api"','crates/core/src/main.rs':core,'src/main.rs':main}},
 {id:'workspace-glob',files:{'Cargo.toml':'[workspace]\nmembers=["crates/{core,other}"]','crates/core/Cargo.toml':'[package]\nname="core-api"','crates/core/src/lib.rs':core,'src/main.rs':main}},
 {id:'workspace-deep',files:{'Cargo.toml':'[workspace]\nmembers=["a/b/c/d/e/f/g/core"]','a/b/c/d/e/f/g/core/Cargo.toml':'[package]\nname="core-api"','a/b/c/d/e/f/g/core/src/lib.rs':core,'src/main.rs':main}},
 {id:'workspace-normalized',files:{'Cargo.toml':'[workspace]\nmembers=["./crates/core"]','crates/core/Cargo.toml':'[package]\nname="core-api"','crates/core/src/lib.rs':core,'src/main.rs':main}},
 {id:'local-bare',files:{'Cargo.toml':'[package]\nname="app"','src/main.rs':'pub fn run(){helper();}\n','src/helper.rs':core}},
 {id:'local-module',files:{'Cargo.toml':'[package]\nname="app"','src/main.rs':'mod helper;\npub fn run(){helper::build();}\n','src/helper.rs':core}},
 {id:'local-declaration',files:{'Cargo.toml':'[package]\nname="app"','src/main.rs':'mod helper;\npub fn helper(){}\npub fn run(){helper();}\n','src/helper.rs':core}},
 {id:'local-before-workspace',files:{'Cargo.toml':'[workspace]\nmembers=["crates/core"]','crates/core/Cargo.toml':'[package]\nname="helper"','crates/core/src/lib.rs':core,'src/main.rs':'mod helper;\npub fn run(){helper::build();}\n','src/helper.rs':core}},
];
for(const input of inputs){const directory=mkdtempSync('/tmp/prometeu-cargo-');let project;try{for(const [file,source]of Object.entries(input.files)){mkdirSync(dirname(join(directory,file)),{recursive:true});writeFileSync(join(directory,file),source)}project=await Project.init(directory,{silent:true});const indexed=await project.indexAll();if(!indexed.success)throw Error('Index failed');const nodes=project.queries.getAllNodes(),ctx=project.resolver.context,allEdges=nodes.flatMap(n=>project.queries.getOutgoingEdges(n.id)),byId=new Map(nodes.map(n=>[n.id,n]));const refs=allEdges.filter(e=>e.metadata?.refName).map(e=>({fromNodeId:e.source,referenceName:e.metadata.refName,referenceKind:e.metadata.refKind??e.kind,line:e.line??byId.get(e.source).startLine,column:e.column??0,filePath:byId.get(e.source).filePath,language:byId.get(e.source).language}));refs.push(...ctx.getAllFiles().flatMap(file=>project.queries.getUnresolvedReferencesInFile(file,100000)));const answers=refs.map(reference=>({reference,expected:rust.resolve(reference,ctx)}));projects.push(clean({...input,nodes,allEdges,answers}));}finally{project?.close();rmSync(directory,{recursive:true,force:true})}console.log(input.id)}
writeFileSync(new URL('./cargo-workspace.json',import.meta.url),JSON.stringify({revision,picomatchVersion:require('picomatch/package.json').version,globs,packages,maps,projects},null,2)+'\n');console.log({globs:globs.length,packages:packages.length,maps:maps.length,projects:projects.length});
