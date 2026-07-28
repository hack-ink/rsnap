import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import test from 'node:test';

import { HttpError, Publisher, type ReleaseTransport } from '../publish-github-release.ts';
import { type Exec, type ExecRequest, type ExecResult } from '../release-contract.ts';

const COMMIT = 'a'.repeat(40);
const TAG_OBJECT = 'b'.repeat(40);
const ASSET_NAMES = [
	'rsnap-aarch64-apple-darwin.zip',
	'appcast.xml',
	'rsnap-aarch64-apple-darwin.zip.sha256',
] as const;

interface Fixture {
	readonly root: string;
	readonly input: string;
	readonly bytes: ReadonlyMap<string, Buffer>;
}

async function fixture(): Promise<Fixture> {
	const root = await mkdtemp(join(tmpdir(), 'rsnap-publisher-test-'));
	const input = join(root, 'input');
	await mkdir(input);
	const bytes = new Map<string, Buffer>();
	for (const name of ASSET_NAMES) {
		const value = Buffer.from(`local ${name}`);
		bytes.set(name, value);
		await writeFile(join(input, name), value);
	}
	return { root, input, bytes };
}

function digest(bytes: Buffer): string {
	return `sha256:${createHash('sha256').update(bytes).digest('hex')}`;
}

function releaseJson(draft: boolean, tag = 'v1.2.3'): Record<string, unknown> {
	return {
		id: 42,
		tag_name: tag,
		draft,
		prerelease: false,
	};
}

class MockTransport implements ReleaseTransport {
	readonly log: string[] = [];
	readonly assetBytes = new Map<number, Buffer>();
	readonly assets: Record<string, unknown>[] = [];
	release: Record<string, unknown> | undefined;
	stableReleases: Record<string, unknown>[] = [];
	remoteSourceChecks = 0;
	failRemoteSourceCheck: number | undefined;
	failDigest = false;
	stableRaceVersion: string | undefined;
	failPatchAfterApply = false;
	releaseListScans = 0;
	mutateAssetsAfterFinalSourceCheck = false;
	mutateAssetsAfterPatch = false;
	mutateReleaseAfterFinalSourceCheck = false;
	nextAssetId = 100;
	readonly starterAssetIds = new Set<number>();

	async wait(): Promise<void> {}

	private assetJson(id: number, name: string, bytes: Buffer): Record<string, unknown> {
		const starter = this.starterAssetIds.has(id);
		return {
			id,
			name,
			size: starter ? 0 : bytes.length,
			state: starter ? 'starter' : 'uploaded',
			url: `https://api.github.com/repos/acg-box/rsnap/releases/assets/${String(id)}`,
			browser_download_url:
				`https://github.com/acg-box/rsnap/releases/download/` +
				`${this.release?.draft === false ? String(this.release.tag_name) : 'untagged-fixture'}/${name}`,
			digest: starter ? null : this.failDigest ? null : digest(bytes),
		};
	}

	seedAssets(bytesByName: ReadonlyMap<string, Buffer>): void {
		for (const [name, bytes] of bytesByName) {
			const id = this.nextAssetId;
			this.nextAssetId += 1;
			this.assetBytes.set(id, bytes);
			this.assets.push(this.assetJson(id, name, bytes));
		}
	}

	seedOldAssets(count: number): void {
		for (let index = 0; index < count; index += 1) {
			const bytes = Buffer.from(`old-${String(index)}`);
			const id = this.nextAssetId;
			this.nextAssetId += 1;
			this.assetBytes.set(id, bytes);
			this.assets.push(this.assetJson(id, `old-${String(index)}`, bytes));
		}
	}

	seedStarterAsset(name: string): void {
		const id = this.nextAssetId;
		this.nextAssetId += 1;
		this.starterAssetIds.add(id);
		this.assetBytes.set(id, Buffer.alloc(0));
		this.assets.push(this.assetJson(id, name, Buffer.alloc(0)));
	}

	async requestJson(request: Parameters<ReleaseTransport['requestJson']>[0]): Promise<unknown> {
		this.log.push(`${request.method} ${request.path}`);
		if (request.path.includes('/git/ref/tags/')) {
			return { object: { type: 'tag', sha: TAG_OBJECT } };
		}
		if (request.path.includes(`/git/tags/${TAG_OBJECT}`)) {
			return {
				tag: this.release?.tag_name ?? 'v1.2.3',
				object: { type: 'commit', sha: COMMIT },
			};
		}
		if (request.path.includes('/compare/')) {
			this.remoteSourceChecks += 1;
			if (this.mutateAssetsAfterFinalSourceCheck && this.remoteSourceChecks === 2) {
				const asset = this.assets.find((candidate) => candidate.name === ASSET_NAMES[0]);
				assert.notEqual(asset, undefined);
				const id = Number(asset?.id);
				const changed = Buffer.from('late remote mutation');
				this.assetBytes.set(id, changed);
			}
			if (this.mutateReleaseAfterFinalSourceCheck && this.remoteSourceChecks === 2) {
				assert.notEqual(this.release, undefined);
				this.release = { ...this.release, id: 43 };
			}
			return {
				merge_base_commit: {
					sha:
						this.failRemoteSourceCheck === this.remoteSourceChecks
							? 'c'.repeat(40)
							: COMMIT,
				},
			};
		}
		if (request.path.includes('/releases/tags/')) {
			if (this.release === undefined || this.release.draft === true) {
				throw new HttpError('not found', 404, 0);
			}
			return this.release;
		}
		if (/\/releases\?per_page=100&page=/.test(request.path)) {
			const pageText = /page=([0-9]+)$/.exec(request.path)?.[1];
			assert.notEqual(pageText, undefined);
			const page = Number(pageText);
			if (page === 1) {
				this.releaseListScans += 1;
			}
			const releases = [...this.stableReleases];
			if (this.release !== undefined) {
				releases.push(this.release);
			}
			if (this.stableRaceVersion !== undefined && this.releaseListScans >= 3) {
				releases.push(releaseJson(false, `v${this.stableRaceVersion}`));
			}
			return releases.slice((page - 1) * 100, page * 100);
		}
		if (request.method === 'POST' && request.path.endsWith('/releases')) {
			this.release = releaseJson(true);
			return this.release;
		}
		if (/\/releases\/42\/assets\?/.test(request.path)) {
			const pageText = /page=([0-9]+)$/.exec(request.path)?.[1];
			assert.notEqual(pageText, undefined);
			const page = Number(pageText);
			return this.assets.slice((page - 1) * 100, page * 100).map((asset) => {
				const id = Number(asset.id);
				const bytes = this.assetBytes.get(id);
				assert.notEqual(bytes, undefined);
				return this.assetJson(id, String(asset.name), bytes ?? Buffer.alloc(0));
			});
		}
		if (request.method === 'DELETE' && request.path.includes('/releases/assets/')) {
			const id = Number(request.path.split('/').at(-1));
			const index = this.assets.findIndex((asset) => asset.id === id);
			if (index >= 0) {
				this.assets.splice(index, 1);
			}
			this.assetBytes.delete(id);
			this.starterAssetIds.delete(id);
			return null;
		}
		if (request.method === 'PATCH' && request.path.endsWith('/releases/42')) {
			assert.deepEqual(request.body, { draft: false, make_latest: 'true' });
			assert.notEqual(this.release, undefined);
			this.release = { ...this.release, draft: false };
			if (this.mutateAssetsAfterPatch) {
				const asset = this.assets.find((candidate) => candidate.name === ASSET_NAMES[0]);
				assert.notEqual(asset, undefined);
				this.assetBytes.set(Number(asset?.id), Buffer.from('post-patch mutation'));
			}
			if (this.failPatchAfterApply) {
				throw new Error('connection closed after PATCH');
			}
			return this.release;
		}
		throw new Error(`unexpected request: ${request.method} ${request.path}`);
	}

	async uploadAsset(path: string, source: string): Promise<unknown> {
		this.log.push(`UPLOAD ${path}`);
		const name = new URL(`https://uploads.github.test${path}`).searchParams.get('name');
		assert.notEqual(name, null);
		const bytes = await readFile(source);
		const id = this.nextAssetId;
		this.nextAssetId += 1;
		this.assetBytes.set(id, bytes);
		const asset = this.assetJson(id, name ?? '', bytes);
		this.assets.push(asset);
		return asset;
	}

	async downloadAsset(path: string, destination: string, maxBytes: number): Promise<void> {
		this.log.push(`DOWNLOAD ${path}`);
		const id = Number(path.split('/').at(-1));
		const bytes = this.assetBytes.get(id);
		assert.notEqual(bytes, undefined);
		assert.equal(maxBytes, bytes?.length);
		await writeFile(destination, bytes ?? Buffer.alloc(0), { flag: 'wx' });
	}
}

function validatorExec(calls: ExecRequest[], log?: string[]): Exec {
	return async (request): Promise<ExecResult> => {
		calls.push(request);
		log?.push('VALIDATE');
		return {
			status: 0,
			stdout: Buffer.from('validated'),
			stderr: Buffer.alloc(0),
		};
	};
}

async function publisher(
	fixtureValue: Fixture,
	transport: MockTransport,
	calls: ExecRequest[],
	version = '1.2.3',
): Promise<Publisher> {
	return await Publisher.create(
		{
			repository: 'acg-box/rsnap',
			releaseCommit: COMMIT,
			githubSha: COMMIT,
			tag: `v${version}`,
			version,
			sparkleVersion: '2.9.4',
			inputDir: fixtureValue.input,
			validator: join(fixtureValue.root, 'validator'),
		},
		transport,
		validatorExec(calls, transport.log),
	);
}

void test('draft flow paginates delete-all, validates exact readback, and patches last', async () => {
	const fixtureValue = await fixture();
	const transport = new MockTransport();
	const calls: ExecRequest[] = [];
	try {
		transport.release = releaseJson(true);
		transport.seedOldAssets(101);
		const result = await (await publisher(fixtureValue, transport, calls)).publish();
		assert.equal(result, 'published');
		assert.equal(transport.remoteSourceChecks, 2);
		assert.deepEqual(
			new Set(transport.assets.map((asset) => asset.name)),
			new Set(ASSET_NAMES),
		);
		assert.equal(calls.length, 3);
		const patchIndex = transport.log.findIndex((entry) => entry.startsWith('PATCH '));
		assert.notEqual(patchIndex, -1);
		assert.match(transport.log[patchIndex - 1] ?? '', /^VALIDATE$/);
		assert.equal(
			transport.log.slice(patchIndex + 1).some((entry) => entry.startsWith('DOWNLOAD ')),
			true,
		);
		assert.equal(transport.log.at(-1), 'VALIDATE');
		assert.equal(
			transport.log.some((entry) => entry.includes('/releases/tags/v1.2.3')),
			true,
		);
		assert.equal(
			transport.log.some((entry) => entry.includes('/releases?per_page=100&page=1')),
			true,
		);
	} finally {
		await rm(fixtureValue.root, { recursive: true, force: true });
	}
});

void test('draft lookup rejects duplicate releases with the same tag', async () => {
	const fixtureValue = await fixture();
	const transport = new MockTransport();
	const calls: ExecRequest[] = [];
	try {
		transport.release = releaseJson(true);
		transport.stableReleases = [{ ...releaseJson(true), id: 43 }];
		await assert.rejects(
			(await publisher(fixtureValue, transport, calls)).publish(),
			/multiple GitHub releases use tag v1\.2\.3/,
		);
		assert.equal(
			transport.log.some((entry) => /^(POST|PATCH|DELETE|UPLOAD) /.test(entry)),
			false,
		);
	} finally {
		await rm(fixtureValue.root, { recursive: true, force: true });
	}
});

void test('repair deletes a starter asset with a null digest before uploading', async () => {
	const fixtureValue = await fixture();
	const transport = new MockTransport();
	const calls: ExecRequest[] = [];
	try {
		transport.release = releaseJson(true);
		transport.seedStarterAsset(ASSET_NAMES[0]);
		const result = await (await publisher(fixtureValue, transport, calls)).publish();
		assert.equal(result, 'published');
		assert.equal(transport.starterAssetIds.size, 0);
		assert.equal(
			transport.log.some((entry) => entry.startsWith('DELETE ')),
			true,
		);
	} finally {
		await rm(fixtureValue.root, { recursive: true, force: true });
	}
});

void test('same-tag public retry is read-only and validates downloaded public bytes', async () => {
	const fixtureValue = await fixture();
	const transport = new MockTransport();
	const calls: ExecRequest[] = [];
	try {
		transport.release = releaseJson(false);
		transport.stableReleases = [{ ...releaseJson(false, 'v99.0.0'), id: 99 }];
		transport.seedAssets(
			new Map(ASSET_NAMES.map((name) => [name, Buffer.from(`published ${name}`)])),
		);
		const result = await (await publisher(fixtureValue, transport, calls)).publish();
		assert.equal(result, 'already-public');
		assert.equal(
			transport.log.filter((entry) => /^(POST|PATCH|DELETE|UPLOAD) /.test(entry)).length,
			0,
		);
		assert.equal(calls.length, 2);
		assert.match(calls[1]?.args.join(' ') ?? '', /rsnap-release-download-/);
	} finally {
		await rm(fixtureValue.root, { recursive: true, force: true });
	}
});

void test('supports stable SemVer components beyond Number safe integer range', async () => {
	const fixtureValue = await fixture();
	const transport = new MockTransport();
	const calls: ExecRequest[] = [];
	const version = '900719925474099300000.0.0';
	try {
		transport.release = releaseJson(true, `v${version}`);
		const result = await (await publisher(fixtureValue, transport, calls, version)).publish();
		assert.equal(result, 'published');
	} finally {
		await rm(fixtureValue.root, { recursive: true, force: true });
	}
});

void test('finds a higher stable release on a bounded later page', async () => {
	const fixtureValue = await fixture();
	const transport = new MockTransport();
	const calls: ExecRequest[] = [];
	try {
		transport.release = releaseJson(true);
		transport.stableReleases = [
			...Array.from({ length: 100 }, (_, index) => ({
				...releaseJson(true, `v0.0.${String(index)}`),
				id: 1_000 + index,
			})),
			{ ...releaseJson(false, 'v2.0.0'), id: 2_000 },
		];
		await assert.rejects(
			(await publisher(fixtureValue, transport, calls)).publish(),
			/higher than every published stable/,
		);
	} finally {
		await rm(fixtureValue.root, { recursive: true, force: true });
	}
});

void test('a final remote source race leaves the release as a draft', async () => {
	const fixtureValue = await fixture();
	const transport = new MockTransport();
	const calls: ExecRequest[] = [];
	try {
		transport.release = releaseJson(true);
		transport.failRemoteSourceCheck = 2;
		await assert.rejects(
			(await publisher(fixtureValue, transport, calls)).publish(),
			/no longer reachable/,
		);
		assert.equal(transport.release?.draft, true);
		assert.equal(
			transport.log.some((entry) => entry.startsWith('PATCH ')),
			false,
		);
	} finally {
		await rm(fixtureValue.root, { recursive: true, force: true });
	}
});

void test('a higher stable release appearing at the final gate leaves the draft private', async () => {
	const fixtureValue = await fixture();
	const transport = new MockTransport();
	const calls: ExecRequest[] = [];
	try {
		transport.release = releaseJson(true);
		transport.stableRaceVersion = '2.0.0';
		await assert.rejects(
			(await publisher(fixtureValue, transport, calls)).publish(),
			/higher than every published stable/,
		);
		assert.equal(transport.release?.draft, true);
		assert.equal(
			transport.log.some((entry) => entry.startsWith('PATCH ')),
			false,
		);
	} finally {
		await rm(fixtureValue.root, { recursive: true, force: true });
	}
});

void test('a late draft metadata mutation blocks the final PATCH', async () => {
	const fixtureValue = await fixture();
	const transport = new MockTransport();
	const calls: ExecRequest[] = [];
	try {
		transport.release = releaseJson(true);
		transport.mutateReleaseAfterFinalSourceCheck = true;
		await assert.rejects(
			(await publisher(fixtureValue, transport, calls)).publish(),
			/release ID changed/,
		);
		assert.equal(
			transport.log.some((entry) => entry.startsWith('PATCH ')),
			false,
		);
	} finally {
		await rm(fixtureValue.root, { recursive: true, force: true });
	}
});

void test('a late asset mutation fails final byte validation before PATCH', async () => {
	const fixtureValue = await fixture();
	const transport = new MockTransport();
	const calls: ExecRequest[] = [];
	try {
		transport.release = releaseJson(true);
		transport.mutateAssetsAfterFinalSourceCheck = true;
		await assert.rejects(
			(await publisher(fixtureValue, transport, calls)).publish(),
			/release asset size changed/,
		);
		assert.equal(
			transport.log.some((entry) => entry.startsWith('PATCH ')),
			false,
		);
	} finally {
		await rm(fixtureValue.root, { recursive: true, force: true });
	}
});

void test('an ambiguous final PATCH converges by readback and validates public bytes', async () => {
	const fixtureValue = await fixture();
	const transport = new MockTransport();
	const calls: ExecRequest[] = [];
	try {
		transport.release = releaseJson(true);
		transport.failPatchAfterApply = true;
		const result = await (await publisher(fixtureValue, transport, calls)).publish();
		assert.equal(result, 'published');
		assert.equal(transport.release?.draft, false);
		assert.equal(transport.log.filter((entry) => entry.startsWith('PATCH ')).length, 1);
		assert.equal(calls.length, 3);
	} finally {
		await rm(fixtureValue.root, { recursive: true, force: true });
	}
});

void test('a successful PATCH is followed by exact public byte validation', async () => {
	const fixtureValue = await fixture();
	const transport = new MockTransport();
	const calls: ExecRequest[] = [];
	try {
		transport.release = releaseJson(true);
		transport.mutateAssetsAfterPatch = true;
		await assert.rejects(
			(await publisher(fixtureValue, transport, calls)).publish(),
			/release asset size changed/,
		);
		assert.equal(transport.log.filter((entry) => entry.startsWith('PATCH ')).length, 1);
		assert.equal(
			transport.log
				.slice(transport.log.findIndex((entry) => entry.startsWith('PATCH ')) + 1)
				.some((entry) => entry.includes('/releases/tags/v1.2.3')),
			true,
		);
	} finally {
		await rm(fixtureValue.root, { recursive: true, force: true });
	}
});

void test('publisher rejects a workflow SHA that differs from the validated release commit', async () => {
	const fixtureValue = await fixture();
	const transport = new MockTransport();
	try {
		await assert.rejects(
			Publisher.create(
				{
					repository: 'acg-box/rsnap',
					releaseCommit: COMMIT,
					githubSha: 'c'.repeat(40),
					tag: 'v1.2.3',
					version: '1.2.3',
					sparkleVersion: '2.9.4',
					inputDir: fixtureValue.input,
					validator: join(fixtureValue.root, 'validator'),
				},
				transport,
				validatorExec([]),
			),
			/GITHUB_SHA must match/,
		);
	} finally {
		await rm(fixtureValue.root, { recursive: true, force: true });
	}
});

void test('missing GitHub asset digest blocks publication', async () => {
	const fixtureValue = await fixture();
	const transport = new MockTransport();
	const calls: ExecRequest[] = [];
	try {
		transport.release = releaseJson(true);
		transport.failDigest = true;
		await assert.rejects(
			(await publisher(fixtureValue, transport, calls)).publish(),
			/release asset digest is invalid/,
		);
		assert.equal(transport.release?.draft, true);
	} finally {
		await rm(fixtureValue.root, { recursive: true, force: true });
	}
});

void test('credential-free dry-run performs only local artifact validation', async () => {
	const fixtureValue = await fixture();
	const transport = new MockTransport();
	const calls: ExecRequest[] = [];
	try {
		await (await publisher(fixtureValue, transport, calls)).dryRun();
		assert.equal(calls.length, 1);
		assert.deepEqual(transport.log, ['VALIDATE']);
	} finally {
		await rm(fixtureValue.root, { recursive: true, force: true });
	}
});
