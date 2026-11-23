import { test } from 'node:test';
import assert from 'node:assert/strict';
import { imageNames, registryHost } from './image-names.mjs';

const base = { REGISTRY: 'registry.example.com', REPOSITORY: 'Galgathor/bucket', COMMIT: 'a'.repeat(40), REF: 'refs/heads/main', DEFAULT_BRANCH: 'main' };
for (const [input, expected] of [
  ['registry.example.com', 'registry.example.com'],
  [' https://REGISTRY.example.com/\n', 'registry.example.com'],
  ['http://registry.example.com:3000/', 'registry.example.com:3000'],
  ['localhost:5000', 'localhost:5000'],
  ['[::1]:5000', '[::1]:5000'],
]) test(`normalizes registry ${JSON.stringify(input)}`, () => assert.equal(registryHost(input), expected));

for (const input of ['', 'https://', 'https://registry.example.com/v2/', 'registry.example.com/owner', 'user:secret@registry.example.com', 'registry.example.com?token=secret', 'registry.example.com\nother', 'registry.example.com:99999', 'ftp://registry.example.com', 'registry..example.com']) {
  test(`rejects invalid registry without disclosing it: ${JSON.stringify(input)}`, () => {
    assert.throws(() => registryHost(input), error => error.message.startsWith('CONTAINER_REGISTRY must be') && !error.message.includes('secret'));
  });
}
test('uses the same normalized authority for login, tags, and cache', () => {
  const value = imageNames({ ...base, REGISTRY: ' https://REGISTRY.example.com:3000/ ' });
  assert.equal(value.registry, 'registry.example.com:3000');
  assert.equal(value.tags, `registry.example.com:3000/galgathor/bucket:sha-${base.COMMIT},registry.example.com:3000/galgathor/bucket:latest`);
  assert.equal(value.cache, 'registry.example.com:3000/galgathor/bucket:buildcache');
});
test('feature branches do not publish latest', () => assert.equal(imageNames({ ...base, REF: 'refs/heads/fix' }).tags.split(',').length, 1));
test('release tags retain the immutable commit tag', () => assert.equal(imageNames({ ...base, REF: 'refs/tags/v1.2.3' }).tags, `registry.example.com/galgathor/bucket:sha-${base.COMMIT},registry.example.com/galgathor/bucket:v1.2.3`));
test('rejects output injection through context values', () => {
  assert.throws(() => imageNames({ ...base, REPOSITORY: 'owner/repo\nregistry=evil' }));
  assert.throws(() => imageNames({ ...base, COMMIT: 'invalid' }));
});
