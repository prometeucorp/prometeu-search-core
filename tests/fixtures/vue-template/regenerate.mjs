// Development-only oracle for the pure Vue template-call scanner.
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { runInNewContext } from 'node:vm';

const revision = '6aa358ccbae30fbf27318f79ee183212f726f9ba';
if (!process.argv[2]) throw new Error('Pass the prepared upstream checkout directory.');
const upstream = resolve(process.argv[2]);
if (execFileSync('git', ['-C', upstream, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim() !== revision) {
  throw new Error(`Expected upstream ${revision}`);
}
const require = createRequire(join(upstream, 'package.json'));
const ts = require('typescript');
const { vueTemplateCalls } = require('./dist/extraction/vue-template-calls.js');
const cases = [];
const seen = new Set();
let currentTest = '';
function capture(source) {
  const result = vueTemplateCalls(source);
  if (!seen.has(source)) {
    cases.push({
      id: `${cases.length + 1}: ${currentTest}`,
      source,
      expected: result.map(({ name, offset }) => [name, Buffer.byteLength(source.slice(0, offset), 'utf8')]),
    });
    seen.add(source);
  }
  return result;
}

// Execute only the pure scanner suite, not the separate indexing/IO suites.
// Capture the independent scanner's actual arguments and outputs, including
// fixture values assembled by the upstream tests themselves.
const text = readFileSync(join(upstream, '__tests__/vue-template-calls.test.ts'), 'utf8');
const ast = ts.createSourceFile('vue-template-calls.test.ts', text, ts.ScriptTarget.Latest, true);
const names = ast.statements.find(statement => ts.isVariableStatement(statement)
  && statement.declarationList.declarations.some(declaration => declaration.name.getText(ast) === 'names'));
const suite = ast.statements.find(statement => ts.isExpressionStatement(statement)
  && ts.isCallExpression(statement.expression)
  && statement.expression.expression.getText(ast) === 'describe'
  && statement.expression.arguments[0]?.text === 'vueTemplateCalls');
if (!names || !suite) throw new Error('Pinned pure scanner suite not found.');
const ignoreExpectation = new Proxy(() => ignoreExpectation, {
  get: () => ignoreExpectation,
  apply: () => ignoreExpectation,
});
runInNewContext(ts.transpileModule(`${names.getText(ast)}\n${suite.getText(ast)}`, {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText, {
  describe: (_name, callback) => callback(),
  it: (name, callback) => { currentTest = name; callback(); },
  expect: ignoreExpectation,
  vueTemplateCalls: capture,
}, { timeout: 5000 });

const additional = [
  ['unicode offsets and quoted closing angle', '<template>é😀<p :title="choose(a > b, café())">{{ 雪() }} {{ helper() }}</p></template>'],
  ['self-closing scope does not leak', '<template><Row v-for="row in rows()" @click="row.save()"/><p>{{ row() }}{{ done() }}</p></template>'],
  ['nested templates and root closing', '<template><template v-if="enabled()"><template v-for="x in rows()">{{ render(x) }}</template>{{ afterInner() }}</template>{{ done() }}</template><template>{{ second() }}</template>'],
  ['template substitutions and escapes', '<template><p :title="`plain(fake()) ${format(value)} ${nested(`${inner()}`)}`">{{ done() }}</p></template>'],
  ['regular expressions and comments', '<template><p :x="/fake\\(call\\)/.test(input) && real()" @click="/* hidden() */ save() // later()">{{ end() }}</p></template>'],
  ['arrow destructuring and default values', '<template><p :f="({row, value: renamed}) => row.save() + renamed() + outside()" :g="(x = defaulted()) => x.run() + other()"/></template>'],
  ['function names and parameter scope', '<template><p :f="function handler(item) { item.run(); helper(); }" :g="function (value) { return format(value) }"/></template>'],
  ['slot destructuring aliases', '<template><Row v-slot:default="{ item: row, save = fallback }">{{ row.label() }}{{ save() }}{{ format(row) }}</Row>{{ row() }}</template>'],
  ['v-pre subtree', '<template><div v-pre :x="ignored()"><span>{{ ignored() }}</span></div>{{ live() }}</template>'],
  ['scripts styles comments', '<script>const x = "<template>{{ wrong() }}</template>"</script><style>.x { content: "{{ wrong() }}" }</style><!-- <template>{{ wrong() }}</template> --><template>{{ right() }}</template>'],
  ['plain attributes and unquoted binding', '<template><p title="plain()" :title=bare() v-bind:data-id="identify()" @click.stop.prevent="save()"/></template>'],
  ['globals helpers and constructors', '<template>{{ Number(value()) }}{{ Math.min(one(), two()) }}{{ $t(label()) }}{{ new Thing(input()) }}{{ globalThis.parseInt(text()) }}</template>'],
];
for (const [name, source] of additional) {
  currentTest = name;
  capture(source);
}
const destination = dirname(fileURLToPath(import.meta.url));
writeFileSync(join(destination, 'cases.json'), `${JSON.stringify({ revision, cases }, null, 2)}\n`);
writeFileSync(join(destination, 'LICENSE'), readFileSync(join(upstream, 'LICENSE')));
console.log(`Captured ${cases.length} Vue scanner cases from ${revision}.`);
