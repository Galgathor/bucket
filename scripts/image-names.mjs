import { appendFileSync } from 'node:fs';
import { isIP } from 'node:net';
import { pathToFileURL } from 'node:url';

export function registryHost(value = '') {
  const authority = value.trim().replace(/^https?:\/\//i, '').replace(/\/+$/, '').toLowerCase();
  const fail = () => { throw new Error('CONTAINER_REGISTRY must be a registry hostname with an optional port, or a root HTTP(S) URL. Remove paths, credentials, queries, and internal whitespace.'); };
  if (!authority || /[\s/@?#]/.test(authority) || authority.includes('://')) fail();
  let url;
  try { url = new URL(`https://${authority}`); } catch { fail(); }
  const hostname = url.hostname;
  if (hostname.startsWith('[')) {
    if (isIP(hostname.slice(1, -1)) !== 6) fail();
  } else if (!hostname.split('.').every(label => /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/.test(label))) fail();
  // URL parsing must not silently discard or rewrite any authority characters.
  if (!/^(?:[a-z0-9.-]+|\[[a-f0-9:]+\])(?::[0-9]+)?$/.test(authority)) fail();
  return authority;
}

export function imageNames({ REGISTRY, REPOSITORY = '', COMMIT = '', REF = '', DEFAULT_BRANCH = '' }) {
  const registry = registryHost(REGISTRY);
  const repository = REPOSITORY.toLowerCase();
  if (!/^[a-z0-9]+(?:[._-][a-z0-9]+)*\/[a-z0-9]+(?:[._-][a-z0-9]+)*$/.test(repository)) throw new Error('The repository must have a valid owner/name for container publication.');
  if (!/^[a-f0-9]{40,64}$/.test(COMMIT)) throw new Error('A valid commit SHA is required for container publication.');
  const image = `${registry}/${repository}`;
  const tags = [`${image}:sha-${COMMIT}`];
  if (DEFAULT_BRANCH && REF === `refs/heads/${DEFAULT_BRANCH}`) tags.push(`${image}:latest`);
  if (/^refs\/tags\/v[0-9]+\.[0-9]+\.[0-9]+([.-][a-zA-Z0-9.-]+)?$/.test(REF)) {
    const tag = REF.slice('refs/tags/'.length);
    if (tag.length > 128) throw new Error('The release image tag is too long.');
    tags.push(`${image}:${tag}`);
  }
  return { registry, tags: tags.join(','), cache: `${image}:buildcache` };
}

export function publicationNames(env) {
  const gitea = imageNames(env);
  const username = (env.GHCR_USERNAME ?? '').trim().toLowerCase();
  if (!/^[a-z0-9](?:[a-z0-9-]{0,37}[a-z0-9])?$/.test(username)) {
    throw new Error('GHCR_USERNAME must be a valid GitHub account username for container publication.');
  }
  const repository = `${username}/${env.REPOSITORY.split('/')[1]}`;
  const ghcr = imageNames({ ...env, REGISTRY: 'ghcr.io', REPOSITORY: repository });
  return {
    ...gitea,
    tags: [...new Set([...gitea.tags.split(','), ...ghcr.tags.split(',')])].join(','),
    ghcr_username: username,
  };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    if (!process.env.GITHUB_OUTPUT) throw new Error('The Actions output file is unavailable.');
    const outputs = publicationNames(process.env);
    appendFileSync(process.env.GITHUB_OUTPUT, Object.entries(outputs).map(([key, value]) => `${key}=${value}\n`).join(''));
  } catch (error) {
    // Never include the supplied registry value, credentials, or raw parser errors.
    console.error(error.message);
    process.exitCode = 1;
  }
}
