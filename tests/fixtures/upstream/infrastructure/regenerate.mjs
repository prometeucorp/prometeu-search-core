// Independent infrastructure lookup records from committed reference projects.
import {execFileSync} from 'node:child_process';
import {createRequire} from 'node:module';
import {mkdirSync,mkdtempSync,rmSync,readFileSync,writeFileSync} from 'node:fs';
import {dirname,join,resolve} from 'node:path';
import {tmpdir} from 'node:os';
import {fileURLToPath} from 'node:url';
const root=resolve(process.argv[2]),revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('Wrong reference revision');
const require=createRequire(join(root,'package.json'));
const Project=Object.values(require('./dist/index.js')).find(value=>typeof value==='function'&&typeof value.init==='function'&&typeof value.open==='function');
const {ReferenceResolver}=require('./dist/resolution/index.js');
const {terraformResolver}=require('./dist/resolution/frameworks/terraform.js');
const {cicsResolver}=require('./dist/resolution/frameworks/cics.js');
const base={
 'main.tf':'variable "region" {}\nprovider "aws" {\n alias = "east"\n}\nmodule "child" {\n source = "./modules/child"\n region = var.region\n}\noutput "network" {\n value = module.child.network\n}\n',
 'vars.tf':'variable "root_only" {}\nlocals { region = var.region }\n',
 'envs/prod.tfvars':'region = "us-east-1"\nroot_only = "value"\n',
 'modules/child/main.tf':'variable "region" {}\nvariable "size" {}\nresource "aws_vpc" "main" {\n provider = aws.east\n cidr_block = var.region\n}\noutput "network" {\n value = aws_vpc.main.id\n}\n',
 'modules/child/other.tofu':'locals { label = var.region }\n',
 'modules/sibling/main.tf':'variable "region" {}\noutput "network" { value = var.region }\n',
 'modules/orphan/main.tf':'output "bad" { value = var.root_only }\n',
 'remote.tf':'module "remote" {\n source = "cloudposse/stack-config/yaml//modules/remote-state"\n component = "vpc"\n}\noutput "vpc" {\n value = module.remote.outputs.vpc_id\n}\n',
 'components/vpc/outputs.tf':'output "vpc_id" { value = "value" }\n',
};
const projects=[{name:'local-and-remote',files:base},{name:'ambiguous-remote',files:{...base,'other/vpc/outputs.tf':base['components/vpc/outputs.tf']}},{name:'remote-variable',files:{...base,'remote.tf':'variable "component_name" {\n default = "vpc"\n}\nmodule "remote" {\n source = "cloudposse/stack-config/yaml//modules/remote-state"\n component = var.component_name\n}\noutput "vpc" { value = module.remote.outputs.vpc_id }\n'}},{name:'remote-dynamic',files:{...base,'remote.tf':base['remote.tf'].replace('component = "vpc"','component = local.component')}},{name:'registry-boundary',files:{...base,'main.tf':base['main.tf'].replace('./modules/child','vendor/child/aws')}},{name:'parent-source',files:{...base,'nested/main.tf':'module "parent" {\n source = "../modules/child"\n region = "value"\n}\noutput "network" { value = module.parent.network }\n'}},{name:'tofu-entry',files:{'main.tofu':'module "child" {\n source = "./child"\n}\n','child/aa.tofu':'output "result" { value = "a" }','child/z.tofu':'output "other" { value = "b" }'}}];
const cobol=(name,transaction,target)=>`       >>SOURCE FORMAT FREE\nidentification division.\nprogram-id. ${name}.\ndata division.\nworking-storage section.\n01 WS-TRANID PIC X(4) VALUE '${transaction}'.\nprocedure division.\nMAIN.\n    EXEC CICS RETURN TRANSID('${target}') END-EXEC.\n    GOBACK.\n`;
projects.push({name:'cics-transactions',files:{'caller.cbl':cobol('CALLER','CA00','CB00'),'handler.cbl':cobol('HANDLER','CB00','CA00'),'other.cbl':cobol('OTHER','CC00','ZZ99')}});
projects.push({name:'cics-collision',files:{'a.cbl':cobol('FIRST','CB00','CC00'),'b.cbl':cobol('SECOND','CB00','CC00'),'c.cbl':cobol('CALLER','CC00','CB00')}});
for(const project of projects){const directory=mkdtempSync(join(tmpdir(),'prometeu-infra-'));let indexed;try{
 for(const [path,source]of Object.entries(project.files)){mkdirSync(dirname(join(directory,path)),{recursive:true});writeFileSync(join(directory,path),source);}
 indexed=await Project.init(directory,{silent:true});await indexed.indexAll();const resolver=new ReferenceResolver(directory,indexed.queries);resolver.initialize();project.queries=[];
 for(const path of resolver.context.getAllFiles()){const origin=resolver.context.getNodesInFile(path).find(node=>node.kind==='file');if(!origin)continue;const framework=origin.language==='cobol'?cicsResolver:terraformResolver;const names=origin.language==='cobol'?['cics-transid:CA00','cics-transid:CB00','cics-transid:cb00','cics-transid:CC00','cics-transid:ZZ99','other']:['var.region','var.root_only','local.region','provider.aws.east','aws_vpc.main','module.child','module.child:file','module.child:var.region','module.child:output.network','module.child:output.missing','module.remote:remote-output.vpc_id','module.remote:output.vpc_id','module.parent:file','module.parent:var.region','module.parent:output.network','unknown'];
 for(const name of names){const reference={fromNodeId:origin.id,referenceName:name,referenceKind:name.endsWith(':file')?'imports':'references',line:1,column:0,filePath:path,language:origin.language};const result=framework.resolve(reference,resolver.context);project.queries.push({reference,expected:result?{id:result.targetNodeId,confidence:result.confidence}:null});}}
 project.edges=indexed.queries.getAllNodes().flatMap(node=>indexed.queries.getOutgoingEdges(node.id)).filter(edge=>edge.metadata?.resolvedBy==='framework'&&['terraform','cics'].includes(edge.metadata.framework));console.log(project.name,project.queries.length,project.edges.length);
}finally{indexed?.close();rmSync(directory,{recursive:true,force:true});}}
const destination=dirname(fileURLToPath(import.meta.url));writeFileSync(join(destination,'cases.json'),JSON.stringify({revision,projects},null,2)+'\n');writeFileSync(join(destination,'LICENSE'),readFileSync(join(root,'LICENSE')));
