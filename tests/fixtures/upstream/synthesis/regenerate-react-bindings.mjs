// Independent framework answers and persisted graph records from the pinned runtime.
import {createRequire} from 'node:module';
import {execFileSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdirSync,mkdtempSync,rmSync} from 'node:fs';
import {join,dirname,resolve} from 'node:path';
import {tmpdir} from 'node:os';
const root=resolve(process.argv[2]),revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('Wrong reference revision');
const require=createRequire(join(root,'package.json'));
const Project=Object.values(require('./dist/index.js')).find(value=>typeof value==='function'&&typeof value.init==='function'&&typeof value.open==='function');
const {reactResolver}=require('./dist/resolution/frameworks/react.js'),modules=require('./dist/resolution/import-resolver.js'),resolvePath=modules.resolveImportPath;
const corpus=JSON.parse(readFileSync(new URL('./cases.json',import.meta.url),'utf8'));
const inputs=corpus.cases.filter(c=>/^jsx-|^react-tables-/.test(c.id));
const cases=[];
for(const input of inputs){const temporary=mkdtempSync(join(tmpdir(),'prometeu-react-bindings-'));let project;try{
 const written={...input.assets,...input.files};for(const [path,source]of Object.entries(written)){mkdirSync(dirname(join(temporary,path)),{recursive:true});writeFileSync(join(temporary,path),source);}
 project=await Project.init(temporary,{silent:true});const result=await project.indexAll();if(!result.success)throw Error('Index failed');
 const queries=project.queries,ctx=project.resolver.context,nodes=queries.getAllNodes(),files={},assets={},imports=[],moduleAnswers=[],importAnswers=[];
 for(const [path,source]of Object.entries(written))(ctx.getAllFiles().includes(path)?files:assets)[path]=source;
 for(const path of Object.keys(files)){const language=nodes.find(n=>n.filePath===path&&n.kind==='file')?.language??'typescript';for(const binding of ctx.getImportMappings(path,language)){imports.push({file:path,local_name:binding.localName,source:binding.source,imported_name:binding.exportedName??'',is_default:binding.isDefault??false,is_namespace:binding.isNamespace??false,resolved_path:resolvePath(binding.source,path,language,ctx),mapped_path:binding.resolvedPath??null,external:false});}}
 modules.resolveImportPath=(specifier,from,language,context)=>{const answer=resolvePath(specifier,from,language,context);moduleAnswers.push({specifier,from,language,answer});return answer;};
 const originalImport=ctx.resolveImport;ctx.resolveImport=(ref)=>{const result=originalImport?.call(ctx,ref)??null;importAnswers.push({reference:ref,answer:result?.targetNodeId??null});return result;};
 // Resolution removes successful pending rows. Reconstruct their exact input name,
 // kind and location from its persisted metadata, alongside remaining raw rows.
 const referenceKeys=new Set(),byId=new Map(nodes.map(n=>[n.id,n]));
 const allEdges=nodes.flatMap(node=>queries.getOutgoingEdges(node.id));
 const pending=Object.keys(files).flatMap(file=>queries.getUnresolvedReferencesInFile(file,100000));
 const resolved=allEdges.filter(e=>e.metadata?.refName).map(e=>({fromNodeId:e.source,referenceName:e.metadata.refName,referenceKind:e.metadata.refKind??e.kind,line:e.line??byId.get(e.source).startLine,column:e.column??0,filePath:byId.get(e.source).filePath,language:byId.get(e.source).language}));
 const references=[...pending,...resolved].filter(reference=>{const key=JSON.stringify([reference.fromNodeId,reference.referenceName,reference.referenceKind,reference.line,reference.column]);if(referenceKeys.has(key))return false;referenceKeys.add(key);return true;});

 const expected=references.map(reference=>{const answer=reactResolver.resolve(reference,ctx);return answer?{targetNodeId:answer.targetNodeId,confidence:answer.confidence,...(answer.metadata?{metadata:answer.metadata}:{})}:null});
 ctx.resolveImport=originalImport;modules.resolveImportPath=resolvePath;
 const edges=nodes.flatMap(node=>queries.getOutgoingEdges(node.id)).filter(edge=>edge.metadata?.framework==='react');
 for(const node of nodes)delete node.updatedAt;
 cases.push({id:input.id,origin:input.origin,files,assets,nodes,imports,moduleAnswers,importAnswers,references,expected,edges});console.log(input.id,references.length,expected.filter(Boolean).length,edges.length);
}finally{modules.resolveImportPath=resolvePath;project?.close();rmSync(temporary,{recursive:true,force:true});}}
writeFileSync(new URL('./react-bindings.json',import.meta.url),JSON.stringify({revision,cases},null,2)+'\n');
