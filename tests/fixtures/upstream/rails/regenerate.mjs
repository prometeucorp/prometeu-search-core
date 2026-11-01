// Development-only extraction and indexing oracle from the pinned reference.
import {execFileSync} from 'node:child_process';
import {createRequire} from 'node:module';
import {readFileSync,writeFileSync,mkdirSync,mkdtempSync,rmSync} from 'node:fs';
import {dirname,join,resolve} from 'node:path';
import {tmpdir} from 'node:os';
import {fileURLToPath} from 'node:url';
const root=resolve(process.argv[2]),revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';
if(execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('Wrong reference revision');
const require=createRequire(join(root,'package.json')),ts=require('typescript');
const {railsResolver:resolver}=require('./dist/resolution/frameworks/ruby.js');
const Project=Object.values(require('./dist/index.js')).find(value=>typeof value==='function'&&typeof value.init==='function'&&typeof value.open==='function');
const {ReferenceResolver}=require('./dist/resolution/index.js');
const strip=node=>{const copy={...node};delete copy.updatedAt;return copy;};
const cases=[],seen=new Set(),projects=[];
function add(path,source,origin){const key=JSON.stringify([path,source]);if(seen.has(key))return;seen.add(key);const extracted=resolver.extract(path,source);cases.push({path,source,origin,expected:{nodes:extracted.nodes.map(strip),references:extracted.references}});}
function literal(node,tree,depth=0){if(!node||depth>20)return undefined;if(ts.isStringLiteral(node)||ts.isNoSubstitutionTemplateLiteral(node))return node.text;if(ts.isNumericLiteral(node))return Number(node.text);if(node.kind===ts.SyntaxKind.TrueKeyword)return true;if(node.kind===ts.SyntaxKind.FalseKeyword)return false;if(ts.isArrayLiteralExpression(node)){const values=node.elements.map(value=>literal(value,tree,depth+1));return values.every(value=>value!==undefined)?values:undefined;}if(ts.isObjectLiteralExpression(node)){const object={};for(const property of node.properties){if(ts.isSpreadAssignment(property)){const spread=literal(property.expression,tree,depth+1);if(!spread)return undefined;Object.assign(object,spread);continue;}if(ts.isShorthandPropertyAssignment(property)){const value=literal(property.name,tree,depth+1);if(value===undefined)return undefined;object[property.name.text]=value;continue;}if(!ts.isPropertyAssignment(property))return undefined;const key=ts.isIdentifier(property.name)?property.name.text:literal(ts.isComputedPropertyName(property.name)?property.name.expression:property.name,tree,depth+1);const value=literal(property.initializer,tree,depth+1);if(key===undefined||value===undefined)return undefined;object[key]=value;}return object;}if(ts.isBinaryExpression(node)&&node.operatorToken.kind===ts.SyntaxKind.PlusToken){const left=literal(node.left,tree,depth+1),right=literal(node.right,tree,depth+1);if(typeof left==='string'&&typeof right==='string')return left+right;}if(ts.isIdentifier(node)){for(let block=node.parent;block;block=block.parent){for(const statement of block.statements??[]){if(statement.pos>node.pos)continue;if(ts.isVariableStatement(statement))for(const declaration of statement.declarationList.declarations)if(declaration.name.getText(tree)===node.text)return literal(declaration.initializer,tree,depth+1);}}}if(ts.isCallExpression(node)&&node.expression.getText(tree)==='JSON.stringify'){const value=literal(node.arguments[0],tree,depth+1);if(value!==undefined)return JSON.stringify(value);}}for(const file of ['rails-scoped-routes.test.ts','rails-only-except-forms.test.ts','frameworks.test.ts']){
 const tree=ts.createSourceFile(file,readFileSync(join(root,'__tests__',file),'utf8'),ts.ScriptTarget.Latest,true);
 function visit(node){if((ts.isNoSubstitutionTemplateLiteral(node)||ts.isStringLiteral(node))&&/\b(?:resources?\s+:|routes\.draw|get\s+['"][^'"]*['"]\s*(?:,|=>)[^\n]*#)/.test(node.text))add('config/routes.rb',node.text,file);
 if(file.startsWith('rails-')&&ts.isVariableDeclaration(node)&&node.name.getText(tree)==='files'){const files=literal(node.initializer,tree);if(!files)throw Error('Uncaptured files');projects.push({name:file.replace('.test.ts',''),files});}ts.forEachChild(node,visit);}visit(tree);
}
const flat=["get '/hello', to: 'pages#show'","match '/hello' => 'pages#match'","resources :articles","resource :session",...['only: :show','only: "show"','only: [:index, :show]','only: %i[new create index]','only: %w(index show)','only: %I[show edit]','only: %W(show edit)','except: [:show]','only: [], except: [:index]',':only => [:index, "show"]'].map(option=>`resources :books, ${option}`)];
for(const source of flat)for(const path of ['other.rb','config/routes.rb','config/routes/extra.rb'])add(path,source,'flat/resource forms');
const boundaries=[
`Rails.application.routes.draw do
 namespace :admin, path: 'console' do
  scope(path: 'v1', module: :api) do
   resources :companies, controller: 'organizations', path: 'firms', only: [:show] do
    get :my_action, action: :view
    collection do
     match 'search'
    end
    member do
     get 'detail', to: 'details#show'
    end
   end
  end
 end
end`,
`Rails.application.routes.draw do
 if enabled
  resource :person
 end
 scope '/edge' do
  root 'home#index'
 end
 resources :status do
  resources :boxes
 end
end`,
`Rails.application.routes.draw do
 resources :buses do
  resources :children
 end
 resources :brushes do
  get 'my-action'
 end
 resources :churches do
  get :view
 end
end`,
"# resources :ignored\nget '/café😀', to: 'pages#show' # resource :comment\ntext = \"resources :string\"\n",
"Rails.application.routes.draw do\n resources :entries, only: [\n :show,\n :new\n ]\n resources :broken, only: [\n",
];
for(const source of boundaries)add('config/routes.rb',source,'nested scopes and boundary forms');
add('routes.RB','resources :ignored','extension gate');
const base={'Gemfile':"gem 'rails'\n"};
projects.push({name:'convention-bindings',files:{...base,'config/routes.rb':"Rails.application.routes.draw do\n resources :articles, only: [:index,:show]\n get '/missing', to: 'absent#show'\nend\n",'app/controllers/articles_controller.rb':'class ArticlesController\n def index; end\n def show; end\nend\n','app/models/user_profile.rb':'class UserProfile; end\n','app/models/concerns/a_p_i_record.rb':'class APIRecord; end\n','engine/app/models/legacy.rb':'class Legacy; end\n','app/controllers/api/v1/users_controller.rb':'class UsersController; end\n','engine/controllers/other_controller.rb':'class OtherController; end\n','app/helpers/application_helper.rb':'module ApplicationHelper; end\n','app/services/billing_service.rb':'class BillingService; end\n','app/jobs/send_job.rb':'class SendJob; end\n','app/workers/work_job.rb':'class WorkJob; end\n','lib/unrelated.rb':'class UserProfile; end\nclass MissingJob; end\n'}});
for(const project of projects){const dir=mkdtempSync(join(tmpdir(),'prometeu-rails-'));let indexed;try{
 for(const [path,source]of Object.entries(project.files)){mkdirSync(dirname(join(dir,path)),{recursive:true});writeFileSync(join(dir,path),source);if(path.endsWith('.rb'))add(path,source,project.name);}
 indexed=await Project.init(dir,{silent:true});const result=await indexed.indexAll();if(!result.success)throw Error('Index failed');
 project.nodes=indexed.queries.getAllNodes().map(strip);project.routes=indexed.queries.getNodesByKind('route').map(node=>({node:strip(node),targets:indexed.queries.getOutgoingEdges(node.id).map(edge=>({kind:edge.kind,node:strip(indexed.getNode(edge.target)),metadata:edge.metadata}))}));
 const context=new ReferenceResolver(dir,indexed.queries);context.initialize();project.resolutions=[];
 const origin=project.nodes.find(node=>node.kind==='file'&&node.language==='ruby');
 const names=new Set(['UserProfile','APIRecord','Legacy','ArticlesController','UsersController','OtherController','ApplicationHelper','BillingService','SendJob','WorkJob','MissingJob','AbsentController','articles#index','articles#show','absent#show','admin/zones#index','admin/zones#show','api/zones#index','bad-name#foo']);
 for(const node of project.nodes)if(node.kind==='route')names.add(node.qualifiedName.split('::route:')[1]);
 for(const name of names){const reference={fromNodeId:origin.id,referenceName:name,referenceKind:'references',line:1,column:0,filePath:origin.filePath,language:'ruby'};const result=resolver.resolve(reference,context.context);project.resolutions.push({name,claimed:resolver.claimsReference(name),expected:result?{id:result.targetNodeId,confidence:result.confidence}:null});}
 console.log(project.name,project.routes.length);
}finally{indexed?.close();rmSync(dir,{recursive:true,force:true});}}
const destination=dirname(fileURLToPath(import.meta.url));writeFileSync(join(destination,'cases.json'),JSON.stringify({revision,cases,projects},null,2)+'\n');writeFileSync(join(destination,'LICENSE'),readFileSync(join(root,'LICENSE')));console.log(cases.length,'extractions',projects.length,'projects');
