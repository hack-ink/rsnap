#!/usr/bin/env node

import { createHash, timingSafeEqual } from 'node:crypto';
import { mkdtemp, readFile, readdir, rm, stat, writeFile } from 'node:fs/promises';
import { request as httpsRequest } from 'node:https';
import { basename, join, resolve } from 'node:path';
import { tmpdir } from 'node:os';

import {
	APPCAST_NAME,
	ARCHIVE_NAME,
	ASSET_NAMES,
	CANONICAL_REPOSITORY,
	CHECKSUM_NAME,
	ContractError,
	checkedExec,
	compareStableVersions,
	defaultExec,
	isMainModule,
	isRecord,
	parseStableTag,
	requireCondition,
	requiredEnvironment,
	type Exec,
	type StableVersion,
} from './release-contract.ts';

const API_VERSION = '2026-03-10';
const JSON_BODY_LIMIT = 4 * 1024 * 1024;
const ASSET_BYTE_LIMIT = 512 * 1024 * 1024;
const PAGE_SIZE = 100;
const MAX_RELEASE_PAGES = 100;
const MAX_ASSET_PAGES = 10;
const REQUEST_TIMEOUT_MS = 30_000;
const MAX_RETRY_DELAY_MS = 60_000;
const MAX_SAFE_RETRIES = 4;
const MAX_DOWNLOAD_REDIRECTS = 5;

type HttpMethod = 'GET' | 'POST' | 'PATCH' | 'DELETE';

interface ApiRequest {
	readonly method: HttpMethod;
	readonly path: string;
	readonly body?: Readonly<Record<string, unknown>>;
	readonly retrySafe?: boolean;
	readonly maxResponseBytes?: number;
}

export interface ReleaseTransport {
	requestJson(request: ApiRequest): Promise<unknown>;
	uploadAsset(path: string, source: string, contentType: string): Promise<unknown>;
	downloadAsset(path: string, destination: string, maxBytes: number): Promise<void>;
	wait(milliseconds: number): Promise<void>;
}

interface ReleaseEnvironment {
	readonly repository: string;
	readonly releaseCommit: string;
	readonly githubSha: string;
	readonly tag: string;
	readonly version: string;
	readonly sparkleVersion: string;
	readonly inputDir: string;
	readonly validator: string;
}

interface Artifact {
	readonly name: AssetName;
	readonly path: string;
	readonly size: number;
	readonly digest: string;
}

type AssetName = (typeof ASSET_NAMES)[number];

function isAssetName(value: string): value is AssetName {
	return ASSET_NAMES.some((name) => name === value);
}

interface RemoteAsset {
	readonly id: number;
	readonly name: string;
	readonly size: number;
	readonly state: string;
	readonly url: string;
	readonly browserDownloadUrl: string;
	readonly digest: string | null;
}

interface RemoteRelease {
	readonly id: number;
	readonly tagName: string;
	readonly draft: boolean;
	readonly prerelease: boolean;
}

export class HttpError extends Error {
	override readonly name = 'HttpError';
	readonly status: number;
	readonly retryAfterMs: number;

	constructor(message: string, status: number, retryAfterMs: number) {
		super(message);
		this.status = status;
		this.retryAfterMs = retryAfterMs;
	}

	get transient(): boolean {
		return this.status === 429 || (this.status >= 500 && this.status <= 599);
	}
}

function retryAfterMilliseconds(value: string | string[] | undefined): number {
	const text = Array.isArray(value) ? value[0] : value;
	if (text === undefined) {
		return 1_000;
	}
	const seconds = Number(text);
	if (Number.isFinite(seconds) && seconds >= 0) {
		return Math.min(MAX_RETRY_DELAY_MS, Math.ceil(seconds * 1_000));
	}
	const timestamp = Date.parse(text);
	if (Number.isNaN(timestamp)) {
		return 1_000;
	}
	return Math.min(MAX_RETRY_DELAY_MS, Math.max(0, timestamp - Date.now()));
}

export class GitHubTransport implements ReleaseTransport {
	readonly token: string;
	readonly userAgent: string;

	constructor(token: string, userAgent = 'rsnap-release-publisher') {
		requireCondition(token.trim() !== '', 'GitHub token is required');
		this.token = token;
		this.userAgent = userAgent;
	}

	async wait(milliseconds: number): Promise<void> {
		await new Promise<void>((resolveWait) => {
			setTimeout(resolveWait, milliseconds);
		});
	}

	private async requestBuffer(
		hostname: string,
		request: ApiRequest & {
			readonly rawBody?: Buffer;
			readonly accept?: string;
			readonly contentType?: string;
			readonly authorize?: boolean;
		},
	): Promise<{
		readonly status: number;
		readonly headers: NodeJS.Dict<string | string[]>;
		readonly body: Buffer;
	}> {
		const requestBody =
			request.rawBody ??
			(request.body === undefined
				? undefined
				: Buffer.from(JSON.stringify(request.body), 'utf8'));
		const maxResponseBytes = request.maxResponseBytes ?? JSON_BODY_LIMIT;
		return await new Promise((resolveRequest, rejectRequest) => {
			const outgoing = httpsRequest(
				{
					hostname,
					method: request.method,
					path: request.path,
					headers: {
						Accept: request.accept ?? 'application/vnd.github+json',
						...(request.authorize === false
							? {}
							: { Authorization: `Bearer ${this.token}` }),
						'User-Agent': this.userAgent,
						'X-GitHub-Api-Version': API_VERSION,
						...(requestBody === undefined
							? {}
							: {
									'Content-Length': String(requestBody.length),
									'Content-Type': request.contentType ?? 'application/json',
								}),
					},
				},
				(response) => {
					const chunks: Buffer[] = [];
					let byteCount = 0;
					response.on('data', (chunk: Buffer) => {
						byteCount += chunk.length;
						if (byteCount > maxResponseBytes) {
							outgoing.destroy(
								new ContractError(
									`GitHub response exceeded ${String(maxResponseBytes)} bytes`,
								),
							);
							return;
						}
						chunks.push(chunk);
					});
					response.on('end', () => {
						resolveRequest({
							status: response.statusCode ?? 0,
							headers: response.headers,
							body: Buffer.concat(chunks),
						});
					});
				},
			);
			outgoing.setTimeout(REQUEST_TIMEOUT_MS, () => {
				outgoing.destroy(new ContractError('GitHub request timed out'));
			});
			outgoing.on('error', rejectRequest);
			outgoing.end(requestBody);
		});
	}

	private async requestWithRetry(
		hostname: string,
		request: ApiRequest & {
			readonly rawBody?: Buffer;
			readonly accept?: string;
			readonly contentType?: string;
			readonly authorize?: boolean;
		},
	): Promise<{
		readonly status: number;
		readonly headers: NodeJS.Dict<string | string[]>;
		readonly body: Buffer;
	}> {
		for (let attempt = 0; attempt <= MAX_SAFE_RETRIES; attempt += 1) {
			let response: Awaited<ReturnType<GitHubTransport['requestBuffer']>>;
			try {
				response = await this.requestBuffer(hostname, request);
			} catch (error) {
				if (!request.retrySafe || attempt === MAX_SAFE_RETRIES) {
					throw error;
				}
				await this.wait(Math.min(MAX_RETRY_DELAY_MS, 1_000 * 2 ** attempt));
				continue;
			}
			if (response.status >= 200 && response.status < 300) {
				return response;
			}
			const retryAfterMs = retryAfterMilliseconds(response.headers['retry-after']);
			const detail = response.body.toString('utf8').slice(0, 4_096).trim();
			const error = new HttpError(
				`GitHub API ${request.method} ${request.path} returned ${String(response.status)}: ${detail}`,
				response.status,
				retryAfterMs,
			);
			if (!request.retrySafe || !error.transient || attempt === MAX_SAFE_RETRIES) {
				throw error;
			}
			await this.wait(retryAfterMs);
		}
		throw new ContractError('GitHub retry loop ended unexpectedly');
	}

	private async downloadResponse(
		hostname: string,
		path: string,
		maxResponseBytes: number,
	): Promise<Awaited<ReturnType<GitHubTransport['requestBuffer']>>> {
		for (let attempt = 0; attempt <= MAX_SAFE_RETRIES; attempt += 1) {
			try {
				const response = await this.requestBuffer(hostname, {
					method: 'GET',
					path,
					retrySafe: true,
					maxResponseBytes,
					accept: 'application/octet-stream',
					authorize: hostname === 'api.github.com',
				});
				const transient =
					response.status === 429 || (response.status >= 500 && response.status <= 599);
				if (!transient || attempt === MAX_SAFE_RETRIES) {
					return response;
				}
				await this.wait(retryAfterMilliseconds(response.headers['retry-after']));
			} catch (error) {
				if (attempt === MAX_SAFE_RETRIES) {
					throw error;
				}
				await this.wait(Math.min(MAX_RETRY_DELAY_MS, 1_000 * 2 ** attempt));
			}
		}
		throw new ContractError('asset download retry loop ended unexpectedly');
	}

	async requestJson(request: ApiRequest): Promise<unknown> {
		const { body } = await this.requestWithRetry('api.github.com', request);
		if (body.length === 0) {
			return null;
		}
		try {
			return JSON.parse(body.toString('utf8'));
		} catch (error) {
			throw new ContractError(`GitHub returned invalid JSON: ${String(error)}`);
		}
	}

	async uploadAsset(path: string, source: string, contentType: string): Promise<unknown> {
		const data = await readFile(source);
		requireCondition(
			data.length <= ASSET_BYTE_LIMIT,
			`release asset is too large: ${basename(source)}`,
		);
		const { body } = await this.requestWithRetry('uploads.github.com', {
			method: 'POST',
			path,
			rawBody: data,
			contentType,
			retrySafe: false,
			maxResponseBytes: JSON_BODY_LIMIT,
		});
		try {
			return JSON.parse(body.toString('utf8'));
		} catch (error) {
			throw new ContractError(`GitHub upload returned invalid JSON: ${String(error)}`);
		}
	}

	async downloadAsset(path: string, destination: string, maxBytes: number): Promise<void> {
		requireCondition(
			maxBytes > 0 && maxBytes <= ASSET_BYTE_LIMIT,
			'invalid asset download limit',
		);
		let hostname = 'api.github.com';
		let currentPath = path;
		for (let redirect = 0; redirect <= MAX_DOWNLOAD_REDIRECTS; redirect += 1) {
			const response = await this.downloadResponse(hostname, currentPath, maxBytes);
			if (response.status >= 200 && response.status < 300) {
				await writeFile(destination, response.body, { flag: 'wx', mode: 0o600 });
				return;
			}
			if ([301, 302, 303, 307, 308].includes(response.status)) {
				requireCondition(
					redirect < MAX_DOWNLOAD_REDIRECTS,
					'asset download redirect bound exceeded',
				);
				const locationValue = response.headers.location;
				const location = Array.isArray(locationValue) ? locationValue[0] : locationValue;
				requireCondition(
					location !== undefined && location.length <= 8_192,
					'asset redirect is invalid',
				);
				const target = new URL(location, `https://${hostname}${currentPath}`);
				requireCondition(target.protocol === 'https:', 'asset redirect must use HTTPS');
				requireCondition(
					target.hostname === 'api.github.com' ||
						target.hostname === 'github.com' ||
						target.hostname.endsWith('.githubusercontent.com'),
					'asset redirect host is not trusted',
				);
				hostname = target.hostname;
				currentPath = `${target.pathname}${target.search}`;
				continue;
			}
			const retryAfterMs = retryAfterMilliseconds(response.headers['retry-after']);
			const detail = response.body.toString('utf8').slice(0, 4_096).trim();
			throw new HttpError(
				`GitHub asset download returned ${String(response.status)}: ${detail}`,
				response.status,
				retryAfterMs,
			);
		}
		throw new ContractError('asset download redirect loop ended unexpectedly');
	}
}

function parseRelease(value: unknown): RemoteRelease {
	requireCondition(isRecord(value), 'GitHub release metadata must be an object');
	requireCondition(
		Number.isSafeInteger(value.id) && Number(value.id) > 0,
		'release ID is invalid',
	);
	requireCondition(typeof value.tag_name === 'string', 'release tag is invalid');
	requireCondition(typeof value.draft === 'boolean', 'release draft state is invalid');
	requireCondition(typeof value.prerelease === 'boolean', 'release prerelease state is invalid');
	return {
		id: Number(value.id),
		tagName: value.tag_name,
		draft: value.draft,
		prerelease: value.prerelease,
	};
}

function parseAsset(value: unknown): RemoteAsset {
	requireCondition(isRecord(value), 'GitHub release asset must be an object');
	requireCondition(Number.isSafeInteger(value.id) && Number(value.id) > 0, 'asset ID is invalid');
	requireCondition(typeof value.name === 'string' && value.name !== '', 'asset name is invalid');
	requireCondition(
		Number.isSafeInteger(value.size) &&
			Number(value.size) >= 0 &&
			Number(value.size) <= ASSET_BYTE_LIMIT,
		`asset size is invalid: ${value.name}`,
	);
	requireCondition(typeof value.state === 'string', 'asset state is invalid');
	requireCondition(typeof value.url === 'string', 'asset API URL is invalid');
	requireCondition(
		typeof value.browser_download_url === 'string',
		'asset browser download URL is invalid',
	);
	requireCondition(
		typeof value.digest === 'string' || value.digest === null,
		'asset digest is invalid',
	);
	return {
		id: Number(value.id),
		name: value.name,
		size: Number(value.size),
		state: value.state,
		url: value.url,
		browserDownloadUrl: value.browser_download_url,
		digest: value.digest,
	};
}

async function sha256File(path: string): Promise<string> {
	const bytes = await readFile(path);
	requireCondition(
		bytes.length <= ASSET_BYTE_LIMIT,
		`release asset is too large: ${basename(path)}`,
	);
	return `sha256:${createHash('sha256').update(bytes).digest('hex')}`;
}

function equalFiles(left: Buffer, right: Buffer): boolean {
	return left.length === right.length && timingSafeEqual(left, right);
}

export class Publisher {
	readonly environment: ReleaseEnvironment;
	readonly transport: ReleaseTransport;
	readonly exec: Exec;
	readonly artifacts: ReadonlyMap<string, Artifact>;
	readonly targetVersion: StableVersion;

	private constructor(
		environment: ReleaseEnvironment,
		transport: ReleaseTransport,
		exec: Exec,
		artifacts: ReadonlyMap<string, Artifact>,
	) {
		this.environment = environment;
		this.transport = transport;
		this.exec = exec;
		this.artifacts = artifacts;
		this.targetVersion = parseStableTag(environment.tag);
	}

	static async create(
		environment: ReleaseEnvironment,
		transport: ReleaseTransport,
		exec: Exec = defaultExec,
	): Promise<Publisher> {
		requireCondition(
			environment.repository === CANONICAL_REPOSITORY,
			`release repository must be ${CANONICAL_REPOSITORY}`,
		);
		requireCondition(
			environment.githubSha === environment.releaseCommit,
			'GITHUB_SHA must match RSNAP_RELEASE_COMMIT',
		);
		requireCondition(
			/^[0-9a-f]{40}$/.test(environment.releaseCommit),
			'release commit must be a full lowercase commit SHA',
		);
		requireCondition(
			parseStableTag(environment.tag).text === environment.version,
			'release tag and version do not match',
		);
		const directoryEntries = await readdir(environment.inputDir);
		for (const name of ASSET_NAMES) {
			requireCondition(
				directoryEntries.includes(name),
				`release artifact is missing: ${name}`,
			);
		}
		const artifacts = new Map<string, Artifact>();
		for (const name of ASSET_NAMES) {
			const path = resolve(environment.inputDir, name);
			const metadata = await stat(path);
			requireCondition(
				metadata.isFile() && metadata.size > 0,
				`release artifact is invalid: ${name}`,
			);
			requireCondition(
				metadata.size <= ASSET_BYTE_LIMIT,
				`release artifact is too large: ${name}`,
			);
			artifacts.set(name, {
				name,
				path,
				size: metadata.size,
				digest: await sha256File(path),
			});
		}
		return new Publisher(environment, transport, exec, artifacts);
	}

	private async validateArtifacts(artifacts: ReadonlyMap<string, Artifact>): Promise<void> {
		const archive = artifacts.get(ARCHIVE_NAME);
		const appcast = artifacts.get(APPCAST_NAME);
		const checksum = artifacts.get(CHECKSUM_NAME);
		requireCondition(
			archive !== undefined && appcast !== undefined && checksum !== undefined,
			'release artifact set is incomplete',
		);
		await checkedExec(this.exec, {
			command: this.environment.validator,
			args: [
				'--archive',
				archive.path,
				'--appcast',
				appcast.path,
				'--checksum',
				checksum.path,
				'--version',
				this.environment.version,
				'--sparkle-version',
				this.environment.sparkleVersion,
				'--tag',
				this.environment.tag,
				'--repository',
				this.environment.repository,
			],
			timeoutMs: 60_000,
			maxOutputBytes: 4 * 1024 * 1024,
		});
	}

	private async remoteSourceRecheck(): Promise<void> {
		const ref = await this.transport.requestJson({
			method: 'GET',
			path: `/repos/${this.environment.repository}/git/ref/tags/${encodeURIComponent(this.environment.tag)}`,
			retrySafe: true,
		});
		requireCondition(
			isRecord(ref) && isRecord(ref.object),
			'remote tag ref metadata is invalid',
		);
		requireCondition(
			ref.object.type === 'tag',
			'remote release ref must point to an annotated tag object',
		);
		requireCondition(
			typeof ref.object.sha === 'string' && /^[0-9a-f]{40}$/.test(ref.object.sha),
			'remote annotated tag object SHA is invalid',
		);
		const tagObject = await this.transport.requestJson({
			method: 'GET',
			path: `/repos/${this.environment.repository}/git/tags/${ref.object.sha}`,
			retrySafe: true,
		});
		requireCondition(isRecord(tagObject), 'remote annotated tag metadata is invalid');
		requireCondition(
			tagObject.tag === this.environment.tag,
			'remote annotated tag name changed',
		);
		requireCondition(isRecord(tagObject.object), 'remote annotated tag target is invalid');
		requireCondition(
			tagObject.object.type === 'commit' &&
				tagObject.object.sha === this.environment.releaseCommit,
			'remote annotated tag does not point directly to the expected commit',
		);
		const comparison = await this.transport.requestJson({
			method: 'GET',
			path: `/repos/${this.environment.repository}/compare/${this.environment.releaseCommit}...main`,
			retrySafe: true,
		});
		requireCondition(
			isRecord(comparison) &&
				isRecord(comparison.merge_base_commit) &&
				comparison.merge_base_commit.sha === this.environment.releaseCommit,
			'release commit is no longer reachable from canonical main',
		);
	}

	private async getPublishedSameTagRelease(): Promise<RemoteRelease | undefined> {
		try {
			const value = await this.transport.requestJson({
				method: 'GET',
				path: `/repos/${this.environment.repository}/releases/tags/${encodeURIComponent(this.environment.tag)}`,
				retrySafe: true,
			});
			return parseRelease(value);
		} catch (error) {
			if (error instanceof HttpError && error.status === 404) {
				return undefined;
			}
			throw error;
		}
	}

	private async listAllReleases(): Promise<RemoteRelease[]> {
		const releases: RemoteRelease[] = [];
		for (let page = 1; page <= MAX_RELEASE_PAGES; page += 1) {
			const value = await this.transport.requestJson({
				method: 'GET',
				path: `/repos/${this.environment.repository}/releases?per_page=${String(PAGE_SIZE)}&page=${String(page)}`,
				retrySafe: true,
			});
			requireCondition(Array.isArray(value), 'GitHub releases page must be an array');
			releases.push(...value.map(parseRelease));
			if (value.length < PAGE_SIZE) {
				return releases;
			}
		}
		throw new ContractError('GitHub release pagination exceeded its bound');
	}

	private async getSameTagRelease(): Promise<RemoteRelease | undefined> {
		const matching = (await this.listAllReleases()).filter(
			(release) => release.tagName === this.environment.tag,
		);
		requireCondition(
			matching.length <= 1,
			`multiple GitHub releases use tag ${this.environment.tag}`,
		);
		return matching[0];
	}

	private async validateAllStableReleases(allowSameTag: boolean): Promise<void> {
		for (const release of await this.listAllReleases()) {
			if (release.draft || release.prerelease) {
				continue;
			}
			let version: StableVersion;
			try {
				version = parseStableTag(release.tagName);
			} catch {
				continue;
			}
			const comparison = compareStableVersions(this.targetVersion, version);
			const isSameTag = release.tagName === this.environment.tag;
			requireCondition(
				comparison > 0 || (allowSameTag && isSameTag && comparison === 0),
				`release version ${this.environment.version} must be higher than every published stable release`,
			);
		}
	}

	private async createOrRecoverDraft(): Promise<RemoteRelease> {
		try {
			const value = await this.transport.requestJson({
				method: 'POST',
				path: `/repos/${this.environment.repository}/releases`,
				body: {
					tag_name: this.environment.tag,
					target_commitish: this.environment.releaseCommit,
					name: `Rsnap ${this.environment.tag}`,
					draft: true,
					prerelease: false,
					generate_release_notes: true,
				},
				retrySafe: false,
			});
			return parseRelease(value);
		} catch (error) {
			if (error instanceof HttpError && !error.transient) {
				throw error;
			}
			await this.transport.wait(error instanceof HttpError ? error.retryAfterMs : 1_000);
			const recovered = await this.getSameTagRelease();
			requireCondition(
				recovered !== undefined,
				'draft creation failed without a recoverable release',
			);
			return recovered;
		}
	}

	private async listAllAssets(releaseId: number): Promise<RemoteAsset[]> {
		const assets: RemoteAsset[] = [];
		for (let page = 1; page <= MAX_ASSET_PAGES; page += 1) {
			const value = await this.transport.requestJson({
				method: 'GET',
				path: `/repos/${this.environment.repository}/releases/${String(releaseId)}/assets?per_page=${String(PAGE_SIZE)}&page=${String(page)}`,
				retrySafe: true,
			});
			requireCondition(Array.isArray(value), 'GitHub assets page must be an array');
			assets.push(...value.map(parseAsset));
			if (value.length < PAGE_SIZE) {
				return assets;
			}
		}
		throw new ContractError('GitHub asset pagination exceeded its bound');
	}

	private async deleteAllAssets(releaseId: number): Promise<void> {
		for (const asset of await this.listAllAssets(releaseId)) {
			try {
				await this.transport.requestJson({
					method: 'DELETE',
					path: `/repos/${this.environment.repository}/releases/assets/${String(asset.id)}`,
					retrySafe: false,
				});
			} catch (error) {
				if (error instanceof HttpError && error.status === 404) {
					continue;
				}
				if (error instanceof HttpError && !error.transient) {
					throw error;
				}
				const remaining = await this.listAllAssets(releaseId);
				requireCondition(
					remaining.every((candidate) => candidate.id !== asset.id),
					`asset deletion had an unknown result: ${asset.name}`,
				);
			}
		}
		requireCondition(
			(await this.listAllAssets(releaseId)).length === 0,
			'draft assets remain after delete-all',
		);
	}

	private async uploadArtifact(releaseId: number, artifact: Artifact): Promise<void> {
		const path =
			`/repos/${this.environment.repository}/releases/${String(releaseId)}/assets?name=` +
			encodeURIComponent(artifact.name);
		for (let attempt = 0; attempt < 2; attempt += 1) {
			try {
				await this.transport.uploadAsset(path, artifact.path, 'application/octet-stream');
				return;
			} catch (error) {
				if (error instanceof HttpError && !error.transient) {
					throw error;
				}
				await this.transport.wait(error instanceof HttpError ? error.retryAfterMs : 1_000);
				const matching = (await this.listAllAssets(releaseId)).filter(
					(asset) => asset.name === artifact.name,
				);
				if (
					matching.length === 1 &&
					matching[0]?.size === artifact.size &&
					matching[0]?.digest === artifact.digest &&
					matching[0]?.state === 'uploaded'
				) {
					return;
				}
				requireCondition(attempt === 0, `asset upload did not converge: ${artifact.name}`);
				for (const asset of matching) {
					try {
						await this.transport.requestJson({
							method: 'DELETE',
							path: `/repos/${this.environment.repository}/releases/assets/${String(asset.id)}`,
							retrySafe: false,
						});
					} catch (deleteError) {
						if (deleteError instanceof HttpError && deleteError.status === 404) {
							continue;
						}
						if (deleteError instanceof HttpError && !deleteError.transient) {
							throw deleteError;
						}
						const remaining = await this.listAllAssets(releaseId);
						requireCondition(
							remaining.every((candidate) => candidate.id !== asset.id),
							`conflicting asset deletion had an unknown result: ${artifact.name}`,
						);
					}
				}
			}
		}
		throw new ContractError(`asset upload retry ended unexpectedly: ${artifact.name}`);
	}

	private validateAssetMetadata(
		assets: readonly RemoteAsset[],
		expected: ReadonlyMap<string, Artifact> | undefined,
		publicRelease: boolean,
	): void {
		requireCondition(
			assets.length === ASSET_NAMES.length,
			'release must contain exactly three assets',
		);
		const names = new Set<string>();
		for (const asset of assets) {
			requireCondition(!names.has(asset.name), `duplicate release asset: ${asset.name}`);
			names.add(asset.name);
			requireCondition(isAssetName(asset.name), `unexpected release asset: ${asset.name}`);
			requireCondition(
				asset.state === 'uploaded',
				`release asset is not uploaded: ${asset.name}`,
			);
			requireCondition(
				asset.url ===
					`https://api.github.com/repos/${this.environment.repository}/releases/assets/${String(asset.id)}`,
				`release asset API URL is not canonical: ${asset.name}`,
			);
			const publicUrl =
				`https://github.com/${this.environment.repository}/releases/download/` +
				`${this.environment.tag}/${encodeURIComponent(asset.name)}`;
			if (publicRelease) {
				requireCondition(
					asset.browserDownloadUrl === publicUrl,
					`published asset URL is not canonical: ${asset.name}`,
				);
			} else {
				let draftUrl: URL;
				try {
					draftUrl = new URL(asset.browserDownloadUrl);
				} catch {
					throw new ContractError(`draft asset URL is invalid: ${asset.name}`);
				}
				const pathPrefix = `/${this.environment.repository}/releases/download/`;
				const pathSuffix = `/${encodeURIComponent(asset.name)}`;
				const slug = draftUrl.pathname.slice(
					pathPrefix.length,
					draftUrl.pathname.length - pathSuffix.length,
				);
				requireCondition(
					draftUrl.protocol === 'https:' &&
						draftUrl.hostname === 'github.com' &&
						draftUrl.search === '' &&
						draftUrl.hash === '' &&
						draftUrl.pathname.startsWith(pathPrefix) &&
						draftUrl.pathname.endsWith(pathSuffix) &&
						(slug === this.environment.tag ||
							/^untagged-[A-Za-z0-9][A-Za-z0-9._-]*$/.test(slug)),
					`draft asset URL is not canonical: ${asset.name}`,
				);
			}
			requireCondition(
				typeof asset.digest === 'string' && /^sha256:[0-9a-f]{64}$/.test(asset.digest),
				`release asset digest is invalid: ${asset.name}`,
			);
			const local = expected?.get(asset.name);
			if (local !== undefined) {
				requireCondition(
					asset.size === local.size,
					`release asset size changed: ${asset.name}`,
				);
				requireCondition(
					asset.digest === local.digest,
					`release asset digest changed: ${asset.name}`,
				);
			}
		}
		requireCondition(
			ASSET_NAMES.every((name) => names.has(name)),
			'release asset triplet is incomplete',
		);
	}

	private async downloadAndValidate(
		releaseId: number,
		publicRelease: boolean,
		compareLocal: boolean,
	): Promise<void> {
		const assets = await this.listAllAssets(releaseId);
		this.validateAssetMetadata(
			assets,
			compareLocal ? this.artifacts : undefined,
			publicRelease,
		);
		const directory = await mkdtemp(join(tmpdir(), 'rsnap-release-download-'));
		try {
			const downloaded = new Map<string, Artifact>();
			for (const asset of assets) {
				const assetName = asset.name;
				requireCondition(isAssetName(assetName), `unexpected release asset: ${assetName}`);
				const destination = join(directory, assetName);
				await this.transport.downloadAsset(
					`/repos/${this.environment.repository}/releases/assets/${String(asset.id)}`,
					destination,
					asset.size,
				);
				const metadata = await stat(destination);
				requireCondition(
					metadata.size === asset.size,
					`downloaded asset size changed: ${asset.name}`,
				);
				const digest = await sha256File(destination);
				requireCondition(
					digest === asset.digest,
					`downloaded asset digest changed: ${asset.name}`,
				);
				const local = this.artifacts.get(assetName);
				if (compareLocal) {
					requireCondition(
						local !== undefined,
						`local artifact is missing: ${asset.name}`,
					);
					const [localBytes, remoteBytes] = await Promise.all([
						readFile(local.path),
						readFile(destination),
					]);
					requireCondition(
						equalFiles(localBytes, remoteBytes),
						`downloaded release bytes do not match local artifact: ${asset.name}`,
					);
				}
				downloaded.set(assetName, {
					name: assetName,
					path: destination,
					size: asset.size,
					digest,
				});
			}
			await this.validateArtifacts(downloaded);
		} finally {
			await rm(directory, { recursive: true, force: true });
		}
	}

	private validateReleaseState(
		release: RemoteRelease,
		expectedDraft: boolean,
		expectedId?: number,
	): void {
		requireCondition(release.tagName === this.environment.tag, 'release tag does not match');
		requireCondition(release.draft === expectedDraft, 'release draft state does not match');
		requireCondition(!release.prerelease, 'stable release must not be a prerelease');
		requireCondition(
			expectedId === undefined || release.id === expectedId,
			'release ID changed',
		);
	}

	async publish(): Promise<'published' | 'already-public'> {
		await this.validateArtifacts(this.artifacts);
		await this.remoteSourceRecheck();

		const publishedRetry = await this.getPublishedSameTagRelease();
		if (publishedRetry !== undefined) {
			this.validateReleaseState(publishedRetry, false);
			const sameTag = await this.getSameTagRelease();
			requireCondition(sameTag !== undefined, 'published release disappeared');
			this.validateReleaseState(sameTag, false, publishedRetry.id);
			await this.downloadAndValidate(sameTag.id, true, false);
			return 'already-public';
		}

		let release = await this.getSameTagRelease();
		if (release !== undefined && !release.draft) {
			this.validateReleaseState(release, false);
			await this.downloadAndValidate(release.id, true, false);
			return 'already-public';
		}

		await this.validateAllStableReleases(false);
		if (release === undefined) {
			release = await this.createOrRecoverDraft();
		}
		this.validateReleaseState(release, true);
		await this.deleteAllAssets(release.id);
		for (const name of ASSET_NAMES) {
			const artifact = this.artifacts.get(name);
			requireCondition(artifact !== undefined, `local artifact is missing: ${name}`);
			await this.uploadArtifact(release.id, artifact);
		}

		await this.remoteSourceRecheck();
		await this.validateAllStableReleases(false);
		const finalDraft = await this.getSameTagRelease();
		requireCondition(finalDraft !== undefined, 'draft release disappeared before publication');
		this.validateReleaseState(finalDraft, true, release.id);
		await this.downloadAndValidate(release.id, false, true);
		let publishedValue: unknown;
		try {
			publishedValue = await this.transport.requestJson({
				method: 'PATCH',
				path: `/repos/${this.environment.repository}/releases/${String(release.id)}`,
				body: { draft: false, make_latest: 'true' },
				retrySafe: false,
			});
		} catch (error) {
			if (error instanceof HttpError && !error.transient) {
				throw error;
			}
			const finalState = await this.getSameTagRelease();
			requireCondition(
				finalState !== undefined,
				'publication result is unknown and release disappeared',
			);
			if (!finalState.draft) {
				this.validateReleaseState(finalState, false, release.id);
				await this.downloadAndValidate(finalState.id, true, true);
				return 'published';
			}
			this.validateReleaseState(finalState, true, release.id);
			throw new ContractError(
				'publication result is unknown; verified release remains a draft',
			);
		}
		this.validateReleaseState(parseRelease(publishedValue), false, release.id);
		const publicRelease = await this.getPublishedSameTagRelease();
		requireCondition(
			publicRelease !== undefined,
			'published release disappeared after publication',
		);
		this.validateReleaseState(publicRelease, false, release.id);
		await this.downloadAndValidate(publicRelease.id, true, true);
		return 'published';
	}

	async dryRun(): Promise<void> {
		await this.validateArtifacts(this.artifacts);
	}
}

function environmentFromProcess(environment: NodeJS.ProcessEnv): ReleaseEnvironment {
	return {
		repository: requiredEnvironment(environment, 'GITHUB_REPOSITORY'),
		releaseCommit: requiredEnvironment(environment, 'RSNAP_RELEASE_COMMIT'),
		githubSha: requiredEnvironment(environment, 'GITHUB_SHA'),
		tag: requiredEnvironment(environment, 'RSNAP_RELEASE_TAG'),
		version: requiredEnvironment(environment, 'RSNAP_RELEASE_VERSION'),
		sparkleVersion: requiredEnvironment(environment, 'RSNAP_SPARKLE_VERSION'),
		inputDir: resolve(requiredEnvironment(environment, 'RSNAP_RELEASE_INPUT_DIR')),
		validator: resolve(
			environment.RSNAP_RELEASE_VALIDATOR?.trim() ||
				resolve(import.meta.dirname, 'validate-release-artifacts.py'),
		),
	};
}

export async function main(argv = process.argv.slice(2)): Promise<number> {
	try {
		requireCondition(
			argv.length === 0 || (argv.length === 1 && argv[0] === '--dry-run'),
			'usage: node scripts/release/publish-github-release.ts [--dry-run]',
		);
		const dryRun = argv[0] === '--dry-run';
		const environment = environmentFromProcess(process.env);
		const transport = dryRun
			? ({
					requestJson: async () => {
						throw new ContractError('dry-run must not use GitHub transport');
					},
					uploadAsset: async () => {
						throw new ContractError('dry-run must not upload');
					},
					downloadAsset: async () => {
						throw new ContractError('dry-run must not download');
					},
					wait: async () => {},
				} satisfies ReleaseTransport)
			: new GitHubTransport(requiredEnvironment(process.env, 'GH_TOKEN'));
		const publisher = await Publisher.create(environment, transport);
		if (dryRun) {
			await publisher.dryRun();
			process.stdout.write(
				`Validated credential-free release dry-run for ${environment.tag}.\n`,
			);
			return 0;
		}
		const result = await publisher.publish();
		process.stdout.write(
			result === 'published'
				? `Published ${environment.tag} as the latest stable GitHub release.\n`
				: `GitHub release ${environment.tag} is already public and valid.\n`,
		);
		return 0;
	} catch (error) {
		process.stderr.write(`error: ${error instanceof Error ? error.message : String(error)}\n`);
		return 1;
	}
}

if (isMainModule(import.meta.url)) {
	process.exitCode = await main();
}
