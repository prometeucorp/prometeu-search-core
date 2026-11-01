// Run: node regenerate.mjs /path/to/pinned/reference-checkout
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createRequire } from 'node:module';
const require = createRequire(path.resolve(process.argv[2], 'package.json'));
const config = require('./dist/project-config.js');
const ignore = require('ignore');
const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'prometeu-config-oracle-'));
const entries = [
  ['empty', '{}'], ['null', 'null'], ['array', '[]'], ['malformed', '{ bad'],
  ['normalize', JSON.stringify({ extensions: { ' DOTA_LUA ': 'lua', '.TPL': 'php', '.h':'cpp', '.d.ts':'typescript', '.':'rust', '':'rust', '../rs':'rust', 'a\\b':'rust', '.zzz':'not-supported', '.rs':17, '.inc':'php' }, include:[ '  Tools/** ', '', null, 7], exclude: 'bad', includeIgnored: ['repos/'], deprioritize:['samples/'] })],
  ['collision-forward', '{"extensions":{".TS":"javascript","ts":"typescript"}}'],
  ['collision-reverse', '{"extensions":{"ts":"typescript",".TS":"javascript"}}'],
  ['duplicate-root', '{"extensions":null,"extensions":{".rs":"rust"}}'],
  ['numeric-keys', '{"extensions":{".1":"lua","1":"rust"}}'],
  ['unicode-trim', JSON.stringify({extensions:{'\ufeff.TS\ufeff':'typescript','\u0085.rs':'rust'},include:['\ufeffsrc/\ufeff','\u0085data/']})],
  ['invalid-fields', '{"extensions":[],"includeIgnored":null,"include":{},"deprioritize":[false,"  ","  x/ "]}'],
];
const paths = ['src/a.ts','src/nested/a.ts','src/keep.ts','other/src/a.ts','SRC/a.TS','root.ts','x.ts','deep/x.ts','samples/demo.ts','tools/a.ts','Tools/a.ts','repos/a/code.ts','a[b]/file.ts','foo?bar/a.ts','fooXbar/a.ts','dot/.hidden.ts','nested/node_modules/a.ts'];
for (const patterns of [['src/'],['src/','!src/keep.ts'],['src/*','!src/keep.ts'],['/src/'],['**/*.ts','!src/**'],['**/*.ts','!src/','src/keep.ts'],['# comment','\\#literal'],['a[b]/'],['a\\[b]/'],['foo?bar/'],['Tools/**','!Tools/generated/'],['*.TS'],['!src/'],['src/**/a.ts'],['foo{a,b}/'],['../outside/']]) {
  entries.push([`patterns-${entries.length}`,JSON.stringify({ include:patterns,exclude:patterns,includeIgnored:patterns,deprioritize:patterns })]);
}
const cases=[];
try {
  for (const [id,source] of entries) {
    fs.writeFileSync(path.join(directory,config.PROJECT_CONFIG_FILENAME),source);
    config.clearProjectConfigCache();
    const expected={extensions:config.loadExtensionOverrides(directory),includeIgnored:config.loadIncludeIgnoredPatterns(directory),exclude:config.loadExcludePatterns(directory),include:config.loadIncludePatterns(directory),deprioritize:config.loadDeprioritizePatterns(directory)};
    const matches=[];
    for (const field of ['include','exclude','includeIgnored','deprioritize']) {
      const matcher=ignore().add(expected[field]);
      for (const file of paths) {
        matches.push({field,path:file,directory:false,expected:matcher.ignores(file)});
        const parent=file.slice(0,file.lastIndexOf('/')+1);
        if(parent) matches.push({field,path:parent.slice(0,-1),directory:true,expected:matcher.ignores(parent)});
      }
    }
    cases.push({id,source,expected,matches});
  }
} finally { fs.rmSync(directory,{recursive:true}); }
fs.writeFileSync(new URL('./cases.json',import.meta.url),JSON.stringify({revision:'6aa358ccbae30fbf27318f79ee183212f726f9ba',cases},null,2)+'\n');
