import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { appendFile, readFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { formatCommitChangelog } from './changelog-utils.mjs';

const env = process.env;
const apiRoot = `${env.GITHUB_API_URL}/repos/${env.GITHUB_REPOSITORY}`;
const dayMs = 86_400_000;
const beijingOffset = 8 * 3_600_000;
const shaPattern = /^[0-9a-f]{40}$/;
const corePattern = '(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)';
const stablePattern = new RegExp(`^${corePattern}$`);
const nightlyPattern = new RegExp(`^${corePattern}-nightly\\.([0-9]{8})$`);
const mainRef = 'refs/heads/main';
const updateRef = 'heads/nightly-updates';
const manifestName = 'nightly.json';
const platformKey = 'windows-x86_64-standard';

function requireValue(condition, message) {
  if (!condition) throw new Error(message);
}

async function request(path, { method = 'GET', body, optional = false, binary = false, token = env.GH_TOKEN } = {}) {
  const response = await fetch(new URL(path, `${apiRoot}/`), {
    method,
    signal: AbortSignal.timeout(60_000),
    headers: {
      Accept: binary ? 'application/octet-stream' : 'application/vnd.github+json',
      Authorization: `Bearer ${token}`,
      'X-GitHub-Api-Version': '2022-11-28',
      ...(body && { 'Content-Type': 'application/json' }),
    },
    body: body && JSON.stringify(body),
  });
  if (optional && response.status === 404) return null;
  if (!response.ok) {
    throw new Error(`${method} ${path} failed (${response.status}): ${await response.text()}`);
  }
  return response;
}

async function api(path, options) {
  const response = await request(path, options);
  return response && response.status !== 204 ? response.json() : null;
}

function createRef(ref, sha) {
  // GitHub can require Workflows permission to create refs at older commits.
  return api('git/refs', {
    method: 'POST', body: { ref, sha }, token: env.NIGHTLY_REF_TOKEN || env.GH_TOKEN,
  });
}

async function* pages(path) {
  while (path) {
    const response = await request(path);
    const items = await response.json();
    requireValue(Array.isArray(items), 'Expected a paginated GitHub API result');
    yield* items;
    path = response.headers.get('link')?.match(/<([^>]+)>;\s*rel="next"/)?.[1];
  }
}

async function assetBytes(asset) {
  const response = await request(`releases/assets/${asset.id}`, { binary: true });
  return Buffer.from(await response.arrayBuffer());
}

function dateStart(date) {
  const utc = Date.parse(`${date}T00:00:00Z`);
  requireValue(/^\d{4}-\d{2}-\d{2}$/.test(date) && Number.isFinite(utc) &&
    new Date(utc).toISOString().slice(0, 10) === date, `Invalid Beijing build date: ${date}`);
  return utc - beijingOffset;
}

function parseVersion(version, pattern) {
  const match = typeof version === 'string' && pattern.exec(version);
  requireValue(match, `Invalid release version: ${version}`);
  return match.slice(1).map(BigInt);
}

function compareNumbers(left, right) {
  for (let index = 0; index < left.length; index++) {
    if (left[index] !== right[index]) return left[index] > right[index] ? 1 : -1;
  }
  return 0;
}

function tagFor(date) {
  return `nightly-${date.replaceAll('-', '')}`;
}

async function refSha(ref, optional = false) {
  const value = await api(`git/ref/${ref}`, { optional });
  if (!value) return null;
  let object = value.object;
  while (object.type === 'tag') {
    object = (await api(`git/tags/${object.sha}`)).object;
  }
  requireValue(object.type === 'commit' && shaPattern.test(object.sha), `${ref} does not resolve to a commit`);
  return object.sha;
}

async function requireAncestor(base, head, message) {
  if (base === head) return;
  const result = await api(`compare/${base}...${head}?per_page=1`);
  requireValue(result.status === 'ahead' && result.merge_base_commit.sha === base, message);
}

async function checkHistory(sha) {
  await requireAncestor(sha, await refSha('heads/main'), `${sha} is outside current main history`);
  const stable = await api('releases/latest');
  const asset = stable.assets.find(({ name }) => name === 'latest.json');
  requireValue(asset, 'Latest stable release is missing latest.json');
  const { version } = JSON.parse(await assetBytes(asset));
  const core = parseVersion(version, stablePattern);
  requireValue(stable.tag_name === version, `Stable updater version ${version} differs from release tag ${stable.tag_name}`);
  await requireAncestor(await refSha(`tags/${version}`), sha, `${sha} does not contain stable tag ${version}`);
  return { core, stableTag: version };
}

export function buildNightlyNotes({
  version,
  date,
  sha,
  commits = [],
  baseRef,
  repository = env.GITHUB_REPOSITORY || 'Dreaminko/VRCS',
}) {
  const shortSha = typeof sha === 'string' && sha ? sha.slice(0, 7) : '';
  const heading = shortSha
    ? `VRCS ${version} Nightly (${date}, \`${shortSha}\`)`
    : `VRCS ${version} Nightly (${date})`;
  const compareUrl = baseRef && sha && baseRef !== sha
    ? `https://github.com/${repository}/compare/${baseRef}...${sha}`
    : undefined;
  return formatCommitChangelog(commits, { heading, compareUrl });
}

async function resolveNightlyBaseline(sha, stableTag) {
  try {
    const head = await refSha(updateRef, true);
    if (head) {
      const existing = await api(`contents/${manifestName}?ref=${head}`);
      const previous = JSON.parse(Buffer.from(existing.content, 'base64'));
      if (shaPattern.test(previous?.source_sha) && previous.source_sha !== sha) {
        const result = await api(`compare/${previous.source_sha}...${sha}?per_page=1`);
        if (result?.status === 'ahead' && result?.merge_base_commit?.sha === previous.source_sha) {
          return previous.source_sha;
        }
      }
    }
  } catch {
    // Fall back to the latest stable tag when previous nightly metadata is unavailable.
  }
  return stableTag;
}

async function collectNightlyNotes({ version, date, sha, stableTag }) {
  try {
    const baseRef = await resolveNightlyBaseline(sha, stableTag);
    const compare = await api(`compare/${baseRef}...${sha}?per_page=100`);
    const commits = Array.isArray(compare?.commits)
      ? compare.commits.map((entry) => ({
          sha: entry.sha,
          subject: String(entry.commit?.message || '').split(/\r?\n/, 1)[0],
        }))
      : [];
    return buildNightlyNotes({ version, date, sha, commits, baseRef });
  } catch (error) {
    console.warn(`Could not build detailed Nightly commit summary: ${error.message}`);
    return buildNightlyNotes({ version, date, sha, commits: [] });
  }
}

async function buildVersion(sha, date, stableCore) {
  const config = await api(`contents/apps/desktop/src-tauri/tauri.conf.json?ref=${sha}`);
  const { version } = JSON.parse(Buffer.from(config.content, 'base64'));
  const base = parseVersion(version, stablePattern);
  const nextPatch = [stableCore[0], stableCore[1], stableCore[2] + 1n];
  const core = compareNumbers(base, nextPatch) >= 0 ? base : nextPatch;
  return `${core.join('.')}-nightly.${tagFor(date).slice('nightly-'.length)}`;
}

async function releaseFor(tag, includeDraft = false) {
  const release = await api(`releases/tags/${tag}`, { optional: true });
  if (release || !includeDraft) return release;
  for await (const entry of pages('releases?per_page=100')) {
    if (entry.tag_name === tag) return entry;
  }
  return null;
}

async function requireTag(tag, sha, optional = false) {
  const taggedSha = await refSha(`tags/${tag}`, optional);
  requireValue(taggedSha === sha || (optional && taggedSha === null), `${tag} points at ${taggedSha}, expected ${sha}`);
  return taggedSha;
}

async function detect() {
  const run = await api(`actions/runs/${env.GITHUB_RUN_ID}`);
  const created = Date.parse(run.created_at);
  requireValue(Number.isFinite(created), 'Actions run has no valid creation time');
  const today = new Date(created + beijingOffset).toISOString().slice(0, 10);
  const date = env.GITHUB_EVENT_NAME === 'workflow_dispatch'
    ? env.NIGHTLY_REQUESTED_DATE
    : new Date(dateStart(today) + beijingOffset - dayMs).toISOString().slice(0, 10);
  const start = dateStart(date);
  requireValue(start < dateStart(today), `Beijing build date ${date} has not ended`);

  const types = new Set(['push', 'pr_merge', 'merge_queue_merge', 'force_push']);
  const query = new URLSearchParams({ ref: mainRef, direction: 'desc', per_page: '100' });
  for await (const activity of pages(`activity?${query}`)) {
    const timestamp = Date.parse(activity.timestamp);
    requireValue(Number.isFinite(timestamp), 'Repository activity has no valid timestamp');
    if (timestamp < start) break;
    if (timestamp >= start + dayMs || activity.ref !== mainRef ||
        !types.has(activity.activity_type) || activity.before === activity.after) continue;

    const sha = activity.after;
    requireValue(shaPattern.test(sha), 'Repository activity has no valid target SHA');
    const { core: stableCore, stableTag } = await checkHistory(sha);
    const tag = tagFor(date);
    const release = await releaseFor(tag);
    const published = Boolean(release && !release.draft);
    await requireTag(tag, sha, !published);
    const version = published ? '' : await buildVersion(sha, date, stableCore);
    const notes = published ? '' : await collectNightlyNotes({ version, date, sha, stableTag });
    const notesB64 = notes ? Buffer.from(notes, 'utf8').toString('base64') : '';
    await appendFile(env.GITHUB_OUTPUT, `sha=${sha}\ndate=${date}\nversion=${version}\npublished=${published}\nnotes_b64=${notesB64}\n`);
    console.log(`Selected ${sha} for ${date}${published ? ' (published release)' : ''}`);
    return;
  }
  console.log(`No main commit changes on ${date} (Beijing time)`);
}

function installerName(version) {
  return `VRCS-${version}-windows-x64.exe`;
}

function assetNames(version) {
  const installer = installerName(version);
  return [installer, `${installer}.sig`, `${installer}.sha256`, manifestName];
}

function validateManifest(bytes, date, sha, version) {
  const manifest = JSON.parse(bytes);
  const parts = parseVersion(manifest.version, nightlyPattern);
  requireValue(parts[3] === BigInt(date.replaceAll('-', '')) &&
    manifest.build_date === date && manifest.source_sha === sha &&
    (!version || manifest.version === version), 'Nightly manifest does not match the selected build');
  const platform = manifest.platforms?.[platformKey];
  const url = `${env.GITHUB_SERVER_URL}/${env.GITHUB_REPOSITORY}/releases/download/${tagFor(date)}/${installerName(manifest.version)}`;
  requireValue(platform?.url === url && typeof platform.signature === 'string' && platform.signature.trim() &&
    typeof manifest.notes === 'string' && manifest.notes.trim() && Number.isFinite(Date.parse(manifest.pub_date)),
  'Nightly manifest is missing valid updater metadata');
  return manifest;
}

function validateChecksum(manifest, signature, checksum, digest) {
  requireValue(signature.toString().trim() === manifest.platforms[platformKey].signature,
    'Nightly signature differs from the manifest');
  requireValue(/^[0-9a-f]{64}$/.test(digest) &&
    checksum.toString().trim() === `${digest}  ${installerName(manifest.version)}`,
  'Nightly installer checksum does not match the asset');
}

async function verifyRelease(release, tag, date, sha, version) {
  requireValue(release.tag_name === tag && release.prerelease, `Release ${tag} is not a Nightly prerelease`);
  const assets = new Map(release.assets.map(asset => [asset.name, asset]));
  const manifestAsset = assets.get(manifestName);
  requireValue(manifestAsset?.state === 'uploaded', `Release ${tag} is missing ${manifestName}`);
  const bytes = await assetBytes(manifestAsset);
  const manifest = validateManifest(bytes, date, sha, version);
  const names = assetNames(manifest.version);
  requireValue(release.assets.length === names.length && names.every(name => {
    const asset = assets.get(name);
    return asset?.state === 'uploaded' && asset.size > 0;
  }), `Release ${tag} does not have the expected four uploaded assets`);
  const [installer, signature, checksum] = names.map(name => assets.get(name));
  const digest = installer.digest?.match(/^sha256:([0-9a-f]{64})$/)?.[1];
  validateChecksum(manifest, await assetBytes(signature), await assetBytes(checksum), digest);
  return { bytes, manifest };
}

async function updateManifest(bytes, manifest) {
  await checkHistory(manifest.source_sha);
  await requireTag(tagFor(manifest.build_date), manifest.source_sha);
  const head = await refSha(updateRef, true);
  let parentTree;
  if (head) {
    const existing = await api(`contents/${manifestName}?ref=${head}`);
    const previousBytes = Buffer.from(existing.content, 'base64');
    const previous = JSON.parse(previousBytes);
    requireValue(shaPattern.test(previous.source_sha), 'Published manifest has an invalid source SHA');
    dateStart(previous.build_date);
    validateManifest(previousBytes, previous.build_date, previous.source_sha);
    if (previous.version === manifest.version) {
      requireValue(previousBytes.equals(bytes), 'Published manifest conflicts with the same Nightly version');
      console.log(`Nightly updater already points to ${manifest.version}`);
      return;
    }
    if (manifest.build_date <= previous.build_date || compareNumbers(
      parseVersion(manifest.version, nightlyPattern), parseVersion(previous.version, nightlyPattern)) <= 0) {
      console.log(`Keeping ${previous.version}; ${manifest.version} does not advance both date and version`);
      return;
    }
    parentTree = (await api(`git/commits/${head}`)).tree.sha;
  }

  const blob = await api('git/blobs', { method: 'POST', body: { content: bytes.toString('base64'), encoding: 'base64' } });
  const tree = await api('git/trees', {
    method: 'POST',
    body: { base_tree: parentTree, tree: [{ path: manifestName, mode: '100644', type: 'blob', sha: blob.sha }] },
  });
  const commit = await api('git/commits', {
    method: 'POST',
    body: { message: `Publish Nightly ${manifest.version}`, tree: tree.sha, parents: head ? [head] : [] },
  });
  if (head) {
    await api(`git/refs/${updateRef}`, { method: 'PATCH', body: { sha: commit.sha, force: false } });
  } else {
    await createRef(`refs/${updateRef}`, commit.sha);
  }
}

async function publish() {
  const { NIGHTLY_SHA: sha, NIGHTLY_DATE: date, NIGHTLY_VERSION: version } = env;
  dateStart(date);
  requireValue(shaPattern.test(sha), 'Publishing requires the selected full source SHA');
  const tag = tagFor(date);
  const { core: stableCore } = await checkHistory(sha);
  let release = await releaseFor(tag, true);
  const taggedSha = await requireTag(tag, sha, !release || release.draft);

  if (!release || release.draft) {
    requireValue(version && version === await buildVersion(sha, date, stableCore),
      'Stable version changed after detection or the selected build artifact is missing');
    const paths = assetNames(version).map(name => join('release-artifacts', name));
    const [installer, signature, checksum, metadata] = paths;
    const bytes = await readFile(metadata);
    const manifest = validateManifest(bytes, date, sha, version);
    const hash = createHash('sha256');
    for await (const chunk of createReadStream(installer)) hash.update(chunk);
    validateChecksum(manifest, await readFile(signature), await readFile(checksum), hash.digest('hex'));

    if (!taggedSha) {
      await createRef(`refs/tags/${tag}`, sha);
    }
    const fields = {
      name: `VRCS ${version}`,
      body: `${manifest.notes.trim()}\n\n---\n\nBuild date: ${date} (Beijing time)\n\nSource commit: \`${sha}\`\n\n[Actions run](${env.GITHUB_SERVER_URL}/${env.GITHUB_REPOSITORY}/actions/runs/${env.GITHUB_RUN_ID})`,
      target_commitish: sha,
      prerelease: true,
      make_latest: 'false',
    };
    release = release
      ? await api(`releases/${release.id}`, { method: 'PATCH', body: fields })
      : await api('releases', { method: 'POST', body: { ...fields, tag_name: tag, draft: true } });
    requireValue(release.draft, `Refusing to overwrite published assets for ${tag}`);
    const names = assetNames(version);
    for (const asset of release.assets) {
      if (!names.includes(asset.name)) await api(`releases/assets/${asset.id}`, { method: 'DELETE' });
    }
    execFileSync('gh', ['release', 'upload', tag, ...paths, '--clobber', '--repo', env.GITHUB_REPOSITORY], {
      stdio: 'inherit',
      timeout: 20 * 60_000,
    });
    release = await api(`releases/${release.id}`);
    requireValue(release.draft, `${tag} published before its assets could be verified`);
    const verified = await verifyRelease(release, tag, date, sha, version);
    requireValue(verified.bytes.equals(bytes), 'Uploaded manifest differs from the build artifact');
    requireValue(version === await buildVersion(sha, date, (await checkHistory(sha)).core),
      'Stable version changed before the draft could be published');
    await requireTag(tag, sha);
    const published = await api(`releases/${release.id}`, {
      method: 'PATCH', body: { draft: false, prerelease: true, make_latest: 'false' },
    });
    requireValue(!published.draft && published.prerelease, `Failed to publish ${tag} as a prerelease`);
    await updateManifest(verified.bytes, verified.manifest);
    return;
  }

  const verified = await verifyRelease(release, tag, date, sha, version);
  await updateManifest(verified.bytes, verified.manifest);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  requireValue(env.GITHUB_REF === mainRef, 'Nightly release workflow must run from main');
  const mode = process.argv[2];
  if (mode === 'detect') await detect();
  else if (mode === 'publish') await publish();
  else throw new Error('Expected detect or publish');
}
