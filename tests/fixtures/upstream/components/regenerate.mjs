// Development-only TypeScript/WASM oracle; Rust tests use the saved results.
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const revision = '6aa358ccbae30fbf27318f79ee183212f726f9ba';
const upstream = resolve(process.argv[2] ?? '');
if (!process.argv[2]) throw new Error('Pass the prepared upstream checkout.');
if (execFileSync('git', ['-C', upstream, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim() !== revision) throw new Error('Incorrect upstream revision');
const require = createRequire(join(upstream, 'package.json'));
const ts = require('typescript');
const { initGrammars, loadGrammarsForLanguages } = require('./dist/extraction/grammars.js');
const extractors = Object.fromEntries(['svelte', 'vue', 'astro'].map(language => {
  const exports = require(`./dist/extraction/${language}-extractor.js`);
  return [language, exports[`${language[0].toUpperCase()}${language.slice(1)}Extractor`]];
}));
await initGrammars();
await loadGrammarsForLanguages(['typescript', 'tsx', 'javascript']);
const cases = [], seen = new Set();
const upstreamProduct = JSON.parse(readFileSync(join(upstream, 'package.json'), 'utf8')).name.split('/').at(-1);
function capture(language, source, origin) {
  source = source.replace(new RegExp(upstreamProduct, 'gi'), 'prometeu');
  if (seen.has(language + source)) return;
  seen.add(language + source);
  const filePath = `src/Probe.${language}`;
  const expected = new extractors[language](filePath, source).extract();
  for (const node of expected.nodes) delete node.updatedAt;
  delete expected.durationMs;
  cases.push({ id: `${language}-${cases.length + 1}`, origin, language, filePath, source, expected });
}
for (const name of [
  'sfc-component-owns-script', 'sfc-private-declarations', 'sfc-alias-module-imports',
  'vue-options-api', 'vue-template-calls', 'vue-self-closing-script', 'vue-tsx-script',
  'vue-module-imports', 'vue-store-extraction', 'svelte-self-closing-script',
  'astro-component-markup-only',
]) {
  const origin = `__tests__/${name}.test.ts`;
  const text = readFileSync(join(upstream, origin), 'utf8');
  const parsed = ts.createSourceFile(origin, text, ts.ScriptTarget.Latest, true);
  function visit(node) {
    if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) {
      const value = node.text;
      const isComponent = /<(?:script|template)(?:\s|>)/.test(value) || /^\s*---\n/.test(value);
      if (isComponent) {
        const languages = name.startsWith('sfc-') ? ['vue', 'svelte', 'astro'] : [name.split('-')[0]];
        for (const language of languages) capture(language, value, origin);
      } else if (name === 'vue-options-api' && /\bexport\s+default\b/.test(value)) {
        capture('vue', `<script>\n${value}\n</script>`, origin);
      }
    }
    ts.forEachChild(node, visit);
  }
  visit(parsed);
}
for (const language of ['vue', 'svelte', 'astro']) {
  const source = '<script lang="ts">\n/* é😀 */ const value = load();\nexport function run() { return other(value); }\n</script>\n<Widget/><button @click="run">{run()} {{ run() }}</button>\n';
  capture(language, source, 'unicode LF');
  capture(language, source.replace(/\n/g, '\r\n'), 'unicode CRLF');
  capture(language, '<script src="a.js" />\n<script>function go() { call(); }</script   >\n<Thing/>\n<style>.foo { content: "<Fake>{fake()}" }</style>\n', 'self-closing, whitespace closing, style exclusion');
}
writeFileSync(join(dirname(fileURLToPath(import.meta.url)), 'cases.json'), JSON.stringify({ revision, oracle: 'Independent TypeScript SFC extractors delegating to the WASM walker', cases }, null, 2) + '\n');
console.log(`Captured ${cases.length} SFC reference cases.`);
