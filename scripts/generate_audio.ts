#!/usr/bin/env bun
/**
 * BunAmateur Pre-Generated Japanese Audio Pipeline
 *
 * Batch-generates Japanese TTS audio assets for grammar sentences and reading
 * passages using either a local VOICEVOX engine or Azure Neural Speech,
 * stores them content-addressed by SHA-256 hash of ruby-stripped plain text,
 * skips already-cached files, and writes a `manifest.json` index.
 *
 * Usage:
 *   bun run scripts/generate_audio.ts [options]
 *
 * Options:
 *   --provider <voicevox|azure>   TTS provider (default: voicevox)
 *   --out <dir>                   Output cache directory (default: ./data/audio)
 *   --api <url>                   BunAmateur API base URL (default: http://127.0.0.1:8080)
 *   --input <file.json>           Local JSON file of sentences instead of querying --api
 *   --concurrency <n>             Max concurrent synthesis requests (default: 4)
 *   --speaker <id>                VOICEVOX speaker ID (default: 3 or VOICEVOX_SPEAKER_ID)
 *   --voice <name>                Azure Neural voice (default: ja-JP-NanamiNeural or AZURE_SPEECH_VOICE)
 *   --dry-run                     Resolve inputs and hashes without calling TTS or writing files
 *   -h, --help                    Show this help message
 *
 * Environment variables:
 *   VOICEVOX_URL                  VOICEVOX engine base URL (default: http://127.0.0.1:50021)
 *   VOICEVOX_SPEAKER_ID           Default VOICEVOX speaker ID
 *   AZURE_SPEECH_KEY              Required when --provider azure (unless --dry-run)
 *   AZURE_SPEECH_REGION           Required when --provider azure (unless --dry-run)
 *   AZURE_SPEECH_VOICE            Default Azure voice name
 *   BUNAMATEUR_COOKIE             Optional Cookie header when querying an authenticated --api
 */

import { mkdir, rename, stat, unlink, writeFile, readFile } from "node:fs/promises";
import path from "node:path";

type ProviderKind = "voicevox" | "azure";

interface CliOptions {
  provider: ProviderKind;
  outDir: string;
  apiBaseUrl: string;
  inputPath?: string;
  concurrency: number;
  dryRun: boolean;
  speaker: number;
  voice: string;
  voicevoxUrl: string;
  azureKey?: string;
  azureRegion?: string;
  apiCookie?: string;
}

interface RawSentenceRecord {
  id?: number;
  sentence_id?: number;
  kind?: "sentence" | "passage";
  text?: string;
  plain_text?: string;
  jp_before?: string;
  answer?: string;
  jp_after?: string;
  content_ruby?: string;
}

interface NormalizedItem {
  id?: number;
  kind: "sentence" | "passage";
  plainText: string;
  hash: string;
}

interface ManifestEntry {
  hash: string;
  filename: string;
  mime_type: string;
  provider: string;
  voice: string;
  byte_size: number;
  plain_text: string;
  sentence_ids: number[];
}

interface AudioManifest {
  generated_at: string;
  provider: ProviderKind;
  voice: string;
  output_dir: string;
  total_items: number;
  unique_hashes: number;
  by_sentence_id: Record<string, Omit<ManifestEntry, "sentence_ids">>;
  by_hash: Record<string, ManifestEntry>;
}

const KNOWN_EXTENSIONS = ["opus", "mp3", "wav"] as const;
const AZURE_OUTPUT_FORMAT = "audio-24khz-96kbitrate-mono-mp3";

/**
 * Strips inline ruby markup (`[漢字|かんじ]` -> `漢字`).
 * Mirrors `strip_ruby_for_audio` in `src/audio.rs` exactly so hashes match 1:1.
 */
export function stripRubyForAudio(input: string): string {
  return input.replace(/\[([^[\]|]+)\|[^[\]]*\]/g, "$1");
}

/**
 * Computes the deterministic lowercase SHA-256 hex digest of trimmed,
 * ruby-stripped Japanese text using Web Crypto.
 * Mirrors `compute_audio_hash` in `src/audio.rs`.
 */
export async function computeAudioHash(plainText: string): Promise<string> {
  const normalized = stripRubyForAudio(plainText).trim();
  const encoded = new TextEncoder().encode(normalized);
  const digest = await crypto.subtle.digest("SHA-256", encoded);
  const bytes = new Uint8Array(digest);
  let hex = "";
  for (const byte of bytes) {
    hex += byte.toString(16).padStart(2, "0");
  }
  return hex;
}

function escapeXml(input: string): string {
  return input
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&apos;");
}

function buildAzureSsml(voice: string, plainText: string): string {
  return `<speak version='1.0' xml:lang='ja-JP'><voice xml:lang='ja-JP' name='${escapeXml(voice)}'>${escapeXml(plainText)}</voice></speak>`;
}

function mimeTypeForExtension(ext: string): string {
  switch (ext.toLowerCase()) {
    case "opus":
      return "audio/opus";
    case "mp3":
      return "audio/mpeg";
    case "wav":
      return "audio/wav";
    default:
      return "application/octet-stream";
  }
}

function printHelp(): void {
  console.log(`BunAmateur Sentence Audio Generator

Usage:
  bun run scripts/generate_audio.ts [options]

Options:
  --provider <voicevox|azure>   TTS provider (default: voicevox)
  --out <dir>                   Output directory (default: ./data/audio)
  --api <url>                   BunAmateur API base URL (default: http://127.0.0.1:8080)
  --input <path>                Load sentences from a local JSON file instead of --api
  --concurrency <n>             Concurrent synthesis workers (default: 4)
  --speaker <id>                VOICEVOX speaker ID (default: 3)
  --voice <name>                Azure Neural voice (default: ja-JP-NanamiNeural)
  --dry-run                     Compute hashes & report work without generating files
  -h, --help                    Show this help message
`);
}

function parseArgs(argv: string[]): CliOptions {
  let provider: ProviderKind = "voicevox";
  let outDir = "./data/audio";
  let apiBaseUrl = "http://127.0.0.1:8080";
  let inputPath: string | undefined;
  let concurrency = 4;
  let dryRun = false;
  let speaker = Number.parseInt(process.env.VOICEVOX_SPEAKER_ID ?? "3", 10);
  if (Number.isNaN(speaker) || speaker < 0) {
    speaker = 3;
  }
  let voice = process.env.AZURE_SPEECH_VOICE?.trim() || "ja-JP-NanamiNeural";

  const readValue = (index: number, flag: string): string => {
    const val = argv[index + 1];
    if (!val || val.startsWith("--")) {
      throw new Error(`Missing value for ${flag}`);
    }
    return val;
  };

  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "-h" || arg === "--help") {
      printHelp();
      process.exit(0);
    } else if (arg === "--provider") {
      const raw = readValue(i, arg).toLowerCase();
      if (raw !== "voicevox" && raw !== "azure") {
        throw new Error(`Unsupported --provider '${raw}'. Expected 'voicevox' or 'azure'.`);
      }
      provider = raw;
      i++;
    } else if (arg === "--out") {
      outDir = readValue(i, arg);
      i++;
    } else if (arg === "--api") {
      apiBaseUrl = readValue(i, arg).replace(/\/+$/, "");
      i++;
    } else if (arg === "--input") {
      inputPath = readValue(i, arg);
      i++;
    } else if (arg === "--concurrency") {
      const parsed = Number.parseInt(readValue(i, arg), 10);
      if (Number.isNaN(parsed) || parsed < 1 || parsed > 32) {
        throw new Error(`Invalid --concurrency '${argv[i + 1]}'. Must be an integer between 1 and 32.`);
      }
      concurrency = parsed;
      i++;
    } else if (arg === "--speaker") {
      const parsed = Number.parseInt(readValue(i, arg), 10);
      if (Number.isNaN(parsed) || parsed < 0) {
        throw new Error(`Invalid --speaker '${argv[i + 1]}'. Must be a non-negative integer.`);
      }
      speaker = parsed;
      i++;
    } else if (arg === "--voice") {
      voice = readValue(i, arg);
      i++;
    } else if (arg === "--dry-run") {
      dryRun = true;
    } else {
      throw new Error(`Unknown argument: ${arg}`);
    }
  }

  return {
    provider,
    outDir,
    apiBaseUrl,
    inputPath,
    concurrency,
    dryRun,
    speaker,
    voice,
    voicevoxUrl: (process.env.VOICEVOX_URL?.trim() || "http://127.0.0.1:50021").replace(/\/+$/, ""),
    azureKey: process.env.AZURE_SPEECH_KEY?.trim() || undefined,
    azureRegion: process.env.AZURE_SPEECH_REGION?.trim() || undefined,
    apiCookie: process.env.BUNAMATEUR_COOKIE?.trim() || undefined,
  };
}

function extractRawText(record: RawSentenceRecord): string {
  if (typeof record.plain_text === "string" && record.plain_text.trim().length > 0) {
    return record.plain_text;
  }
  if (typeof record.text === "string" && record.text.trim().length > 0) {
    return record.text;
  }
  if (typeof record.content_ruby === "string" && record.content_ruby.trim().length > 0) {
    return record.content_ruby;
  }
  const combined = `${record.jp_before ?? ""}${record.answer ?? ""}${record.jp_after ?? ""}`;
  return combined;
}

async function normalizeRecords(records: RawSentenceRecord[]): Promise<NormalizedItem[]> {
  const items: NormalizedItem[] = [];
  for (const record of records) {
    const raw = extractRawText(record);
    const plainText = stripRubyForAudio(raw).trim();
    if (!plainText) {
      continue;
    }
    const id =
      typeof record.sentence_id === "number"
        ? record.sentence_id
        : typeof record.id === "number"
          ? record.id
          : undefined;
    const kind = record.kind ?? (record.content_ruby ? "passage" : "sentence");
    const hash = await computeAudioHash(plainText);
    items.push({ id, kind, plainText, hash });
  }
  return items;
}

async function loadFromInputFile(inputPath: string): Promise<NormalizedItem[]> {
  const rawContent = await readFile(inputPath, "utf8");
  const parsed: unknown = JSON.parse(rawContent);

  if (Array.isArray(parsed)) {
    return normalizeRecords(parsed as RawSentenceRecord[]);
  }

  if (parsed && typeof parsed === "object") {
    const obj = parsed as { sentences?: RawSentenceRecord[]; passages?: RawSentenceRecord[] };
    const combined: RawSentenceRecord[] = [];
    if (Array.isArray(obj.sentences)) {
      combined.push(...obj.sentences.map((s) => ({ ...s, kind: s.kind ?? ("sentence" as const) })));
    }
    if (Array.isArray(obj.passages)) {
      combined.push(...obj.passages.map((p) => ({ ...p, kind: p.kind ?? ("passage" as const) })));
    }
    return normalizeRecords(combined);
  }

  throw new Error(`Unsupported JSON format in ${inputPath}: expected an array or { sentences, passages } object.`);
}

async function loadFromApi(apiBaseUrl: string, apiCookie?: string): Promise<NormalizedItem[]> {
  const headers: Record<string, string> = { Accept: "application/json" };
  if (apiCookie) {
    headers.Cookie = apiCookie;
  }

  const listResp = await fetch(`${apiBaseUrl}/api/grammar`, { headers });
  if (!listResp.ok) {
    throw new Error(
      `Failed to fetch ${apiBaseUrl}/api/grammar (HTTP ${listResp.status}). ` +
        `If the endpoint requires authentication, set BUNAMATEUR_COOKIE or pass --input sentences.json.`,
    );
  }

  const grammarList = (await listResp.json()) as Array<{ id: number }>;
  const records: RawSentenceRecord[] = [];

  for (const point of grammarList) {
    const detailResp = await fetch(`${apiBaseUrl}/api/grammar/${point.id}`, { headers });
    if (!detailResp.ok) {
      console.warn(`[warn] Skipping grammar/${point.id}: HTTP ${detailResp.status}`);
      continue;
    }
    const detail = (await detailResp.json()) as { sentences?: RawSentenceRecord[] };
    if (Array.isArray(detail.sentences)) {
      for (const sentence of detail.sentences) {
        records.push({ ...sentence, kind: "sentence" });
      }
    }
  }

  const passagesResp = await fetch(`${apiBaseUrl}/api/reading/passages`, { headers });
  if (passagesResp.ok) {
    const passages = (await passagesResp.json()) as RawSentenceRecord[];
    if (Array.isArray(passages)) {
      for (const passage of passages) {
        records.push({ ...passage, kind: "passage" });
      }
    }
  }

  return normalizeRecords(records);
}

async function findExistingCachedFile(
  outDir: string,
  hash: string,
  sentenceIds: number[],
): Promise<{ filename: string; ext: string; byteSize: number } | null> {
  const candidates: Array<{ prefix: string }> = [
    ...sentenceIds.map((id) => ({ prefix: `sentence_${id}` })),
    { prefix: hash },
  ];

  for (const { prefix } of candidates) {
    for (const ext of KNOWN_EXTENSIONS) {
      const filename = `${prefix}.${ext}`;
      try {
        const info = await stat(path.join(outDir, filename));
        if (info.isFile() && info.size > 0) {
          return { filename, ext, byteSize: info.size };
        }
      } catch {
        // Not present; continue checking remaining candidates.
      }
    }
  }

  return null;
}

async function synthesizeVoicevox(
  voicevoxUrl: string,
  speaker: number,
  plainText: string,
): Promise<{ bytes: Uint8Array; ext: string; mimeType: string; voice: string }> {
  const queryUrl = new URL(`${voicevoxUrl}/audio_query`);
  queryUrl.searchParams.set("text", plainText);
  queryUrl.searchParams.set("speaker", String(speaker));

  const queryResp = await fetch(queryUrl, { method: "POST" });
  if (!queryResp.ok) {
    const body = await queryResp.text().catch(() => "");
    throw new Error(`VOICEVOX /audio_query failed (${queryResp.status}): ${body}`);
  }
  const audioQueryJson = await queryResp.text();

  const synthUrl = new URL(`${voicevoxUrl}/synthesis`);
  synthUrl.searchParams.set("speaker", String(speaker));

  const synthResp = await fetch(synthUrl, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: audioQueryJson,
  });
  if (!synthResp.ok) {
    const body = await synthResp.text().catch(() => "");
    throw new Error(`VOICEVOX /synthesis failed (${synthResp.status}): ${body}`);
  }

  const buffer = new Uint8Array(await synthResp.arrayBuffer());
  if (buffer.byteLength === 0) {
    throw new Error("VOICEVOX /synthesis returned an empty payload");
  }

  return {
    bytes: buffer,
    ext: "wav",
    mimeType: "audio/wav",
    voice: `speaker-${speaker}`,
  };
}

async function synthesizeAzure(
  key: string,
  region: string,
  voice: string,
  plainText: string,
): Promise<{ bytes: Uint8Array; ext: string; mimeType: string; voice: string }> {
  const endpoint = `https://${region}.tts.speech.microsoft.com/cognitiveservices/v1`;
  const ssml = buildAzureSsml(voice, plainText);

  const resp = await fetch(endpoint, {
    method: "POST",
    headers: {
      "Ocp-Apim-Subscription-Key": key,
      "Content-Type": "application/ssml+xml",
      "X-Microsoft-OutputFormat": AZURE_OUTPUT_FORMAT,
      "User-Agent": "BunAmateur-AudioGenerator",
    },
    body: ssml,
  });

  if (!resp.ok) {
    const body = await resp.text().catch(() => "");
    throw new Error(`Azure Neural Speech failed (${resp.status}): ${body}`);
  }

  const buffer = new Uint8Array(await resp.arrayBuffer());
  if (buffer.byteLength === 0) {
    throw new Error("Azure Neural Speech returned an empty payload");
  }

  return {
    bytes: buffer,
    ext: "mp3",
    mimeType: "audio/mpeg",
    voice,
  };
}

async function writeAtomic(targetPath: string, data: Uint8Array | string): Promise<void> {
  const dir = path.dirname(targetPath);
  const base = path.basename(targetPath);
  const tmpPath = path.join(dir, `.${base}.${process.pid}.${Date.now()}.tmp`);
  try {
    await writeFile(tmpPath, data);
    await rename(tmpPath, targetPath);
  } catch (err) {
    await unlink(tmpPath).catch(() => {});
    throw err;
  }
}

async function runPool<T>(items: T[], concurrency: number, worker: (item: T, index: number) => Promise<void>): Promise<void> {
  let nextIndex = 0;
  const workerCount = Math.min(concurrency, items.length);

  const runners = Array.from({ length: workerCount }, async () => {
    while (true) {
      const current = nextIndex;
      nextIndex++;
      if (current >= items.length) {
        return;
      }
      await worker(items[current], current);
    }
  });

  await Promise.all(runners);
}

async function main(): Promise<void> {
  const startedAt = Date.now();
  const options = parseArgs(process.argv.slice(2));

  if (!options.dryRun && options.provider === "azure") {
    if (!options.azureKey || !options.azureRegion) {
      throw new Error(
        "AZURE_SPEECH_KEY and AZURE_SPEECH_REGION environment variables are required for --provider azure.",
      );
    }
  }

  const items = options.inputPath
    ? await loadFromInputFile(options.inputPath)
    : await loadFromApi(options.apiBaseUrl, options.apiCookie);

  // Deduplicate by content hash while preserving all associated sentence IDs.
  const uniqueMap = new Map<string, { hash: string; plainText: string; sentenceIds: number[] }>();
  for (const item of items) {
    const existing = uniqueMap.get(item.hash);
    if (existing) {
      if (item.kind === "sentence" && item.id !== undefined && !existing.sentenceIds.includes(item.id)) {
        existing.sentenceIds.push(item.id);
      }
    } else {
      uniqueMap.set(item.hash, {
        hash: item.hash,
        plainText: item.plainText,
        sentenceIds: item.kind === "sentence" && item.id !== undefined ? [item.id] : [],
      });
    }
  }

  const uniqueTasks = Array.from(uniqueMap.values());
  const voiceLabel = options.provider === "voicevox" ? `speaker-${options.speaker}` : options.voice;

  console.log(
    `[audio] Loaded ${items.length} item(s) (${uniqueTasks.length} unique hash(es)) | ` +
      `provider=${options.provider} (${voiceLabel}) | out=${options.outDir} | ` +
      `concurrency=${options.concurrency}${options.dryRun ? " | DRY RUN" : ""}`,
  );

  if (!options.dryRun) {
    await mkdir(options.outDir, { recursive: true });
  }

  let generatedCount = 0;
  let skippedCount = 0;
  let failedCount = 0;

  const byHash: Record<string, ManifestEntry> = {};
  const bySentenceId: Record<string, Omit<ManifestEntry, "sentence_ids">> = {};

  await runPool(uniqueTasks, options.concurrency, async (task, index) => {
    const progress = `[${index + 1}/${uniqueTasks.length}]`;
    const cached = await findExistingCachedFile(options.outDir, task.hash, task.sentenceIds);
    if (cached) {
      skippedCount++;
      const entry: ManifestEntry = {
        hash: task.hash,
        filename: cached.filename,
        mime_type: mimeTypeForExtension(cached.ext),
        provider: "cache",
        voice: "existing",
        byte_size: cached.byteSize,
        plain_text: task.plainText,
        sentence_ids: task.sentenceIds,
      };
      byHash[task.hash] = entry;
      for (const sid of task.sentenceIds) {
        const { sentence_ids: _ids, ...rest } = entry;
        bySentenceId[String(sid)] = rest;
      }
      console.log(`${progress} skip   ${cached.filename} (${cached.byteSize} B)`);
      return;
    }

    if (options.dryRun) {
      const ext = options.provider === "voicevox" ? "wav" : "mp3";
      const previewName = `${task.hash}.${ext}`;
      generatedCount++;
      console.log(`${progress} plan   ${previewName} <- ${task.plainText.slice(0, 48)}`);
      return;
    }

    try {
      const result =
        options.provider === "voicevox"
          ? await synthesizeVoicevox(options.voicevoxUrl, options.speaker, task.plainText)
          : await synthesizeAzure(options.azureKey!, options.azureRegion!, options.voice, task.plainText);

      const filename = `${task.hash}.${result.ext}`;
      const targetPath = path.join(options.outDir, filename);
      await writeAtomic(targetPath, result.bytes);

      generatedCount++;
      const entry: ManifestEntry = {
        hash: task.hash,
        filename,
        mime_type: result.mimeType,
        provider: options.provider,
        voice: result.voice,
        byte_size: result.bytes.byteLength,
        plain_text: task.plainText,
        sentence_ids: task.sentenceIds,
      };
      byHash[task.hash] = entry;
      for (const sid of task.sentenceIds) {
        const { sentence_ids: _ids, ...rest } = entry;
        bySentenceId[String(sid)] = rest;
      }
      console.log(`${progress} wrote  ${filename} (${result.bytes.byteLength} B)`);
    } catch (err) {
      failedCount++;
      const message = err instanceof Error ? err.message : String(err);
      console.error(`${progress} ERROR  ${task.hash.slice(0, 12)}: ${message}`);
    }
  });

  if (!options.dryRun) {
    const manifest: AudioManifest = {
      generated_at: new Date().toISOString(),
      provider: options.provider,
      voice: voiceLabel,
      output_dir: options.outDir,
      total_items: items.length,
      unique_hashes: uniqueTasks.length,
      by_sentence_id: bySentenceId,
      by_hash: byHash,
    };
    const manifestPath = path.join(options.outDir, "manifest.json");
    await writeAtomic(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
    console.log(`[audio] Wrote manifest to ${manifestPath}`);
  }

  const elapsedSec = ((Date.now() - startedAt) / 1000).toFixed(2);
  const verb = options.dryRun ? "planned" : "generated";
  console.log(
    `[audio] Done in ${elapsedSec}s: ${verb}=${generatedCount}, skipped=${skippedCount}, failed=${failedCount}`,
  );

  if (failedCount > 0) {
    process.exit(1);
  }
}

if (import.meta.main) {
  main().catch((err) => {
    console.error(`[audio] Fatal: ${err instanceof Error ? err.message : String(err)}`);
    process.exit(1);
  });
}
