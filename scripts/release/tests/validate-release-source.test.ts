import assert from 'node:assert/strict';
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import test from 'node:test';

import { type Exec, type ExecRequest, type ExecResult } from '../release-contract.ts';
import { type SourceArguments, validateReleaseSource } from '../validate-release-source.ts';

const COMMIT = 'a'.repeat(40);
const SPARKLE_REVISION = '79bc9e872948e47877e76f194cb0c8e0412b0b90';

async function fixture(): Promise<string> {
	const root = await mkdtemp(join(tmpdir(), 'rsnap-source-test-'));
	await mkdir(join(root, 'native/macos-host'), { recursive: true });
	await writeFile(
		join(root, 'native/macos-host/Package.swift'),
		'.package(url: "https://github.com/sparkle-project/Sparkle", exact: "2.9.5")\n',
	);
	await writeFile(
		join(root, 'native/macos-host/Package.resolved'),
		JSON.stringify({
			pins: [
				{
					identity: 'sparkle',
					kind: 'remoteSourceControl',
					location: 'https://github.com/sparkle-project/Sparkle',
					state: { revision: SPARKLE_REVISION, version: '2.9.5' },
				},
			],
		}),
	);
	return root;
}

function successfulExec(): Exec {
	return async (request: ExecRequest): Promise<ExecResult> => {
		let stdout = '';
		if (request.command === 'cargo') {
			stdout = JSON.stringify({
				workspace_members: ['rsnap 1.2.3 (path+file:///fixture)'],
				packages: [
					{
						id: 'rsnap 1.2.3 (path+file:///fixture)',
						name: 'rsnap',
						version: '1.2.3',
						source: null,
					},
				],
			});
		} else {
			const key = request.args.join(' ');
			const responses = new Map<string, string>([
				['cat-file -t refs/tags/v1.2.3', 'tag'],
				[
					'cat-file -p refs/tags/v1.2.3',
					`object ${COMMIT}\ntype commit\ntag v1.2.3\n\nfixture`,
				],
				['rev-parse --verify refs/tags/v1.2.3^{commit}', COMMIT],
				[`rev-parse --verify ${COMMIT}^{commit}`, COMMIT],
				['rev-parse --verify HEAD^{commit}', COMMIT],
				['rev-parse --verify refs/remotes/origin/main^{commit}', COMMIT],
				[`merge-base --is-ancestor ${COMMIT} refs/remotes/origin/main`, ''],
			]);
			const value = responses.get(key);
			assert.notEqual(value, undefined, `unexpected command: ${key}`);
			stdout = value ?? '';
		}
		return {
			status: 0,
			stdout: Buffer.from(stdout),
			stderr: Buffer.alloc(0),
		};
	};
}

function argumentsFor(root: string): SourceArguments {
	return {
		repoRoot: root,
		tag: 'v1.2.3',
		eventCommit: COMMIT,
		repository: 'acg-box/rsnap',
		baseRef: 'refs/remotes/origin/main',
	};
}

function environment(): NodeJS.ProcessEnv {
	return {
		GITHUB_REF: 'refs/tags/v1.2.3',
		GITHUB_REPOSITORY: 'acg-box/rsnap',
		GITHUB_SHA: COMMIT,
	};
}

void test('validates Cargo locked metadata and the exact Sparkle revision', async () => {
	const root = await fixture();
	try {
		const result = await validateReleaseSource(
			argumentsFor(root),
			successfulExec(),
			environment(),
		);
		assert.deepEqual(result, {
			canonicalRepository: 'acg-box/rsnap',
			sparkleRevision: SPARKLE_REVISION,
			sparkleVersion: '2.9.5',
			tagCommit: COMMIT,
			version: '1.2.3',
		});
	} finally {
		await rm(root, { recursive: true, force: true });
	}
});

void test('rejects a Package.resolved Sparkle version mismatch', async () => {
	const root = await fixture();
	try {
		await writeFile(
			join(root, 'native/macos-host/Package.resolved'),
			JSON.stringify({
				pins: [
					{
						identity: 'sparkle',
						kind: 'remoteSourceControl',
						location: 'https://github.com/sparkle-project/Sparkle',
						state: { revision: SPARKLE_REVISION, version: '2.9.3' },
					},
				],
			}),
		);
		await assert.rejects(
			validateReleaseSource(argumentsFor(root), successfulExec(), environment()),
			/disagree on Sparkle version/,
		);
	} finally {
		await rm(root, { recursive: true, force: true });
	}
});

void test('rejects a Package.resolved Sparkle revision mismatch', async () => {
	const root = await fixture();
	try {
		await writeFile(
			join(root, 'native/macos-host/Package.resolved'),
			JSON.stringify({
				pins: [
					{
						identity: 'sparkle',
						kind: 'remoteSourceControl',
						location: 'https://github.com/sparkle-project/Sparkle',
						state: { revision: 'c'.repeat(40), version: '2.9.5' },
					},
				],
			}),
		);
		await assert.rejects(
			validateReleaseSource(argumentsFor(root), successfulExec(), environment()),
			/Sparkle revision must be/,
		);
	} finally {
		await rm(root, { recursive: true, force: true });
	}
});

void test('rejects workflow identity that differs from validated arguments', async () => {
	const root = await fixture();
	try {
		await assert.rejects(
			validateReleaseSource(argumentsFor(root), successfulExec(), {
				...environment(),
				GITHUB_SHA: 'c'.repeat(40),
			}),
			/GITHUB_SHA does not match/,
		);
	} finally {
		await rm(root, { recursive: true, force: true });
	}
});

void test('rejects downstream source output drift', async () => {
	const root = await fixture();
	try {
		await assert.rejects(
			validateReleaseSource(
				{
					...argumentsFor(root),
					expectedVersion: '1.2.4',
				},
				successfulExec(),
				environment(),
			),
			/rechecked release version/,
		);
	} finally {
		await rm(root, { recursive: true, force: true });
	}
});
