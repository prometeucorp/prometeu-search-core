// Development-only Python framework and competing strategy oracle.
import{execFileSync}from'node:child_process';import{createRequire}from'node:module';import{readFileSync,writeFileSync,mkdirSync,mkdtempSync,rmSync}from'node:fs';import{join,dirname,resolve}from'node:path';import{tmpdir}from'node:os';import vm from'node:vm';
const root=resolve(process.argv[2]),revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';if(execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('Wrong reference revision');
const require=createRequire(join(root,'package.json')),ts=require('typescript');const api=require('./dist/resolution/frameworks/python.js');
const {ReferenceResolver}=require('./dist/resolution/index.js');const {extractFromSource}=require('./dist/extraction/tree-sitter.js');const {initGrammars,loadAllGrammars}=require('./dist/extraction/grammars.js');await initGrammars();await loadAllGrammars();
const reader=require('./dist/resolution/import-resolver.js'),matcher=require('./dist/resolution/name-matcher.js');
const Project=Object.values(require('./dist/index.js')).find(value=>typeof value==='function'&&typeof value.init==='function'&&typeof value.open==='function');const clean=node=>{const value={...node};delete value.updatedAt;return value;};
const answer=result=>result?{target:result.targetNodeId,confidence:result.confidence,resolved_by:result.resolvedBy,framework:result.metadata?.framework??null}:null;
const resolvers={django:api.djangoResolver,flask:api.flaskResolver,fastapi:api.fastapiResolver};const projects=[];
const original=readFileSync(join(root,'__tests__/framework-name-heuristic-scope.test.ts'),'utf8');const tree=ts.createSourceFile('scope.ts',original,ts.ScriptTarget.Latest,true);
function visit(node){if(ts.isVariableDeclaration(node)&&node.name.getText(tree)==='files'&&node.initializer&&ts.isObjectLiteralExpression(node.initializer)){projects.push({name:'original-scope',files:vm.runInNewContext(ts.transpile('('+node.initializer.getText(tree)+')'))});}ts.forEachChild(node,visit);}visit(tree);
const django={'manage.py':''},fastapi={'requirements.txt':'fastapi\n'},flask={'requirements.txt':'flask\n'};
for(const[name,files]of[
 ['django-preferred',{...django,'src/caller.py':'def run():\n    return UserModel()\n','app/models/user.py':'class UserModel:\n    pass\n','lib/other.py':'class UserModel:\n    pass\n'}],
 ['django-same-file',{...django,'src/caller.py':'def UserView():\n    return 1\n\ndef run():\n    return UserView()\n','app/views/user.py':'class UserView:\n    pass\n'}],
 ['django-import',{...django,'src/caller.py':'from lib.other import UserModel\n\ndef run():\n    return UserModel()\n','app/models/user.py':'class UserModel:\n    pass\n','lib/other.py':'class UserModel:\n    pass\n'}],
 ['django-iterable',{...django,'django/db/models/query.py':'class ModelIterable:\n    def __iter__(self):\n        return 1\n\nclass QuerySet:\n    def fetch(self):\n        return self._iterable_class(self)\n'}],
 ['django-nested',{...django,'src/models.py':'class Outer:\n    class UserModel:\n        pass\n\ndef outer():\n    class LocalForm:\n        pass\n    return LocalForm()\n','src/other.py':'def run():\n    return LocalForm()\n','app/models/other.py':'class UserModel:\n    pass\n'}],
 ['fastapi-preferred',{...fastapi,'service/main.py':'def run():\n    return get_user()\n','service/dependencies/user.py':'def get_user():\n    return 1\n','lib/other.py':'def get_user():\n    return 2\n'}],
 ['fastapi-import',{...fastapi,'service/main.py':'from lib.other import get_user\n\ndef run():\n    return get_user()\n','service/dependencies/user.py':'def get_user():\n    return 1\n','lib/other.py':'def get_user():\n    return 2\n'}],
 ['fastapi-external',{...fastapi,'service/main.py':'from absent import get_user\n\ndef run():\n    return get_user()\n','service/dependencies/user.py':'def get_user():\n    return 1\n'}],
 ['fastapi-self',{...fastapi,'service/main.py':'class Service:\n    def get_user(self):\n        return 1\n    def run(self):\n        return self.get_user()\n','service/dependencies/user.py':'def get_user():\n    return 2\n'}],
 ['fastapi-self-import',{...fastapi,'service/main.py':'from service.dependencies.user import get_user\n\nclass Service:\n    def get_user(self):\n        return 1\n    def run(self):\n        return self.get_user()\n','service/dependencies/user.py':'def get_user():\n    return 2\n'}],
 ['fastapi-import-receivers',{...fastapi,'service/main.py':'from service.dependencies.user import get_user\n\nclass Base:\n    def get_user(self):\n        return 1\n\nclass Service(Base):\n    @classmethod\n    def class_call(cls):\n        return cls.get_user()\n    def super_call(self):\n        return super().get_user()\n    def multiline(self):\n        return (self.\n            get_user())\n','service/dependencies/user.py':'def get_user():\n    return 2\n'}],
 ['django-inheritance',{...django,'src/caller.py':'class Customer(UserModel):\n    pass\n','app/models/user.py':'class UserModel:\n    pass\n','lib/other.py':'class UserModel:\n    pass\n'}],
 ['competing-frameworks',{'requirements.txt':'django\nfastapi\nflask\n','src/caller.py':'def run():\n    return DependsForm()\n','app/forms/user.py':'class DependsForm:\n    pass\n','app/dependencies/user.py':'def DependsForm():\n    return 1\n'}],
 ['fastapi-inherited',{...fastapi,'service/main.py':'from service.base import Base\n\nclass Service(Base):\n    def run(self):\n        return self.get_user()\n','service/base.py':'class Base:\n    def get_user(self):\n        return 1\n','service/dependencies/user.py':'def get_user():\n    return 2\n'}],
 ['flask-blueprint',{...flask,'site/app.py':'auth_bp = object()\n\ndef run():\n    return auth_bp\n','site/auth.py':'auth_bp = object()\n','other/auth.py':'auth_bp = object()\n'}],
 ['fastapi-locals',{...fastapi,'service/main.py':'def run(get_user):\n    return get_user()\n\ndef outer():\n    router = object()\n    return router\n','service/dependencies/user.py':'def get_user():\n    return 2\n','service/routers/user.py':'router = object()\n'}],
 ['django-tests',{...django,'src/main.py':'def run():\n    return UserModel()\n','tests/models.py':'class UserModel:\n    pass\n','testing/support.py':'class UserModel:\n    pass\n'}],
 ['django-mocks',{...django,'src/main.py':'def run():\n    return UserModel()\n','__mocks__/models.py':'class UserModel:\n    pass\n','testing/support.py':'class UserModel:\n    pass\n'}],
])projects.push({name,files});
for(const project of projects){const dir=mkdtempSync(join(tmpdir(),'prometeu-python-framework-'));let indexed;try{
 for(const[path,source]of Object.entries(project.files)){mkdirSync(dirname(join(dir,path)),{recursive:true});writeFileSync(join(dir,path),source);}
 indexed=await Project.init(dir,{silent:true});await indexed.indexAll();const queries=indexed.queries,resolver=new ReferenceResolver(dir,queries);resolver.initialize();const context=resolver.context,nodes=queries.getAllNodes();project.frameworks=resolver.getDetectedFrameworks();project.nodes=nodes.map(clean);project.edges=nodes.flatMap(node=>queries.getOutgoingEdges(node.id));project.references=[];project.modules=[];
 project.supertypes=Object.fromEntries(nodes.filter(node=>node.kind==='class').map(node=>[node.id,(context.getSupertypeNodes?.(node.id)??[]).map(node=>node.id)]));
 for(const[path,source]of Object.entries(project.files)){if(!path.endsWith('.py'))continue;const extracted=extractFromSource(path,source,'python',project.frameworks);project.references.push(...extracted.unresolvedReferences.map(reference=>({...reference,filePath:path,language:'python'})));}
 // Direct refs exercise values/import/type visibility independently of parser emission.
 for(const node of nodes.filter(node=>['function','method','class','file'].includes(node.kind)&&node.language==='python'))for(const name of ['UserModel','UserView','LocalForm','router','auth_bp','get_user','_iterable_class'])project.references.push({fromNodeId:node.id,referenceName:name,referenceKind:'references',line:node.startLine,column:0,filePath:node.filePath,language:'python',synthetic:true});
 project.queries=[];
 for(const reference of project.references){if(!/^[A-Za-z_]\w*$/.test(reference.referenceName))continue;
  const pure=Object.fromEntries(Object.entries(resolvers).map(([name,resolver])=>[name,answer(resolver.resolve(reference,context))]));
  const imported=reader.resolveViaImport(reference,context),named=matcher.matchByExactName(reference,context),full=resolver.resolveOne(reference);
  project.queries.push({reference,pure,imported:answer(imported),named:answer(named),expected:answer(full)});
 }
 for(const[path,source]of Object.entries(project.files)){for(const match of source.matchAll(/^\s*from\s+([\w.]+)\s+import/gm))project.modules.push({from:path,specifier:match[1],target:reader.resolveImportPath(match[1],path,'python',context)});}
 console.log(project.name,nodes.length,project.queries.length);
}finally{indexed?.close();rmSync(dir,{recursive:true,force:true});}}
writeFileSync(new URL('./cases.json',import.meta.url),JSON.stringify({revision,projects},null,2)+'\n');
