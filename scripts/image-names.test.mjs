import { test } from 'node:test';
import assert from 'node:assert/strict';
import { imageNames, registryHost, publicationNames } from './image-names.mjs';

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

test('publishes matching commit and latest tags to Gitea and GHCR', () => {
  const names=publicationNames({...base,GHCR_USERNAME:' GitHub-User '});
  assert.deepEqual(names.tags.split(','),[
    `registry.example.com/galgathor/bucket:sha-${base.COMMIT}`,
    'registry.example.com/galgathor/bucket:latest',
    `ghcr.io/github-user/bucket:sha-${base.COMMIT}`,
    'ghcr.io/github-user/bucket:latest',
  ]);
  assert.equal(names.registry,'registry.example.com');
  assert.equal(names.ghcr_username,'github-user');
  assert.equal(names.cache,'registry.example.com/galgathor/bucket:buildcache');
});
test('publishes matching release tags without latest',()=>{
  const names=publicationNames({...base,GHCR_USERNAME:'github-user',REF:'refs/tags/v1.2.3'});
  assert.equal(names.tags.split(',').length,4);
  assert.ok(names.tags.includes('registry.example.com/galgathor/bucket:v1.2.3'));
  assert.ok(names.tags.includes('ghcr.io/github-user/bucket:v1.2.3'));
  assert.ok(!names.tags.includes(':latest'));
});
test('feature branches publish only immutable tags to both registries',()=>{
  const names=publicationNames({...base,GHCR_USERNAME:'github-user',REF:'refs/heads/feature'});
  assert.equal(names.tags.split(',').length,2);
  assert.ok(names.tags.split(',').every(t=>t.endsWith(`:sha-${base.COMMIT}`)));
});
test('rejects missing or malformed GHCR usernames without disclosing them',()=>{
  for(const value of [undefined,'','https://github.com/private','name/extra','name\noutput=secret','-name','name-']) {
    assert.throws(()=>publicationNames({...base,GHCR_USERNAME:value}),e=>e.message==='GHCR_USERNAME must be a valid GitHub account username for container publication.');
  }
});
test('deduplicates tags and never returns the GHCR token',()=>{
  const names=publicationNames({...base,REGISTRY:'ghcr.io',GHCR_USERNAME:'Galgathor',GHCR_TOKEN:'test-only-secret'});
  assert.equal(names.tags.split(',').length,2);
  assert.ok(!JSON.stringify(names).includes('test-only-secret'));
});
