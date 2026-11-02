// Run the original Python regression projects and retain their actual graph snapshots.
import { execFileSync } from 'node:child_process';
import { createRequire, Module } from 'node:module';
import { readFileSync, writeFileSync, readdirSync, statSync, renameSync } from 'node:fs';
import { join, resolve, dirname } from 'node:path';
import { createHash } from 'node:crypto';
import vm from 'node:vm';

const root = resolve(process.argv[2]);
const revision = '6aa358ccbae30fbf27318f79ee183212f726f9ba';
if (execFileSync('git', ['-C', root, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim() !== revision) throw Error('Wrong reference revision');
const require = createRequire(join(root, 'package.json'));
const ts = require('typescript');
const library = require('./dist/index.js');
const Project = Object.values(library).find(value => typeof value === 'function' && typeof value.init === 'function' && typeof value.open === 'function');
const { ReferenceResolver } = require('./dist/resolution/index.js');
const matcher = require('./dist/resolution/name-matcher.js');
const helperPath = join(root, 'dist/resolution/name-matcher.js');
const helperModule = new Module(helperPath);
helperModule.filename = helperPath;
helperModule.paths = Module._nodeModulePaths(dirname(helperPath));
helperModule._compile(readFileSync(helperPath, 'utf8') + '\nexports.captureAttribute = matchPythonAttrCall; exports.captureAnnotation = pythonAnnotationEvidence; exports.captureValue = pythonValueEvidence; exports.captureExpression = pythonExprScan; exports.captureScope = pythonBindingScope; exports.captureScopeBindings = pythonScopeBindings; exports.captureFactory = matchPythonFactoryChain;\n', helperPath);
const helpers = helperModule.exports;
const imports = require('./dist/resolution/import-resolver.js');
const { extractFromSource } = require('./dist/extraction/tree-sitter.js');
const { initGrammars, loadAllGrammars } = require('./dist/extraction/grammars.js');
await initGrammars(); await loadAllGrammars();
const answer = result => result ? { target: result.targetNodeId, confidence: result.confidence, resolvedBy: result.resolvedBy, metadata: result.metadata ?? null } : null;
const projects = [], known = new Set();
const snapshots = new WeakMap();
let current = '';

function capture(project, changed = true) {
  const nodes = project.queries.getAllNodes();
  if (!nodes.length || !nodes.some(node => node.language === 'python')) return;
  const directory = project.projectRoot;
  const files = {};
  function walk(relative = '') {
    for (const name of readdirSync(join(directory, relative)).sort()) {
      if (name.startsWith('.') || name === '__pycache__' || name === 'node_modules') continue;
      const path = relative ? `${relative}/${name}` : name;
      const stat = statSync(join(directory, path));
      if (stat.isDirectory()) walk(path);
      else if (stat.size <= 1024 * 1024 && (nodes.some(node => node.filePath === path) || /\.(?:py|pyi|toml|cfg|json)$/.test(name) || name.startsWith('requirements'))) files[path] = readFileSync(join(directory, path), 'utf8');
    }
  }
  walk();
  const key = createHash('sha256').update(JSON.stringify(files)).digest('hex');
  // A test may change live files to prove the index keeps its earlier snapshot.
  // Such a close does not describe a fresh graph of those changed files.
  if (!changed && snapshots.has(project) && snapshots.get(project) !== key) return;
  snapshots.set(project, key);
  if (known.has(key)) return;
  known.add(key);
  const resolver = new ReferenceResolver(directory, project.queries);
  resolver.initialize();
  resolver.warmCaches();
  const context = resolver.context;
  const frameworks = resolver.getDetectedFrameworks();
  const references = [];
  for (const [path, source] of Object.entries(files)) {
    if (!/\.pyi?$/.test(path)) continue;
    const extracted = extractFromSource(path, source, 'python', frameworks);
    for (const raw of extracted.unresolvedReferences) {
      const reference = { ...raw, filePath: path, language: 'python' };
      const name = /^([A-Za-z_]\w*)/.exec(raw.referenceName)?.[1];
      const scope = name ? { name, binding: matcher.pythonReceiverBinding(name, reference, context), alias: matcher.pythonFunctionModuleAlias(name, reference, context), moduleKeys: [...matcher.pythonModuleImportKeys(name, path, context)].sort() } : null;
      if (scope) { const local = helpers.captureScope(name, reference, context); scope.local = local ? { node: local.id, details: helpers.captureScopeBindings(local, name, context) } : null; }
      const dot = raw.referenceName.lastIndexOf('.');
      const attribute = dot > 0 && raw.referenceKind === 'calls' ? helpers.captureAttribute(raw.referenceName.slice(0, dot), raw.referenceName.slice(dot + 1), reference, context) : undefined;
      references.push({ reference, expected: answer(resolver.resolveOne(reference)), imported: answer(imports.resolveViaImport(reference, context)), exact: answer(matcher.matchByExactName(reference, context)), scope, typedLocal: answer(matcher.matchPythonTypedLocalCall(reference, context)), attribute: { handled: attribute !== undefined, answer: answer(attribute) }, factory: answer(helpers.captureFactory(reference, context)), builtin: resolver.isBuiltInOrExternal(reference) });
    }
  }
  const mapping = Object.fromEntries(Object.keys(files).filter(path => /\.pyi?$/.test(path)).map(path => [path, context.getImportMappings(path, 'python')]));
  const modules = [];
  for (const [from, mappings] of Object.entries(mapping)) for (const mapping of mappings) {
    for (const specifier of new Set([mapping.source, mapping.exportedName !== '*' && `${mapping.source}${mapping.source.endsWith('.') ? '' : '.'}${mapping.exportedName}`].filter(Boolean))) {
      modules.push({ from, specifier, target: imports.resolveImportPath(specifier, from, 'python', context) });
    }
  }
  const snapshot = {
    name: current, files, frameworks,
    nodes: nodes.map(node => { const copy = { ...node }; delete copy.updatedAt; return copy; }),
    edges: nodes.flatMap(node => project.queries.getOutgoingEdges(node.id)),
    references, mapping, modules,
    supertypes: Object.fromEntries(nodes.filter(node => node.kind === 'class').map(node => [node.id, (context.getSupertypeNodes?.(node.id) ?? []).map(node => node.id)])),
  };
  projects.push(snapshot);
  console.log(projects.length, current, nodes.length, references.length);
}
const close = Project.prototype.close;
Project.prototype.close = function (...args) {
  if (!this.closed) capture(this, false);
  return close.apply(this, args);
};
for (const method of ['indexAll', 'sync']) {
  const original = Project.prototype[method];
  Project.prototype[method] = async function (...args) {
    const result = await original.apply(this, args);
    capture(this);
    return result;
  };
}

const noop = new Proxy(function () {}, { get: (_, key) => key === 'then' ? undefined : noop, apply: () => noop });
const tests = readdirSync(join(root, '__tests__')).filter(name => (name === 'function-ref.test.ts' || /^(?:python-|pytest-fixture)/.test(name)) && name.endsWith('.test.ts') && !name.includes('resolution-work')).sort();
for (const file of tests) {
  const suite = { name: file, beforeAll: [], afterAll: [], beforeEach: [], afterEach: [], entries: [] };
  let active = suite;
  const describe = (name, fn) => {
    const child = { name, beforeAll: [], afterAll: [], beforeEach: [], afterEach: [], entries: [] };
    active.entries.push(child); const prior = active; active = child; fn(); active = prior;
  };
  describe.each = rows => (name, fn) => rows.forEach((row, i) => describe(`${name} [${i}]`, () => fn(...(Array.isArray(row) ? row : [row]))));
  describe.skip = () => {};
  const it = (name, fn) => active.entries.push({ name, fn });
  it.each = rows => (name, fn) => rows.forEach((row, i) => it(`${name} [${i}]`, () => fn(...(Array.isArray(row) ? row : [row]))));
  it.skip = () => {}; it.skipIf = condition => condition ? it.skip : it; it.runIf = condition => condition ? it : it.skip;
  const environments = new Map();
  const vi = new Proxy({
    spyOn: () => noop,
    stubEnv: (name, value) => { if (!environments.has(name)) environments.set(name, process.env[name]); process.env[name] = value; },
    unstubAllEnvs: () => { for (const [name, value] of environments) { if (value === undefined) delete process.env[name]; else process.env[name] = value; } environments.clear(); },
  }, { get: (target, name) => target[name] ?? noop });
  const vitest = { describe, it, test: it, expect: noop, vi };
  for (const hook of ['beforeAll', 'afterAll', 'beforeEach', 'afterEach']) vitest[hook] = fn => active[hook].push(fn);
  const path = join(root, '__tests__', file);
  const localRequire = specifier => {
    if (specifier === 'vitest') return vitest;
    if (specifier.startsWith('../src')) return require(specifier.replace('../src', './dist'));
    if (specifier.startsWith('.')) return createRequire(path)(specifier);
    return require(specifier);
  };
  const source = ts.transpileModule(readFileSync(path, 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true } }).outputText;
  const exports = {};
  vm.runInNewContext(source, { require: localRequire, exports, module: { exports }, process, console, Buffer, setTimeout, clearTimeout, __dirname: dirname(path), __filename: path }, { filename: path });
  async function run(group, parents = []) {
    const chain = [...parents, group];
    current = chain.map(group => group.name).join(' / ');
    for (const fn of group.beforeAll) await fn();
    for (const entry of group.entries) {
      if (entry.entries) { await run(entry, chain); continue; }
      current = [...chain.map(group => group.name), entry.name].join(' / ');
      try {
        for (const parent of chain) for (const fn of parent.beforeEach) await fn();
        await entry.fn();
      } finally {
        for (const parent of [...chain].reverse()) for (const fn of parent.afterEach) await fn();
      }
    }
    for (const fn of group.afterAll) await fn();
  }
  await run(suite);
}
const annotations = new Set(['Client', '"Client"', "'models.Client'", 'Optional[Client]', 'typing.Union[Client, None]', 'None | Client', 'Client | None', 'Optional[Optional[Client]]', 'Client[Thing]', 'typing.List[Client]', 'Any', 'collections.deque[Client]']);
const expressions = new Set(['Client()', 'Client() if flag else Fake()', 'Client() if flag else Client()', 'await make_client()', 'await Client()', 'Pool().acquire()', '"emoji 😀"', 'Client(\n  1,\n)', 'Client(', "'broken", '"""continued', 'value \\', 'None', '[]']);
for (const project of projects) for (const source of Object.values(project.files)) for (const line of source.split('\n')) {
  const annotation = /:\s*([^=\n]+?)(?:\s*=|$)/.exec(line)?.[1]; if (annotation) annotations.add(annotation);
  const assigned = /(?:^|\s)=\s*(.*)$/.exec(line)?.[1]; if (assigned) expressions.add(assigned);
  const returned = /\breturn\s+(.+)$/.exec(line)?.[1]; if (returned) expressions.add(returned);
}
const evidence = {
  annotations: [...annotations].map(raw => ({ raw, expected: helpers.captureAnnotation(raw) })),
  values: [...expressions].map(raw => ({ raw, signature: null, expression: helpers.captureExpression(raw), expected: helpers.captureValue(raw, null) })),
};
for (const signature of ['(self, cap: Client)', '(self, *, cap: Optional[Client])', '(cap: "Client")', '(cap: list[Client])', '(cap: object, other: Client)']) evidence.values.push({ raw: 'cap', signature, expression: helpers.captureExpression('cap'), expected: helpers.captureValue('cap', signature) });
writeFileSync(new URL('./cases.json.tmp', import.meta.url), JSON.stringify({ revision, projects, evidence }, null, 2) + '\n');
renameSync(new URL('./cases.json.tmp', import.meta.url), new URL('./cases.json', import.meta.url));
