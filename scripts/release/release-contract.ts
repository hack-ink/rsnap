import { spawn } from 'node:child_process';
import { pathToFileURL } from 'node:url';

export const CANONICAL_REPOSITORY = 'acg-box/rsnap';
export const ARCHIVE_NAME = 'rsnap-aarch64-apple-darwin.zip';
export const APPCAST_NAME = 'appcast.xml';
export const CHECKSUM_NAME = `${ARCHIVE_NAME}.sha256`;
export const ASSET_NAMES = [ARCHIVE_NAME, APPCAST_NAME, CHECKSUM_NAME] as const;
export const EXPECTED_APPLE_TEAM_ID = 'RD3D4LH465';
export const EXPECTED_SPARKLE_VERSION = '2.9.4';
export const EXPECTED_SPARKLE_SOURCE = 'https://github.com/sparkle-project/Sparkle';
export const EXPECTED_SPARKLE_REVISION = 'b6496a74a087257ef5e6da1c5b29a447a60f5bd7';

const STABLE_VERSION_PATTERN = /^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/;
const MAX_VERSION_COMPONENT_DIGITS = 128;

export class ContractError extends Error {
	override readonly name = 'ContractError';
}

export interface StableVersion {
	readonly text: string;
	readonly parts: readonly [bigint, bigint, bigint];
}

export interface ExecRequest {
	readonly command: string;
	readonly args: readonly string[];
	readonly cwd?: string;
	readonly env?: NodeJS.ProcessEnv;
	readonly input?: string | Uint8Array;
	readonly timeoutMs?: number;
	readonly maxOutputBytes?: number;
}

export interface ExecResult {
	readonly status: number;
	readonly stdout: Buffer;
	readonly stderr: Buffer;
}

export type Exec = (request: ExecRequest) => Promise<ExecResult>;

export function requireCondition(condition: unknown, message: string): asserts condition {
	if (!condition) {
		throw new ContractError(message);
	}
}

export function requiredEnvironment(environment: NodeJS.ProcessEnv, name: string): string {
	const value = environment[name]?.trim() ?? '';
	requireCondition(value !== '', `missing required environment variable: ${name}`);
	return value;
}

export function parseStableVersion(value: string, label = 'version'): StableVersion {
	const match = STABLE_VERSION_PATTERN.exec(value);
	requireCondition(match !== null, `${label} must be stable X.Y.Z SemVer without leading zeroes`);
	const components = match.slice(1);
	requireCondition(
		components.every((component) => component.length <= MAX_VERSION_COMPONENT_DIGITS),
		`${label} contains an oversized numeric component`,
	);
	const major = components[0];
	const minor = components[1];
	const patch = components[2];
	requireCondition(
		major !== undefined && minor !== undefined && patch !== undefined,
		`${label} is incomplete`,
	);
	return {
		text: value,
		parts: [BigInt(major), BigInt(minor), BigInt(patch)],
	};
}

export function parseStableTag(tag: string): StableVersion {
	requireCondition(tag.startsWith('v'), 'release tag must start with v');
	return parseStableVersion(tag.slice(1), 'release tag');
}

export function compareStableVersions(left: StableVersion, right: StableVersion): number {
	for (let index = 0; index < left.parts.length; index += 1) {
		const leftPart = left.parts[index];
		const rightPart = right.parts[index];
		requireCondition(leftPart !== undefined && rightPart !== undefined, 'invalid SemVer tuple');
		if (leftPart < rightPart) {
			return -1;
		}
		if (leftPart > rightPart) {
			return 1;
		}
	}
	return 0;
}

export function isRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === 'object' && value !== null && !Array.isArray(value);
}

export function isMainModule(moduleUrl: string, argv = process.argv): boolean {
	const entrypoint = argv[1];
	return entrypoint !== undefined && pathToFileURL(entrypoint).href === moduleUrl;
}

export const defaultExec: Exec = async (request) => {
	const timeoutMs = request.timeoutMs ?? 30_000;
	const maxOutputBytes = request.maxOutputBytes ?? 4 * 1024 * 1024;
	requireCondition(timeoutMs > 0, 'command timeout must be positive');
	requireCondition(maxOutputBytes > 0, 'command output limit must be positive');

	return await new Promise<ExecResult>((resolve, reject) => {
		const child = spawn(request.command, [...request.args], {
			cwd: request.cwd,
			env: request.env,
			stdio: ['pipe', 'pipe', 'pipe'],
		});
		const stdout: Buffer[] = [];
		const stderr: Buffer[] = [];
		let outputBytes = 0;
		let settled = false;

		const finish = (error?: Error, result?: ExecResult): void => {
			if (settled) {
				return;
			}
			settled = true;
			clearTimeout(timer);
			if (error !== undefined) {
				reject(error);
			} else if (result !== undefined) {
				resolve(result);
			}
		};

		const collect = (chunks: Buffer[], chunk: Buffer): void => {
			outputBytes += chunk.length;
			if (outputBytes > maxOutputBytes) {
				child.kill('SIGKILL');
				finish(
					new ContractError(`command output exceeded ${String(maxOutputBytes)} bytes`),
				);
				return;
			}
			chunks.push(chunk);
		};

		child.stdout.on('data', (chunk: Buffer) => {
			collect(stdout, chunk);
		});
		child.stderr.on('data', (chunk: Buffer) => {
			collect(stderr, chunk);
		});
		child.on('error', (error) => {
			finish(error);
		});
		child.on('close', (status) => {
			finish(undefined, {
				status: status ?? 1,
				stdout: Buffer.concat(stdout),
				stderr: Buffer.concat(stderr),
			});
		});

		const timer = setTimeout(() => {
			child.kill('SIGKILL');
			finish(new ContractError(`command timed out after ${String(timeoutMs)} ms`));
		}, timeoutMs);
		timer.unref();

		if (request.input === undefined) {
			child.stdin.end();
		} else {
			child.stdin.end(request.input);
		}
	});
};

export async function checkedExec(exec: Exec, request: ExecRequest): Promise<ExecResult> {
	const result = await exec(request);
	if (result.status !== 0) {
		const detail =
			result.stderr.toString('utf8').trim() || result.stdout.toString('utf8').trim();
		throw new ContractError(
			`${request.command} ${request.args.join(' ')} failed: ${detail || 'unknown error'}`,
		);
	}
	return result;
}
