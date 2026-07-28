#!/usr/bin/env node

import { appendFile, readFile } from 'node:fs/promises';
import { resolve } from 'node:path';

import {
	CANONICAL_REPOSITORY,
	ContractError,
	EXPECTED_SPARKLE_SOURCE,
	EXPECTED_SPARKLE_REVISION,
	EXPECTED_SPARKLE_VERSION,
	checkedExec,
	defaultExec,
	isMainModule,
	isRecord,
	parseStableTag,
	requireCondition,
	type Exec,
} from './release-contract.ts';

const FULL_SHA_PATTERN = /^[0-9a-f]{40}$/;
const SPARKLE_DECLARATION_PATTERN =
	/\.package\s*\(\s*url:\s*"https:\/\/github\.com\/sparkle-project\/Sparkle"\s*,\s*exact:\s*"([^"]+)"\s*\)/g;

export interface SourceArguments {
	readonly repoRoot: string;
	readonly tag: string;
	readonly eventCommit: string;
	readonly repository: string;
	readonly baseRef: string;
	readonly githubOutput?: string;
	readonly expectedCommit?: string;
	readonly expectedVersion?: string;
	readonly expectedSparkleVersion?: string;
}

export interface SourceValidation {
	readonly canonicalRepository: string;
	readonly sparkleRevision: string;
	readonly sparkleVersion: string;
	readonly tagCommit: string;
	readonly version: string;
}

interface CargoPackage {
	readonly name: string;
	readonly version: string;
	readonly source: unknown;
}

function parseArguments(argv: readonly string[]): SourceArguments {
	const values = new Map<string, string>();
	for (let index = 0; index < argv.length; index += 2) {
		const name = argv[index];
		const value = argv[index + 1];
		requireCondition(
			name?.startsWith('--') === true && value !== undefined,
			'invalid arguments',
		);
		requireCondition(!values.has(name), `duplicate argument: ${name}`);
		values.set(name, value);
	}

	const required = (name: string): string => {
		const value = values.get(name)?.trim() ?? '';
		requireCondition(value !== '', `${name} is required`);
		return value;
	};
	const optional = (name: string): string | undefined => {
		const value = values.get(name)?.trim() ?? '';
		return value === '' ? undefined : value;
	};

	const allowed = new Set([
		'--repo-root',
		'--tag',
		'--event-commit',
		'--repository',
		'--base-ref',
		'--github-output',
		'--expected-commit',
		'--expected-version',
		'--expected-sparkle-version',
	]);
	for (const name of values.keys()) {
		requireCondition(allowed.has(name), `unknown argument: ${name}`);
	}

	const githubOutput = optional('--github-output');
	const expectedCommit = optional('--expected-commit');
	const expectedVersion = optional('--expected-version');
	const expectedSparkleVersion = optional('--expected-sparkle-version');
	return {
		repoRoot: resolve(optional('--repo-root') ?? '.'),
		tag: required('--tag'),
		eventCommit: required('--event-commit'),
		repository: required('--repository'),
		baseRef: required('--base-ref'),
		...(githubOutput === undefined ? {} : { githubOutput }),
		...(expectedCommit === undefined ? {} : { expectedCommit }),
		...(expectedVersion === undefined ? {} : { expectedVersion }),
		...(expectedSparkleVersion === undefined ? {} : { expectedSparkleVersion }),
	};
}

async function git(exec: Exec, repoRoot: string, args: readonly string[]): Promise<string> {
	const result = await checkedExec(exec, {
		command: 'git',
		args,
		cwd: repoRoot,
		timeoutMs: 20_000,
	});
	return result.stdout.toString('utf8').trim();
}

async function validateVersions(
	exec: Exec,
	repoRoot: string,
	tag: string,
): Promise<Pick<SourceValidation, 'version' | 'sparkleVersion' | 'sparkleRevision'>> {
	const version = parseStableTag(tag).text;
	const metadataResult = await checkedExec(exec, {
		command: process.env.RSNAP_CARGO_BIN?.trim() || 'cargo',
		args: ['metadata', '--locked', '--no-deps', '--format-version', '1'],
		cwd: repoRoot,
		timeoutMs: 60_000,
		maxOutputBytes: 16 * 1024 * 1024,
	});
	let metadata: unknown;
	try {
		metadata = JSON.parse(metadataResult.stdout.toString('utf8'));
	} catch (error) {
		throw new ContractError(`cargo metadata returned invalid JSON: ${String(error)}`);
	}
	requireCondition(isRecord(metadata), 'cargo metadata root must be an object');
	requireCondition(Array.isArray(metadata.packages), 'cargo metadata packages must be an array');
	requireCondition(
		Array.isArray(metadata.workspace_members),
		'cargo metadata workspace_members must be an array',
	);
	const workspaceMembers = new Set(
		metadata.workspace_members.filter((value): value is string => typeof value === 'string'),
	);
	requireCondition(workspaceMembers.size > 0, 'Cargo workspace must contain packages');

	const packages: CargoPackage[] = [];
	for (const value of metadata.packages) {
		requireCondition(isRecord(value), 'cargo metadata package must be an object');
		requireCondition(typeof value.id === 'string', 'cargo metadata package id is missing');
		if (!workspaceMembers.has(value.id)) {
			continue;
		}
		requireCondition(
			typeof value.name === 'string' && value.name !== '',
			'package name is missing',
		);
		requireCondition(
			typeof value.version === 'string',
			`package ${value.name} version is missing`,
		);
		packages.push({ name: value.name, version: value.version, source: value.source });
	}
	requireCondition(
		packages.length === workspaceMembers.size,
		'cargo metadata did not resolve every workspace package',
	);
	for (const packageMetadata of packages) {
		requireCondition(
			packageMetadata.source === null,
			`workspace package ${packageMetadata.name} must be local`,
		);
		requireCondition(
			packageMetadata.version === version,
			`workspace package ${packageMetadata.name} has version ${packageMetadata.version}, expected ${version}`,
		);
	}

	const packageSwift = await readFile(
		resolve(repoRoot, 'native/macos-host/Package.swift'),
		'utf8',
	);
	const declarations = [...packageSwift.matchAll(SPARKLE_DECLARATION_PATTERN)];
	requireCondition(
		declarations.length === 1,
		'Package.swift must declare exactly one exact official Sparkle dependency',
	);
	const sparkleVersion = declarations[0]?.[1];
	requireCondition(sparkleVersion !== undefined, 'Package.swift Sparkle version is missing');
	requireCondition(
		sparkleVersion === EXPECTED_SPARKLE_VERSION,
		`Package.swift Sparkle version must be ${EXPECTED_SPARKLE_VERSION}`,
	);

	let resolvedDocument: unknown;
	try {
		resolvedDocument = JSON.parse(
			await readFile(resolve(repoRoot, 'native/macos-host/Package.resolved'), 'utf8'),
		);
	} catch (error) {
		throw new ContractError(`cannot parse Package.resolved: ${String(error)}`);
	}
	requireCondition(isRecord(resolvedDocument), 'Package.resolved root must be an object');
	requireCondition(
		Array.isArray(resolvedDocument.pins),
		'Package.resolved pins must be an array',
	);
	const sparklePins = resolvedDocument.pins.filter(
		(pin: unknown): pin is Record<string, unknown> =>
			isRecord(pin) && pin.identity === 'sparkle',
	);
	requireCondition(
		sparklePins.length === 1,
		'Package.resolved must contain exactly one Sparkle pin',
	);
	const pin = sparklePins[0];
	requireCondition(pin !== undefined, 'Package.resolved Sparkle pin must be an object');
	requireCondition(
		pin.kind === 'remoteSourceControl' && pin.location === EXPECTED_SPARKLE_SOURCE,
		'Package.resolved must pin the official Sparkle repository',
	);
	requireCondition(isRecord(pin.state), 'Package.resolved Sparkle pin state is missing');
	requireCondition(
		pin.state.version === sparkleVersion,
		'Package.swift and Package.resolved disagree on Sparkle version',
	);
	requireCondition(
		pin.state.revision === EXPECTED_SPARKLE_REVISION,
		`Package.resolved Sparkle revision must be ${EXPECTED_SPARKLE_REVISION}`,
	);

	return {
		version,
		sparkleVersion,
		sparkleRevision: EXPECTED_SPARKLE_REVISION,
	};
}

async function validateGitSource(exec: Exec, args: SourceArguments): Promise<string> {
	requireCondition(
		FULL_SHA_PATTERN.test(args.eventCommit),
		'GitHub event commit must be a full lowercase commit SHA',
	);
	requireCondition(
		args.baseRef === 'refs/remotes/origin/main',
		'release base must be canonical origin/main',
	);
	const tagRef = `refs/tags/${args.tag}`;
	requireCondition(
		(await git(exec, args.repoRoot, ['cat-file', '-t', tagRef])) === 'tag',
		`${args.tag} must be an annotated tag`,
	);
	const tagObject = await git(exec, args.repoRoot, ['cat-file', '-p', tagRef]);
	const headers = tagObject.split('\n\n', 1)[0] ?? '';
	requireCondition(/^type commit$/m.test(headers), `${args.tag} must point directly to a commit`);
	requireCondition(
		new RegExp(`^tag ${args.tag.replaceAll(/[.*+?^${}()|[\]\\]/g, '\\$&')}$`, 'm').test(
			headers,
		),
		`annotated tag object name must match ${args.tag}`,
	);
	const directObjectMatch = /^object ([0-9a-f]{40})$/m.exec(headers);
	requireCondition(directObjectMatch !== null, 'annotated tag object commit is missing');

	const tagCommit = await git(exec, args.repoRoot, [
		'rev-parse',
		'--verify',
		`${tagRef}^{commit}`,
	]);
	const eventCommit = await git(exec, args.repoRoot, [
		'rev-parse',
		'--verify',
		`${args.eventCommit}^{commit}`,
	]);
	const headCommit = await git(exec, args.repoRoot, ['rev-parse', '--verify', 'HEAD^{commit}']);
	requireCondition(
		directObjectMatch[1] === tagCommit,
		'annotated tag does not point directly to its commit',
	);
	requireCondition(eventCommit === tagCommit, 'event commit does not match tag commit');
	requireCondition(headCommit === tagCommit, 'checked-out commit does not match tag commit');
	await git(exec, args.repoRoot, ['rev-parse', '--verify', `${args.baseRef}^{commit}`]);
	await checkedExec(exec, {
		command: 'git',
		args: ['merge-base', '--is-ancestor', tagCommit, args.baseRef],
		cwd: args.repoRoot,
		timeoutMs: 20_000,
	});
	return tagCommit;
}

export async function validateReleaseSource(
	args: SourceArguments,
	exec: Exec = defaultExec,
	environment: NodeJS.ProcessEnv = process.env,
): Promise<SourceValidation> {
	requireCondition(
		args.repository === CANONICAL_REPOSITORY,
		`release repository must be ${CANONICAL_REPOSITORY}`,
	);
	requireCondition(
		environment.GITHUB_REF === `refs/tags/${args.tag}`,
		'GITHUB_REF does not match the release tag',
	);
	requireCondition(
		environment.GITHUB_REPOSITORY === args.repository,
		'GITHUB_REPOSITORY does not match the validated repository',
	);
	requireCondition(
		environment.GITHUB_SHA === args.eventCommit,
		'GITHUB_SHA does not match the event commit',
	);
	const versions = await validateVersions(exec, args.repoRoot, args.tag);
	const tagCommit = await validateGitSource(exec, args);
	requireCondition(
		args.expectedCommit === undefined || args.expectedCommit === tagCommit,
		'rechecked release commit does not match the source job',
	);
	requireCondition(
		args.expectedVersion === undefined || args.expectedVersion === versions.version,
		'rechecked release version does not match the source job',
	);
	requireCondition(
		args.expectedSparkleVersion === undefined ||
			args.expectedSparkleVersion === versions.sparkleVersion,
		'rechecked Sparkle version does not match the source job',
	);
	return {
		canonicalRepository: CANONICAL_REPOSITORY,
		...versions,
		tagCommit,
	};
}

async function emitValidation(validation: SourceValidation, githubOutput?: string): Promise<void> {
	const values = {
		canonical_repository: validation.canonicalRepository,
		sparkle_revision: validation.sparkleRevision,
		sparkle_version: validation.sparkleVersion,
		tag_commit: validation.tagCommit,
		version: validation.version,
	};
	if (githubOutput === undefined) {
		process.stdout.write(`${JSON.stringify(values)}\n`);
		return;
	}
	const lines = Object.entries(values)
		.map(([name, value]) => `${name}=${value}`)
		.join('\n');
	await appendFile(githubOutput, `${lines}\n`, 'utf8');
}

export async function main(argv = process.argv.slice(2)): Promise<number> {
	try {
		const args = parseArguments(argv);
		const validation = await validateReleaseSource(args);
		await emitValidation(validation, args.githubOutput);
		return 0;
	} catch (error) {
		process.stderr.write(`error: ${error instanceof Error ? error.message : String(error)}\n`);
		return 1;
	}
}

if (isMainModule(import.meta.url)) {
	process.exitCode = await main();
}
