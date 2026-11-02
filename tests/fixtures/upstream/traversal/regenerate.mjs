// Development-only: records the pinned reference's actual Git/filesystem traversal.
import {createRequire} from 'node:module';
import {execFileSync} from 'node:child_process';
import {mkdtempSync,mkdirSync,writeFileSync,readFileSync,readdirSync,lstatSync,rmSync,existsSync} from 'node:fs';
import {join,resolve,dirname,relative} from 'node:path';
import {tmpdir} from 'node:os';
const root=resolve(process.argv[2]);
const revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('Wrong reference revision');
const require=createRequire(join(root,'package.json'));
const {scanDirectory,buildDefaultIgnore}=require('./dist/extraction/index.js');
const config=require('./dist/project-config.js');
const temp=mkdtempSync(join(tmpdir(),'prometeu-traversal-oracle-'));
const globalConfig=join(temp,'global-config');writeFileSync(globalConfig,'');
process.env.GIT_CONFIG_GLOBAL=globalConfig;process.env.GIT_CONFIG_NOSYSTEM='1';
const cases=[];
function capture(id,setup,{project='.',git=true,objects=false,environment={}}={}) {
 const directory=join(temp,id);mkdirSync(directory,{recursive:true});
 const command=(args,cwd=directory)=>execFileSync('git',['-c','user.name=Oracle','-c','user.email=oracle@example.invalid',...args],{cwd,encoding:'utf8',stdio:['ignore','pipe','pipe']});
 const write=(file,source='export function symbol() {}\n')=>{const path=join(directory,file);mkdirSync(dirname(path),{recursive:true});writeFileSync(path,source);};
 const initialize=(path='.')=>{mkdirSync(join(directory,path),{recursive:true});command(['init','--quiet'],join(directory,path));command(['config','core.ignorecase','false'],join(directory,path));};
 const configure=settings=>write(join(project,config.PROJECT_CONFIG_FILENAME),JSON.stringify(settings));
 if(git)initialize();
 setup({directory,write,command,initialize,configure});
 config.clearProjectConfigCache();
 const saved=new Map(Object.keys(environment).map(key=>[key,process.env[key]]));
 for(const[key,value]of Object.entries(environment))process.env[key]=value.replaceAll('<WORKSPACE>',directory);
 let expected;
 try{expected=scanDirectory(join(directory,project)).sort();}finally{for(const[key,value]of saved)value===undefined?delete process.env[key]:process.env[key]=value;}
 const files={};
 function collect(path){for(const entry of readdirSync(path,{withFileTypes:true})){
  const absolute=join(path,entry.name),file=relative(directory,absolute).replaceAll('\\','/');
  if(entry.isDirectory()){
   if(file.split('/').some(part=>(objects?['logs','hooks','refs']:['objects','logs','hooks','refs']).includes(part))&&file.includes('.git/'))continue;
   collect(absolute);
  }else if(entry.isFile()){
   const bytes=readFileSync(absolute),text=bytes.toString('utf8');
   const stored=file.endsWith(config.PROJECT_CONFIG_FILENAME)?file.slice(0,-config.PROJECT_CONFIG_FILENAME.length)+'prometeu-search.json':file;
   files[stored]=file.endsWith('/index')||/\/sharedindex\.[0-9a-f]+$/.test(file)||file.includes('/objects/')?{hex:bytes.toString('hex')}:{text:text.replaceAll(directory,'<WORKSPACE>')};
  }
 }}
 collect(directory);cases.push({id,project,files,expected,...(Object.keys(environment).length?{environment}:{})});
}
try{
 capture('tracked-root-and-nested-ignore',({write,command})=>{for(const path of ['root.ts','nested/tracked.ts','nested/deep/tracked.ts'])write(path);command(['add','.']);write('.gitignore','root.ts\n');write('nested/.gitignore','*.ts\ndeep/\n');write('nested/untracked.ts');write('nested/deep/untracked.ts');write('ordinary.ts');});
 capture('nested-negation',({write,command})=>{write('nested/tracked.ts');command(['add','.']);write('nested/.gitignore','*.ts\n!allowed.ts\n');write('nested/allowed.ts');write('nested/ignored.ts');});
 capture('defaults-and-opt-in',({write,command})=>{for(const path of ['vendor/dependency.ts','target/dependency.rs','node_modules/dependency.ts','src/app.ts'])write(path);command(['add','-f','.']);write('.gitignore','!vendor/\n');});
 capture('default-catalog',({write})=>{for(const dir of ['node_modules','bower_components','jspm_packages','web_modules','.yarn','.pnpm-store','.next','.nuxt','.svelte-kit','.turbo','.vite','.parcel-cache','.angular','.docusaurus','storybook-static','.vinxi','.nitro','out-tsc','.vercel','.netlify','.wrangler','dist','build','out','.output','coverage','.nyc_output','__pycache__','__pypackages__','.venv','venv','.pixi','.pdm-build','.mypy_cache','.pytest_cache','.ruff_cache','.tox','.nox','.hypothesis','.ipynb_checkpoints','.eggs','target','.gradle','obj','vendor','.build','Pods','Carthage','DerivedData','.swiftpm','.dart_tool','.pub-cache','.cxx','.externalNativeBuild','vcpkg_installed','.bloop','.metals','lua_modules','.luarocks','__history','__recovery','.cache','project.egg-info','cmake-build-debug','bazel-output','src','lib','packages','bin','deps','env','tmp','storage','Library'])write(`${dir}/source.ts`);},{git:false});
 capture('jvm-and-source-build',({write})=>{for(const path of ['src/main/java/org/build/Worker.java','app/src/test/kotlin/build/Example.kt','src/build/Feature.ts','src/build/output.js','src/build/types.d.ts','src/go/build/project.go','src/Build/Project.cs','src/build/unit.rs','src/build/module.vue','nested/src/web/build/asset.py','app/res/values/strings.xml','app/res/layout-v21/view.xml','app/res/raw/code.js','res/drawable-hdpi/icon.xml'])write(path);},{git:false});
 capture('source-build-opt-in',({write})=>{write('.gitignore','!src/build/\n');write('src/build/output.js');write('src/build/types.d.ts');write('src/build/vendor/pkg.ts');},{git:false});
 capture('include-exclude-and-extensions',({write,configure})=>{write('.gitignore','Tools/\n');write('Tools/main.custom');write('Tools/omit.ts');write('node_modules/dependency.ts');configure({extensions:{custom:'typescript'},include:['Tools/','node_modules/'],exclude:['Tools/omit.ts']});});
 capture('embedded-untracked',({write,initialize,command})=>{initialize('repos/child');write('repos/child/used.ts');write('repos/child/.gitignore','ignored.ts\n');write('repos/child/ignored.ts');write('root.ts');});
 capture('embedded-ignored-default',({write,initialize})=>{initialize('repos/child');write('repos/child/used.ts');write('.gitignore','repos/\n');});
 capture('embedded-ignored-opt-in',({write,initialize,configure})=>{initialize('repos/child');write('repos/child/used.ts');write('repos/plain.ts');write('.gitignore','repos/\n');configure({includeIgnored:['repos/']});});
 capture('embedded-specific-opt-in',({write,initialize,configure})=>{for(const name of ['chosen','other']){initialize(`repos/${name}`);write(`repos/${name}/used.ts`);}write('.gitignore','repos/\n');configure({includeIgnored:['repos/chosen/']});});
 capture('nested-embedded-opt-in',({write,initialize,configure})=>{initialize('repos/child');initialize('repos/child/hidden/grandchild');write('repos/child/used.ts');write('repos/child/.gitignore','hidden/\n');write('repos/child/hidden/grandchild/used.ts');write('.gitignore','repos/\n');configure({includeIgnored:['repos/']});});
 capture('tracked-gitlink',({write,initialize,command})=>{initialize('child');write('child/used.ts');command(['add','.'],join(temp,'tracked-gitlink','child'));command(['commit','--quiet','-m','fixture'],join(temp,'tracked-gitlink','child'));command(['add','child']);write('root.ts');});
 capture('ignored-tracked-gitlink',({write,initialize,command})=>{initialize('child');write('child/used.ts');const child=join(temp,'ignored-tracked-gitlink','child');command(['add','.'],child);command(['commit','--quiet','-m','fixture'],child);command(['add','child']);write('.gitignore','child/\n');});
 capture('nested-worktree',({write,command,configure})=>{write('base.ts');command(['add','.']);command(['commit','--quiet','-m','fixture']);command(['worktree','add','--quiet','copy','-b','copy']);configure({includeIgnored:['copy/'],include:['copy/']});});
 capture('index-version-four',({write,command})=>{for(const path of ['nested/alpha.ts','nested/alphabet.ts','nested/beta.ts'])write(path);command(['add','.']);command(['update-index','--index-version','4']);write('nested/.gitignore','*.ts\n');});
 capture('split-index',({write,command})=>{write('nested/first.ts');write('nested/second.ts');command(['add','.']);command(['update-index','--split-index']);write('nested/first.ts','export function changed() {}\n');command(['add','nested/first.ts']);command(['rm','--cached','nested/second.ts']);write('nested/.gitignore','*.ts\n');});
 capture('skip-worktree-present-and-absent',({write,command,directory})=>{write('nested/present.ts');write('nested/absent.ts');command(['add','.']);command(['update-index','--skip-worktree','nested/present.ts','nested/absent.ts']);rmSync(join(directory,'nested/absent.ts'));write('nested/.gitignore','*.ts\n');});
 capture('project-inside-repository',({write,command})=>{write('app/nested/tracked.ts');write('outside.ts');command(['add','.']);write('app/nested/.gitignore','*.ts\n');write('app/nested/untracked.ts');},{project:'app'});
 capture('project-ignored-by-parent',({write})=>{write('.gitignore','app/\n');write('app/local.ts');write('outside.ts');},{project:'app'});
 capture('nested-worktree-default',({write,command,configure})=>{write('base.ts');command(['add','.']);command(['commit','--quiet','-m','fixture']);command(['worktree','add','--quiet','copy','-b','copy']);configure({includeIgnored:['copy/']});});
 capture('filesystem-include-ignored-no-revival',({write,initialize,configure})=>{initialize('repos/child');write('repos/child/used.ts');write('.gitignore','repos/\n');configure({includeIgnored:['repos/']});},{git:false});
 capture('repository-exclude-files',({write,command,directory})=>{for(const path of ['info.ts','local.ts','kept.ts'])write(path);command(['add','.']);write('.git/info/exclude','info.ts\n');write('rules.ignore','local.ts\n');command(['config','core.excludesFile',join(directory,'rules.ignore')]);});
 capture('index-sha256',({write,command,directory})=>{rmSync(join(directory,'.git'),{recursive:true});command(['init','--quiet','--object-format=sha256']);write('nested/tracked.ts');command(['add','.']);write('nested/.gitignore','*.ts\n');write('nested/untracked.ts');});
 for(const packed of [false,true])capture(packed?'sparse-index-packed':'sparse-index-loose',({write,command,directory})=>{
   write('visible/main.ts');write('hidden/tracked.ts');write('hidden/nested/tracked.ts');write('hidden/.gitignore','*.ts\n');command(['add','-f','.']);command(['commit','--quiet','-m','fixture']);
   command(['sparse-checkout','init','--cone','--sparse-index']);command(['sparse-checkout','set','visible']);
   write('hidden/.gitignore','*.ts\n');write('hidden/tracked.ts');write('hidden/untracked.ts');write('hidden/nested/tracked.ts');write('hidden/nested/untracked.ts');
   if(packed)command(['gc','--quiet','--prune=now']);
 },{objects:true});
 capture('git-config-include',({write,command,directory})=>{write('kept.ts');write('hidden.ts');command(['add','.']);write('.git/extra','[core]\n excludesFile = '+JSON.stringify(join(directory,'included.ignore'))+'\n');write('included.ignore','hidden.ts\n');command(['config','include.path','extra']);});
 capture('git-config-conditional-directory',({write,command,directory})=>{write('kept.ts');write('hidden.ts');command(['add','.']);write('.git/extra','[core]\n excludesFile = '+JSON.stringify(join(directory,'included.ignore'))+'\n');write('included.ignore','hidden.ts\n');command(['config','includeIf.gitdir:**/.git.path','extra']);});
 capture('git-config-conditional-branch',({write,command,directory})=>{write('kept.ts');write('hidden.ts');command(['add','.']);write('.git/extra','[core]\n excludesFile = '+JSON.stringify(join(directory,'included.ignore'))+'\n');write('included.ignore','hidden.ts\n');const branch=command(['symbolic-ref','--short','HEAD']).trim();command(['config',`includeIf.onbranch:${branch}.path`,'extra']);});
 capture('git-config-conditional-remote',({write,command,directory})=>{write('kept.ts');write('hidden.ts');command(['add','.']);write('.git/extra','[core]\n excludesFile = '+JSON.stringify(join(directory,'included.ignore'))+'\n');write('included.ignore','hidden.ts\n');command(['config','includeIf.hasconfig:remote.*.url:https://example.invalid/**.path','extra']);command(['config','remote.origin.url','https://example.invalid/project']);});
 capture('git-config-quoted-path',({write,command,directory})=>{write('kept.ts');write('hidden.ts');command(['add','.']);write('rules with "quotes".ignore','hidden.ts\n');command(['config','core.excludesFile',join(directory,'rules with "quotes".ignore')]);});
 capture('unicode-index-paths',({write,command})=>{write('例/café.ts');write('例/space name.ts');command(['add','.']);write('例/.gitignore','*.ts\n');write('例/untracked.ts');});
 const environmentProject=({write})=>{write('visible.ts');write('hidden.ts');write('rules.ignore','hidden.ts\n');write('other.ignore','visible.ts\n');};
 capture('environment-count-excludes',environmentProject,{environment:{GIT_CONFIG_COUNT:'1',GIT_CONFIG_KEY_0:'core.excludesFile',GIT_CONFIG_VALUE_0:'<WORKSPACE>/rules.ignore'}});
 capture('environment-count-precedence',environmentProject,{environment:{GIT_CONFIG_COUNT:'2',GIT_CONFIG_KEY_0:'core.excludesFile',GIT_CONFIG_VALUE_0:'<WORKSPACE>/other.ignore',GIT_CONFIG_KEY_1:'core.excludesFile',GIT_CONFIG_VALUE_1:'<WORKSPACE>/rules.ignore'}});
 capture('environment-parameters-legacy',environmentProject,{environment:{GIT_CONFIG_PARAMETERS:"'core.excludesFile=<WORKSPACE>/rules.ignore'"}});
 capture('environment-parameters-pairs',environmentProject,{environment:{GIT_CONFIG_PARAMETERS:"'core.excludesFile'='<WORKSPACE>/rules.ignore'"}});
 capture('environment-parameters-quotes',({write})=>{write('visible.ts');write('hidden.ts');write("rules with 'quotes'.ignore",'hidden.ts\n');},{environment:{GIT_CONFIG_PARAMETERS:"'core.excludesFile'='<WORKSPACE>/rules with '\\''quotes'\\''.ignore'"}});
 capture('environment-include',({write,directory})=>{environmentProject({write});write('extra.config','[core]\n excludesFile = '+JSON.stringify(join(directory,'rules.ignore'))+'\n');},{environment:{GIT_CONFIG_COUNT:'1',GIT_CONFIG_KEY_0:'include.path',GIT_CONFIG_VALUE_0:'<WORKSPACE>/extra.config'}});
 capture('environment-conditional-remote',({write,command,directory})=>{environmentProject({write});write('.git/extra','[core]\n excludesFile = '+JSON.stringify(join(directory,'rules.ignore'))+'\n');command(['config','includeIf.hasconfig:remote.*.url:https://example.invalid/**.path','extra']);},{environment:{GIT_CONFIG_COUNT:'1',GIT_CONFIG_KEY_0:'remote.origin.url',GIT_CONFIG_VALUE_0:'https://example.invalid/project'}});
 writeFileSync(new URL('./cases.json',import.meta.url),JSON.stringify({revision,cases},null,2)+'\n');
}finally{rmSync(temp,{recursive:true});}
