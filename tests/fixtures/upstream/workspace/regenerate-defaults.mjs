// Strict default-export proof from the original scanner and native extraction records.
import {createRequire} from 'node:module';import {join,resolve} from 'node:path';import {execFileSync} from 'node:child_process';import {writeFileSync} from 'node:fs';
const upstream=resolve(process.argv[2]),revision='6aa358ccbae30fbf27318f79ee183212f726f9ba';if(execFileSync('git',['rev-parse','HEAD'],{cwd:upstream,encoding:'utf8'}).trim()!==revision)throw Error('Wrong revision');const require=createRequire(join(upstream,'package.json'));
const {isStatedDefaultExport}=require('./dist/resolution/import-resolver.js');const {templateLiteralTextAt}=require('./dist/resolution/strip-comments.js');
const {initGrammars,loadGrammarsForLanguages}=require('./dist/extraction/grammars.js');const {extractFromSource}=require('./dist/extraction/tree-sitter.js');await initGrammars();await loadGrammarsForLanguages(['typescript','svelte']);
const sources=[
'export function loader() {}\nexport default function Home() {}',
'export default\nasync function* Generator() {}',
'export default abstract class Service {}',
'const Home = () => 1;\nexport default Home;',
'export const Home = () => 1;\nexport { Home as default };',
'export const First = () => 1;\nexport default memo(First);',
'export function First() {}\nexport default new First();',
'export function First() {}\nexport default {First};',
'export function First() {}\nexport default function() {}',
'export function First() {}\nexport default class extends First {}',
'export function First() {}\nexport default (First);',
'export function First() {}\nexport default (x): Number => x;',
'export function First() {}\n// export default First\n/* export default First */',
'function App() {}\nconst template = `\nexport default App\n`;',
'function App() {}\nconst template = `${`nested`}\nexport default App\n`;\nexport default App;',
'function App() {}\nconst matcher = /[`]/;\nexport default App;',
'function App() {}\nconst template = `${ /[}]/.test(value) ? `nested ${value}` : value }\nexport default App\n`;',
'function App() {}\nconst template = `unterminated\nexport default App',
'function named() {}\nmodule.exports = named;',
'function named() {}\nexports = module.exports = named;',
'module.exports = function named() {}',
'module.exports = class Named {}',
'function named() {}\nmodule.exports = require("./other");',
'function named() {}\nmodule.exports = make(named);',
'export default function () {}\nfunction Named() {}\nexport default Named;',
'export default class {}\nmodule.exports = function named() {}',
];
const inputs=sources.map(source=>({path:'module.ts',source,language:'typescript'}));inputs.push({path:'types.d.cts',source:'export declare function declared(): void;',language:'typescript'},{path:'Widget.svelte',source:'<script>export let name;</script><h1>{name}</h1>',language:'svelte'});
const cases=[];for(const input of inputs){const result=await extractFromSource(input.path,input.source,input.language);const nodes=result.nodes;const context={readFile:()=>input.source,getNodesInFile:()=>nodes,getNodesInFileNamed:(_file,name)=>nodes.filter(n=>n.name===name),getExportedNodesInFile:()=>nodes.filter(n=>n.isExported)};cases.push({...input,nodes,expected:nodes.map(node=>({id:node.id,stated:isStatedDefaultExport(node,context)}))});}
const templates=[...sources,'const x = `plain 😀 ${`inner ${/`/.test(x)}`} tail`;','const x = `escaped \\` tail`;','if (value) /[`]/.test(value); export default App;'];
const spans=templates.map(source=>{const test=templateLiteralTextAt(source);return {source,expected:test?Array.from({length:source.length+1},(_,at)=>test(at)):null};});
writeFileSync(new URL('./defaults.json',import.meta.url),JSON.stringify({revision,cases,spans},null,2)+'\n');
