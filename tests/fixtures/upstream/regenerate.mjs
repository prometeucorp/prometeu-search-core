// Development-only oracle. Rust tests consume the checked-in snapshots offline.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const revision = '6aa358ccbae30fbf27318f79ee183212f726f9ba';
const upstream = resolve(process.argv[2] ?? '');
if (!process.argv[2]) throw new Error('Pass the prepared upstream checkout directory.');
const actualRevision = execFileSync('git', ['-C', upstream, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();
if (actualRevision !== revision) throw new Error(`Expected upstream ${revision}, found ${actualRevision}`);

const require = createRequire(join(upstream, 'package.json'));
// Construct the reference walker directly, bypassing native addon routing.
const { TreeSitterExtractor } = require('./dist/extraction/tree-sitter.js');
const { initGrammars, loadGrammarsForLanguages, getParser } = require('./dist/extraction/grammars.js');
const { EXTRACTORS } = require('./dist/extraction/languages/index.js');
const upstreamProduct = JSON.parse(readFileSync(join(upstream, 'package.json'), 'utf8')).name.split('/').at(-1);
const branding = new RegExp(upstreamProduct, 'gi');
const destination = dirname(fileURLToPath(import.meta.url));
for (const directory of ['source', 'prepared', 'expected']) mkdirSync(join(destination, directory), { recursive: true });

const fixtures = [
  ['typescript', 'src/extraction/kernel/loader.ts', 'loader.ts'],
  ['tsx', 'torture.tsx'],
  ['javascript', 'torture.js'],
  ['jsx', 'torture.js'],
  ['java', 'Torture.java'],
  ['python', 'torture.py'],
  ['go', 'torture.go'],
  ['c', 'torture.c'],
  ['cpp', 'torture.cpp'],
  ['rust', 'torture.rs'],
  ['csharp', 'Torture.cs'],
  ['ruby', 'torture.rb'],
  ['php', 'torture.php'],
  ['swift', 'torture.swift'],
  ['kotlin', 'torture.kt'],
  ['r', 'torture.R'],
  ['lua', 'torture.lua'],
  ['luau', 'torture.luau'],
  ['scala', 'torture.scala'],
  ['dart', 'torture.dart'],
].map(([language, source, filePath]) => ({
  language,
  upstreamSource: filePath ? source : `__tests__/fixtures/kernel-parity/${source}`,
  filePath: filePath ?? source,
}));

await initGrammars();
await loadGrammarsForLanguages(fixtures.map(({ language }) => language));
const cases = [];
function capture(language, filePath, source, upstreamSource, suffix = '') {
  // The fixture tests behavior, not the original project's product name.
  source = source.replace(branding, 'prometeu');
  const prepared = EXTRACTORS[language]?.preParse?.(source, filePath) ?? source;
  const tree = getParser(language).parse(prepared);
  if (!tree || tree.rootNode.hasError) throw new Error(`Fixture must parse without recovery: ${language}/${filePath}`);
  tree.delete();
  const result = new TreeSitterExtractor(filePath, source, language).extract();
  if (result.errors.length) throw new Error(`Reference extraction failed: ${JSON.stringify(result.errors)}`);
  const id = `${language}${suffix}`;
  const originalPath = `source/${id}-${filePath}`;
  const inputPath = prepared === source ? originalPath : `prepared/${id}-${filePath}`;
  writeFileSync(join(destination, originalPath), source);
  if (inputPath !== originalPath) writeFileSync(join(destination, inputPath), prepared);
  for (const node of result.nodes) delete node.updatedAt;
  delete result.durationMs;
  const expectedPath = `expected/${id}.json`;
  writeFileSync(join(destination, expectedPath), `${JSON.stringify(result, null, 2)}\n`);
  cases.push({
    id, language, filePath, upstreamSource, source: originalPath, input: inputPath, expected: expectedPath,
    sourceSha256: createHash('sha256').update(source).digest('hex'),
    inputSha256: createHash('sha256').update(prepared).digest('hex'),
  });
}

for (const fixture of fixtures) {
  capture(fixture.language, fixture.filePath, readFileSync(join(upstream, fixture.upstreamSource), 'utf8'), fixture.upstreamSource);
}

// The first named-object-literal regression from upstream's TS/JS parity suite.
// Keep its astral character before declarations: native byte columns must become
// the same UTF-16 columns as the independent WASM implementation.
const unicode = [
  '/* é😀 */ const Api = { read() { helper(); }, close: () => helper(), gen: function* () { yield 1; }, \'quoted-key\': function () {}, [dyn()]() { helper(); }, eager: helper(), alias: helper, helper };',
  'function helper() {}',
  '/** Saved. */',
  'export const Exported = { save() { helper(); } };',
  '(function () { const Local = { run() { helper(); } }, data = { x: 1 }; Local.run(); window.WS = { ws() { Local.run(); } }; })();',
  'App.utils = { pad(s) { return s; } };',
  '// The page.',
  'dw_page = { start() { helper(); } };',
  'function setup() { ns.late = { go() {} }; let h; h = { on() {} }; }',
  'module.exports = { cjs() {} };',
  'Foo.prototype = { proto() {} };',
  'self.handlers = { click() {} };',
  'consume({ ephemeral() {} });',
  '',
].join('\n');
for (const [suffix, source] of [['-unicode-lf', unicode], ['-unicode-crlf', unicode.replace(/\n/g, '\r\n')]]) {
  capture('typescript', 'literal.ts', source, '__tests__/kernel-tsjs-parity.test.ts', suffix);
}

writeFileSync(join(destination, 'cases.json'), `${JSON.stringify({ revision, oracle: 'TypeScript/WASM TreeSitterExtractor, bypassing native addon routing', cases }, null, 2)}\n`);
writeFileSync(join(destination, 'LICENSE'), readFileSync(join(upstream, 'LICENSE')));
console.log(`Captured ${cases.length} cases from ${revision}.`);
