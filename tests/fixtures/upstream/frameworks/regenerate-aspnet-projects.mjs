// Development-only persistent ASP.NET graph oracle using the actual project API.
import {createRequire} from 'node:module';
import {execFileSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdirSync,mkdtempSync,rmSync} from 'node:fs';
import {join,resolve,dirname} from 'node:path';
import {tmpdir} from 'node:os';
const root=resolve(process.argv[2]),revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('Wrong reference revision');
const require=createRequire(join(root,'package.json'));
const Project=Object.values(require('./dist/index.js')).find(value=>typeof value==='function'&&typeof value.init==='function'&&typeof value.open==='function');
const input=JSON.parse(readFileSync(new URL('./aspnet.json',import.meta.url),'utf8'));
const projects=[];
for(const original of input.projects.filter(project=>['original-fastendpoints','group-prefix-and-override','constant-variants'].includes(project.id))) {
 const files={...original.files,'Startup.cs':'','Web.csproj':'<Project Sdk="Microsoft.NET.Sdk.Web"></Project>'};
 const temporary=mkdtempSync(join(tmpdir(),'prometeu-aspnet-project-'));let project;
 try {
  for(const [path,content]of Object.entries(files)){mkdirSync(dirname(join(temporary,path)),{recursive:true});writeFileSync(join(temporary,path),content);}
  project=await Project.init(temporary,{silent:true});const result=await project.indexAll();if(!result.success)throw Error('Index failed');
  const clean=node=>{delete node.updatedAt;return node;};
  const routes=project.queries.getNodesByKind('route').map(node=>({node:clean(node),targets:project.queries.getOutgoingEdges(node.id).map(edge=>({edge,node:clean(project.getNode(edge.target))}))}));
  projects.push({id:original.id,files,routes});
 }finally{project?.close();rmSync(temporary,{recursive:true,force:true});}
}
writeFileSync(new URL('./aspnet-projects.json',import.meta.url),JSON.stringify({revision,projects},null,2)+'\n');
console.log(projects.map(project=>[project.id,project.routes.length]));
