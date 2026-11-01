// Independent full-graph capture through the pinned TypeScript resolver and SQLite store.
import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const revision = '6aa358ccbae30fbf27318f79ee183212f726f9ba';
const upstream = resolve(process.argv[2] ?? '');
if (!process.argv[2] || execFileSync('git', ['-C', upstream, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim() !== revision) throw new Error('Pass the pinned prepared upstream checkout.');
const require = createRequire(join(upstream, 'package.json'));
const { initGrammars, loadGrammarsForLanguages } = require('./dist/extraction/grammars.js');
const { TreeSitterExtractor } = require('./dist/extraction/tree-sitter.js');
const { CfmlExtractor } = require('./dist/extraction/cfml-extractor.js');
const { GraphTraverser } = require('./dist/graph/traversal.js');
const { GraphQueryManager } = require('./dist/graph/queries.js');
const Project = Object.values(require('./dist/index.js')).find(value => typeof value === 'function' && typeof value.init === 'function' && typeof value.open === 'function');
if (!Project) throw new Error('Missing upstream project API.');
if (!existsSync(join(upstream, 'dist/db/schema.sql'))) copyFileSync(join(upstream, 'src/db/schema.sql'), join(upstream, 'dist/db/schema.sql'));
await initGrammars();
const projects = [
  { name: 'typescript-imports', language: 'typescript', files: {
    'math.ts': 'export function calculate() { return 1; }\nexport class Worker { work() { return calculate(); } }\n',
    'barrel.ts': 'export { calculate as compute, Worker } from "./math";\n',
    'main.ts': 'import { compute, Worker } from "./barrel";\nexport function run() { const worker = new Worker(); worker.work(); return compute(); }\n',
  } },
  { name: 'python-imports', language: 'python', files: {
    'pkg/helpers.py': 'def calculate():\n    return 1\nclass Worker:\n    def work(self):\n        return calculate()\n',
    'pkg/main.py': 'from .helpers import calculate as compute, Worker\ndef run():\n    worker = Worker()\n    worker.work()\n    return compute()\n',
  } },
  { name: 'go-package', language: 'go', files: {
    'go.mod': 'module example.local/probe\n\ngo 1.22\n',
    'worker/worker.go': 'package worker\nfunc Calculate() int { return 1 }\ntype Worker struct {}\nfunc (w Worker) Work() int { return Calculate() }\n',
    'main.go': 'package main\nimport task "example.local/probe/worker"\nfunc run() int { return task.Calculate() }\n',
    'helper.go': 'package main\nfunc local() int { return run() }\n',
  } },
  { name: 'java-inheritance', language: 'java', files: {
    'src/demo/Base.java': 'package demo;\npublic class Base { public void work() {} }\n',
    'src/demo/Worker.java': 'package demo;\npublic class Worker extends Base { public void run() { this.work(); } }\n',
    'src/app/Main.java': 'package app;\nimport demo.Worker;\npublic class Main { public void run() { Worker worker = new Worker(); worker.run(); } }\n',
  } },
  { name: 'csharp-inheritance', language: 'csharp', uncertainty: [{ sourceFile: 'Main.cs', targetQualifiedName: 'Demo', kind: 'imports' }], files: {
    'Base.cs': 'namespace Demo { public class Base { public void Work() {} } }\n',
    'Worker.cs': 'namespace Demo { public class Worker : Base { public void Run() { this.Work(); } } }\n',
    'Main.cs': 'using Demo;\nclass Main { void Start() { Worker worker = new Worker(); worker.Run(); } }\n',
  } },
  { name: 'ruby-require', language: 'ruby', files: {
    'worker.rb': 'class Worker\n  def work\n    1\n  end\nend\n',
    'main.rb': 'require_relative "worker"\ndef run\n  worker = Worker.new\n  worker.work\nend\n',
  } },
  { name: 'rust-modules', language: 'rust', files: {
    'src/lib.rs': 'pub mod worker;\nuse crate::worker::calculate;\npub fn run() { calculate(); }\n',
    'src/worker.rs': 'pub fn calculate() {}\n',
  } },
  { name: 'cfml-components', language: 'cfml', files: {
    'Base.cfc': 'component { public function work() {} }\n',
    'Worker.cfc': 'component extends="Base" { public function run() { work(); } }\n',
    'Main.cfc': 'component { public function start() { new Worker().run(); } }\n',
  } },
  { name: 'php-namespaces', language: 'php', files: {
    'Worker.php': '<?php namespace Demo; class Worker { public function work() {} }\n',
    'Main.php': '<?php\nnamespace App;\nuse Demo\\Worker;\nfunction run() { $worker = new Worker(); $worker->work(); }\n',
  } },
  { name: 'erlang-modules', language: 'erlang', files: {
    'worker.erl': '-module(worker).\n-export([calculate/0]).\ncalculate() -> 1.\n',
    'main.erl': '-module(main).\n-export([run/0]).\nrun() -> worker:calculate().\n',
  } },
  { name: 'cross-language-collisions', language: 'typescript', languages: { 'foreign.py': 'python', 'caller.ts': 'typescript' }, files: {
    'foreign.py': 'class ForeignThing:\n    pass\ndef mystery():\n    pass\n',
    'caller.ts': 'export function run() { mystery(); return new ForeignThing(); }\n',
  } },
  { name: 'external-import-collision', language: 'python', files: {
    'local.py': 'def calculate():\n    return 1\n',
    'main.py': 'from external_package import calculate\ndef run():\n    return calculate()\n',
  } },
  { name: 'callback-registration', language: 'typescript', files: {
    'events.ts': 'export function dispatch(callback: () => void) { callback(); }\n',
    'main.ts': 'import { dispatch } from "./events";\nexport function handler() {}\nexport function run() { dispatch(handler); }\n',
  } },
  { name: 'python-shadowed-call', language: 'python', files: {
    'helpers.py': 'def calculate():\n    return 1\n',
    'main.py': 'def run(calculate):\n    return calculate()\n',
  } },
  { name: 'go-package-collisions', language: 'go', files: {
    'go.mod': 'module example.local/probe\n\ngo 1.22\n',
    'worker/worker.go': 'package worker\nfunc Calculate() int { return 1 }\nfunc hidden() {}\n',
    'wrong/wrong.go': 'package wrong\nfunc Calculate() int { return 2 }\nfunc hidden() {}\n',
    'main.go': 'package main\nimport task "example.local/probe/worker"\nfunc run() int { return task.Calculate() }\nfunc missing() { hidden() }\n',
  } },
  { name: 'go-external-package', language: 'go', files: {
    'go.mod': 'module example.local/probe\n\ngo 1.22\n',
    'local.go': 'package main\nfunc Calculate() {}\n',
    'main.go': 'package main\nimport task "outside.local/worker"\nfunc run() { task.Calculate() }\n',
  } },
  { name: 'java-imported-receiver', language: 'java', files: {
    'src/demo/Worker.java': 'package demo;\npublic class Worker { public void work() {} }\n',
    'src/other/Worker.java': 'package other;\npublic class Worker { public void work() {} }\n',
    'src/app/Main.java': 'package app;\nimport demo.Worker;\npublic class Main { public void run(Worker worker) { worker.work(); } }\n',
  } },
  { name: 'cfml-case-insensitive', language: 'cfml', files: {
    'Worker.cfc': 'component { public function Work() {} }\n',
    'Main.cfc': 'component { public function Start() { new worker().WORK(); } }\n',
  } },
  { name: 'java-private-method', language: 'java', files: {
    'src/demo/Worker.java': 'package demo;\npublic class Worker { private void hidden() {} }\n',
    'src/app/Main.java': 'package app;\npublic class Main { public void run() { hidden(); } }\n',
  } },
  { name: 'go-local-receivers', language: 'go', files: {
    'worker.go': 'package demo\ntype Worker struct {}\nfunc (worker Worker) Work() {}\n',
    'wrong/worker.go': 'package wrong\ntype Worker struct {}\nfunc (worker Worker) Work() {}\n',
    'main.go': 'package demo\nfunc run() { worker := &Worker{}; worker.Work(); var other Worker; other.Work() }\n',
  } },
];
const cases = [];
for (const project of projects) {
  const root = mkdtempSync(join(tmpdir(), 'prometeu-resolution-'));
  let indexed;
  try {
    const extraction = [];
    for (const [filePath, source] of Object.entries(project.files)) {
      mkdirSync(dirname(join(root, filePath)), { recursive: true });
      writeFileSync(join(root, filePath), source);
      if (filePath.endsWith('.mod')) continue;
      const language = project.languages?.[filePath] ?? project.language;
      await loadGrammarsForLanguages([language]);
      const result = language === 'cfml' ? new CfmlExtractor(filePath, source, language).extract() : new TreeSitterExtractor(filePath, source, language).extract();
      for (const node of result.nodes) delete node.updatedAt;
      delete result.durationMs;
      extraction.push({ filePath, language, result });
    }
    indexed = await Project.init(root, { silent: true });
    const indexedResult = await indexed.indexAll();
    if (!indexedResult.success) throw new Error(`Index failed for ${project.name}`);
    const indexedQueries = indexed.queries;
    const nodes = indexedQueries.getAllNodes();
    for (const node of nodes) delete node.updatedAt;
    const edges = nodes.flatMap(node => indexedQueries.getOutgoingEdges(node.id));
    const traverser = new GraphTraverser(indexedQueries);
    const manager = new GraphQueryManager(indexedQueries);
    const queriesResult = nodes.map(node => ({ nodeId: node.id, callers: traverser.getCallers(node.id).map(row => row.node.id).sort(), callees: traverser.getCallees(node.id).map(row => row.node.id).sort(), metrics: manager.getNodeMetrics(node.id) }));
    cases.push({ ...project, extraction, nodes, edges, queries: queriesResult });
    console.log(`${project.name}: ${nodes.length} nodes, ${edges.length} edges`);
  } finally {
    indexed?.close();
    rmSync(root, { recursive: true, force: true });
  }
}
writeFileSync(join(dirname(fileURLToPath(import.meta.url)), 'cases.json'), JSON.stringify({ revision, cases }, null, 2) + '\n');
