import assert from 'node:assert/strict';
import {execFileSync, spawnSync} from 'node:child_process';
import {mkdtempSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {fileURLToPath} from 'node:url';
import test from 'node:test';

test('regenerators reject unpinned checkouts before loading reference code', () => {
  const checkout = mkdtempSync(join(tmpdir(), 'prometeu-regeneration-'));
  try {
    execFileSync('git', ['init', '-q', checkout]);
    execFileSync('git', ['-C', checkout, '-c', 'user.name=Fixture Test',
      '-c', 'user.email=fixture@example.invalid', '-c', 'core.hooksPath=/dev/null',
      'commit', '-q', '--allow-empty', '-m', 'Unpinned fixture checkout']);
    for (const script of ['config/regenerate.mjs', 'analysis/regenerate-dead-code.mjs',
      'viewer/regenerate.mjs']) {
      const result = spawnSync(process.execPath,
        [fileURLToPath(new URL(script, import.meta.url)), checkout],
        {cwd: checkout, encoding: 'utf8'});
      assert.notEqual(result.status, 0, script);
      assert.match(result.stderr, /Wrong reference revision/, script);
      assert.doesNotMatch(result.stderr, /MODULE_NOT_FOUND|ENOENT/, script);
    }
  } finally {
    rmSync(checkout, {recursive: true, force: true});
  }
});
