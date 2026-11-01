// Development-only module-resolution oracle from unmodified pinned resolver functions.
import {execFileSync} from 'node:child_process';
import {createRequire,Module} from 'node:module';
import {readFileSync,writeFileSync} from 'node:fs';
import {dirname,join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
const root=resolve(process.argv[2]),revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('Wrong reference revision');
const require=createRequire(join(root,'package.json')),ts=require('typescript');
const compiled=join(root,'dist/resolution/import-resolver.js'),module=new Module(compiled);
module.filename=compiled;module.paths=Module._nodeModulePaths(dirname(compiled));
module._compile(readFileSync(compiled,'utf8')+'\nmodule.exports.resolvePythonModuleForTest = resolvePythonModule;\n',compiled);
const {resolvePythonModuleForTest:resolveModule,extractImportMappings}=module.exports;
const {initGrammars,loadGrammarsForLanguages}=require('./dist/extraction/grammars.js');
const {TreeSitterExtractor}=require('./dist/extraction/tree-sitter.js');await initGrammars();await loadGrammarsForLanguages(['python']);
const projects=[];
const filename='python-module-resolution.test.ts';const tree=ts.createSourceFile(filename,readFileSync(join(root,'__tests__',filename),'utf8'),ts.ScriptTarget.Latest,true);
function literal(node){if(!node)return undefined;if(ts.isStringLiteral(node)||ts.isNoSubstitutionTemplateLiteral(node))return node.text;if(ts.isBinaryExpression(node)&&node.operatorToken.kind===ts.SyntaxKind.PlusToken){const a=literal(node.left),b=literal(node.right);if(typeof a==='string'&&typeof b==='string')return a+b;}if(ts.isObjectLiteralExpression(node)){const out={};for(const field of node.properties){if(!ts.isPropertyAssignment(field))return;const key=literal(field.name)??(ts.isIdentifier(field.name)?field.name.text:undefined),value=literal(field.initializer);if(key===undefined||value===undefined)return;out[key]=value;}return out;}}
function visit(node){if(ts.isCallExpression(node)&&node.expression.getText(tree)==='write'){const files=literal(node.arguments[0]);if(files)projects.push({name:`reference-${projects.length}`,files});}ts.forEachChild(node,visit);}visit(tree);
for(const [name,manifest]of [['setuptools-table','[tool.setuptools.package-dir]\n""="lib"\n'],['setuptools-inline','[tool.setuptools]\npackage-dir={""="lib"}\n'],['poetry','[tool.poetry]\npackages = [{include="pkg", from="lib"}]\n'],['hatch','[tool.hatch.build.targets.wheel]\npackages = ["lib/pkg"]\n']])projects.push({name,files:{'pyproject.toml':manifest,'lib/pkg/mod.py':'def go(): pass\n','main.py':'from pkg.mod import go\n'}});
projects.push({name:'setup-config',files:{'setup.cfg':'[options]\npackage_dir =\n    = lib\n','lib/pkg/mod.py':'def go(): pass\n','main.py':'from pkg.mod import go\n'}});
for(const project of projects){const files=project.files,paths=Object.keys(files).filter(path=>path.endsWith('.py')),mappings={},nodes=[];
 for(const path of paths){mappings[path]=extractImportMappings(path,files[path],'python');const extraction=new TreeSitterExtractor(path,files[path],'python').extract();for(const node of extraction.nodes){node.updatedAt=0;nodes.push(node);}}
 const context={getAllFiles:()=>paths,fileExists:path=>Object.hasOwn(files,path),readFile:path=>files[path]??null,getImportMappings:path=>mappings[path]??[],getNodesInFile:path=>nodes.filter(node=>node.filePath===path)};
 const queries=[];for(const from of paths){const specs=new Set(['','../escape','json','html','.', '..','...']);for(const import_ of mappings[from]){specs.add(import_.source);if(!import_.isNamespace&&import_.exportedName!=='*')specs.add(import_.source+'.'+import_.exportedName);}for(const path of paths)specs.add(path.replace(/(?:^|\/)__init__\.py$/,'').replace(/\.py$/,'').replaceAll('/','.'));for(const specifier of specs)queries.push({from,specifier,expected:resolveModule(specifier,from,context)});}
 project.paths=paths;project.nodes=nodes;project.imports=Object.fromEntries(Object.entries(mappings).map(([path,entries])=>[path,entries.map(entry=>({local:entry.localName,imported:entry.isNamespace?'*':entry.exportedName,module:entry.source,scope:'',reexport:false}))]));project.queries=queries;
 console.log(project.name,queries.length);
}
const destination=dirname(fileURLToPath(import.meta.url));writeFileSync(join(destination,'cases.json'),JSON.stringify({revision,projects},null,2)+'\n');writeFileSync(join(destination,'LICENSE'),readFileSync(join(root,'LICENSE')));
