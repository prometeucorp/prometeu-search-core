// Development-only independent reference for the custom-format Rust ports.
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const revision = '6aa358ccbae30fbf27318f79ee183212f726f9ba';
if (!process.argv[2]) throw new Error('Pass the prepared upstream checkout directory.');
const upstream = resolve(process.argv[2]);
if (execFileSync('git', ['-C', upstream, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim() !== revision) {
  throw new Error(`Expected upstream ${revision}`);
}
const require = createRequire(join(upstream, 'package.json'));
const { extractFromSource } = require('./dist/extraction/tree-sitter.js');
const cases = [];
const add = (id, path, language, source) => cases.push({ id, path, language, source });
const liquid = (id, source) => add(id, 'sections/example.liquid', 'liquid', source);
const xml = (id, source) => add(id, 'mappers/Example.xml', 'xml', source);
const theme = (id, source) => add(id, 'templates/product.json', 'liquid', source);

liquid('empty-liquid', '');
liquid('braced-references', `é😀 {% render 'card', item: product %}{% include "card" %}
{%- section 'hero' -%}
{% sections 'header-group' %}
{% render 'card' %}{% render 'card' %}
`);
liquid('bare-liquid-block', `{% liquid
  assign heading = section.settings.title
  render 'card', title: heading
  include "price"
  section 'hero'
  sections 'header-group'
  capture title-text
  endcapture
  # render 'not-code'
  echo 'render "not-a-tag"'
%}
`);
liquid('silent-braced-regions', `{% comment %}{% render 'hidden' %}{% assign hidden = 1 %}
{% schema %}{"name":"Hidden"}{% endschema %}{% endcomment %}
{% raw %}{% section 'hidden' %}{% capture hidden %}x{% endcapture %}{% endraw %}
{% render 'visible' %}`);
liquid('silent-bare-regions', `{% liquid
  comment
  render 'hidden'
  assign hidden = 1
  endcomment
  raw
  capture hidden
  section 'hidden'
  endraw
  render 'visible'
%}`);
liquid('unterminated-comment', `{% render 'before' %}{% comment %}{% render 'after' %}`);
liquid('unterminated-raw', `{% raw %}{% render 'hidden' %}`);
liquid('tag-text-inside-quoted-argument', `{% assign text = "{% render 'fake' %}" %}
{% # render 'commented' %}
{% include 'real' %}`);
liquid('assignment-capture-collisions', `{% assign heading = 1 %}{% assign heading = 2 %}{% capture heading %}x{% endcapture %}
{% capture title-%}trim{% endcapture %}{% capture title-text-%}trim{% endcapture %}
{% capture 'quoted-name' %}one{% endcapture %}{% capture "double" %}two{% endcapture %}
{% assign invalid.member = 1 %}{% capture invalid.member %}bad{% endcapture %}
{% assign _ok = 1 %}{% assign value-2 = 2 %}`);
liquid('schema-translations', `{% schema %}
{"name":{"fr":"Bonjour","en":"Hello"},"settings":[{"id":"key","default":"do-not-expose"}]}
{% endschema %}
{% schema %}{"name":{"fr":"Bonjour","de":"Hallo"}}{% endschema %}
{% schema %}{bad json}{% endschema %}
{% schema %}{"name":"Hello"}{% endschema %}{% schema %}{"name":"Hello"}{% endschema %}`);
liquid('unicode-columns', `😀 é {% assign title = 1 %}{% capture title %}雪{% endcapture %}
{% render "café" %}
{% schema %}{"name":"😀"}{% endschema %}`);
liquid('schema-numeric-translation-keys', '{% schema %}{"name":{"10":"ten","2":"two","fr":"French"}}{% endschema %}');
for (const entry of [...cases].filter(entry => ['braced-references', 'assignment-capture-collisions', 'unicode-columns'].includes(entry.id))) {
  add(`${entry.id}-crlf`, entry.path, entry.language, entry.source.replace(/\n/g, '\r\n'));
}

theme('jsonc-sections', `/* Shopify-generated theme. */
{
  "sections": {
    "main": {"type": "main-product",},
    "hero": {"type": "hero"},
    "duplicate": {"type": "hero"},
  },
  "order": ["main", "hero",],
}`);
theme('json-escaped-types', '{"sections":{"雪":{"type":"caf\\u00e9"},"😀":{"type":"x\\\"y"},"bad":{"type":42},"string":"hero"}}');
theme('json-comments-and-strings', '{"comment":"/* not a comment */ // still text", "sections": {"main": {"type": "hero"}}} // allowed\n');
theme('json-truncated', '{"sections":{"main":{"type":"hero"}');
theme('json-array-root', '[{"sections":{"main":{"type":"hero"}}}]');
theme('json-nonobject-sections', '{"sections":[{"type":"hero"}]}');
theme('json-no-sections', '{"name":"Nothing"}');
theme('json-duplicate-sections-and-type-keys', '{"sections":{"main":{"type":"first","type":"second"},"main":{"type":"third"}},"sections":{"later":{"type":"fourth"}}}');
theme('json-unterminated-trailing-comment', '{"sections":{"main":{"type":"hero"}}} /* unterminated');
theme('json-unterminated-string', '{"sections":{"main":{"type":"hero"}},"tail":"unterminated}');

xml('mixed-quotes-includes', `<mapper namespace='com.example.Mapper'>
  <sql id="columns">id, name</sql>
  <select id='find' resultType='User' parameterType="int">SELECT <include refid='columns'/> FROM t</select>
  <update id="touch" parameterType='User'>UPDATE t SET x=1</update>
  <delete id='remove'>DELETE FROM t</delete>
</mapper>`);
xml('xml-comments-and-lines', `<mapper namespace="com.example.Mapper">
<!-- é😀
<select id="hidden">SELECT 1</select>
-->
<select id="visible">SELECT 2 <!-- <include refid="hidden"/> --></select>
</mapper>\n`);
xml('xml-cdata-markers', `<mapper namespace="com.example.Mapper"><![CDATA[<!--]]><select id="visible">SELECT 1</select><![CDATA[-->]]></mapper>`);
xml('xml-unterminated-comment', `<mapper namespace="M"><select id="before">SELECT 1</select><!-- <select id="after">SELECT 2</select>`);
xml('xml-vendor-duplicates', `<mapper namespace="com.example.Mapper"><select id="find" databaseId="oracle">SELECT 1 FROM dual</select><select id="find" databaseId='mysql'>SELECT 1</select><select id="find">SELECT 2</select></mapper>`);
xml('xml-qualified-includes', `<mapper namespace="com.example.Mapper"><sql id="base">id</sql><select id="all">SELECT <include refid="com.example.Mapper.base"/> FROM t <include refid="external.other.columns"/></select></mapper>`);
xml('xml-spaced-close-tags', `<mapper namespace="M"><sql id="cols">id</sql><select id="a"><include refid="cols"/></select ><select id="b">SELECT 2</select></mapper>`);
xml('xml-unicode-positions', `é😀<mapper namespace="M">雪<select id="find">SELECT '😀'</select></mapper>`);
xml('ibatis-namespaced', `<?xml version="1.0"?>
<sqlMap namespace="Account">
<sql id="columns">id, name</sql>
<select id="getById" resultClass="Account">SELECT <include refid="columns"/> FROM account</select>
<insert id="insert" parameterClass="Account">INSERT INTO account VALUES (#id#)</insert>
<statement id="runIt">SELECT 1</statement>
<procedure id="callIt">{ call do_it() }</procedure>
</sqlMap>`);
xml('ibatis-qualified-identifiers', `<sqlMap><select id="com.example.Account.getById">SELECT 1</select><sql id="Account.cols">id</sql><statement id="bare">SELECT <include refid="Account.cols"/></statement></sqlMap>`);
xml('ibatis-config', '<sqlMapConfig><sqlMap resource="Account.xml"/></sqlMapConfig>');
xml('plain-xml', '<project><name>Sample</name><select id="not-a-mapper">value</select></project>');
xml('mapper-missing-namespace', '<mapper><select id="not-qualified">SELECT 1</select></mapper>');
xml('mapper-unclosed-root', '<mapper namespace="M"><select id="find">SELECT 1</select>');
for (const entry of [...cases].filter(entry => ['mixed-quotes-includes', 'xml-comments-and-lines', 'ibatis-namespaced'].includes(entry.id))) {
  add(`${entry.id}-crlf`, entry.path, entry.language, entry.source.replace(/\n/g, '\r\n'));
}
add('yaml-file-tracking', 'config/routes.yml', 'yaml', 'route:\n  path: /example\n  controller: Sample\n');
add('twig-file-tracking', 'views/page.twig', 'twig', '{% include "layout.twig" %}{{ value }}\n');
add('properties-file-tracking', 'config/application.properties', 'properties', 'service.url=https://example.test\n');

for (const entry of cases) {
  entry.expected = extractFromSource(entry.path, entry.source, entry.language);
  for (const node of entry.expected.nodes) delete node.updatedAt;
  delete entry.expected.durationMs;
}
const destination = dirname(fileURLToPath(import.meta.url));
writeFileSync(join(destination, 'cases.json'), `${JSON.stringify({ revision, cases }, null, 2)}\n`);
writeFileSync(join(destination, 'LICENSE'), readFileSync(join(upstream, 'LICENSE')));
console.log(`Captured ${cases.length} custom-format cases from ${revision}.`);
