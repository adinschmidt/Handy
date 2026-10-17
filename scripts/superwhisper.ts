#!/usr/bin/env bun
import { parseArgs } from "node:util";
import { open, mkdtemp, rm, readFile } from "node:fs/promises";
import { tmpdir, homedir } from "node:os";
import { join, resolve, parse, basename } from "node:path";

const API = "https://api.superwhisper.com";
const USER_AGENT =
  "superwhisper/2.16.6 (com.superduper.superwhisper; build:2.16.6; macOS 26.5.2) Alamofire/5.8.0";
const HELP = `Usage: bun scripts/superwhisper.ts [options] <audio-or-video-file>

  -s, --subtitles          Output SRT instead of plain text
  -m, --model MODEL        scribe (default) or s1-voice
  -d, --diarize            Request speaker labels, preserved as speaker_0, etc.
      --tag-audio-events   Request sound labels such as (laughter)
      --no-tag-audio-events  Disable sound labels explicitly
  -l, --language CODE      Language hint, default auto
  -o, --output FILE        Override output path, or - for stdout
      --save-json FILE    Also save the original response to a new file
      --handy-config FILE  Override Handy settings path, including portable installs
      --from-json         Render a saved response without uploading audio
      --no-convert        Upload as-is instead of extracting with ffmpeg
                          (otherwise audio over 14 minutes is split at
                          silences, since Superwhisper's limit is 15)
  -h, --help              Show help

Subtitles default to <input-basename>.srt beside the input; text uses stdout.
Requires Bun, ffmpeg and ffprobe unless --no-convert or --from-json is used.
SW_X_ID, SW_X_LICENSE, SW_X_SIGNATURE override saved Handy credentials.
Missing values are read from Handy settings without changing the file.
Bun's --env-file=/path/to/env can load credentials from a private file.
Scribe revision is chosen by Superwhisper. S1 supports plain text only here.
Audio is sent to Superwhisper and may consume account usage.
Existing output files are never overwritten.
`;

export function options(args: string[]) {
  const { values, positionals } = parseArgs({
    args,
    allowPositionals: true,
    strict: true,
    options: {
      help: { type: "boolean", short: "h" },
      subtitles: { type: "boolean", short: "s" },
      model: { type: "string", short: "m", default: "scribe" },
      diarize: { type: "boolean", short: "d" },
      "tag-audio-events": { type: "boolean" },
      "no-tag-audio-events": { type: "boolean" },
      language: { type: "string", short: "l", default: "auto" },
      output: { type: "string", short: "o" },
      "save-json": { type: "string" },
      "from-json": { type: "boolean" },
      "handy-config": { type: "string" },
      "no-convert": { type: "boolean" },
    },
  });
  if (values.help) return { ...values, input: "" };
  if (positionals.length !== 1)
    throw new Error("Expected exactly one input file. Use --help for usage.");
  if (!["scribe", "s1-voice"].includes(values.model))
    throw new Error(
      "--model must be scribe or s1-voice. The proxy chooses the Scribe revision.",
    );
  if (values["tag-audio-events"] && values["no-tag-audio-events"])
    throw new Error("Choose only one audio-event flag.");
  if (
    !values["from-json"] &&
    values.model === "s1-voice" &&
    (values.subtitles ||
      values.diarize ||
      values["tag-audio-events"] ||
      values["no-tag-audio-events"])
  )
    throw new Error(
      "Subtitles, diarization, and audio-event options require --model scribe.",
    );
  const input = positionals[0]!;
  const parsed = parse(input);
  const output =
    values.output === "-"
      ? undefined
      : (values.output ??
        (values.subtitles
          ? join(parsed.dir, `${parsed.name}.srt`)
          : undefined));
  return { ...values, output, input };
}
type Options = ReturnType<typeof options>;
type Word = { text: string; start: number; end: number; speaker?: string };
function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
function transcript(value: unknown) {
  if (!record(value) || typeof value.text !== "string")
    throw new Error("Invalid transcription response: missing text.");
  return value;
}
function words(value: Record<string, unknown>): Word[] {
  if (!Array.isArray(value.words))
    throw new Error(
      "Response has no word timestamps. Cannot render subtitles or speaker labels.",
    );
  return value.words.flatMap((word: unknown) => {
    if (!record(word) || typeof word.text !== "string")
      throw new Error("Invalid word in transcription response.");
    if (word.type === "spacing" || !word.text.trim()) return [];
    if (
      typeof word.start !== "number" ||
      typeof word.end !== "number" ||
      !Number.isFinite(word.start) ||
      !Number.isFinite(word.end) ||
      word.start < 0 ||
      word.end < word.start
    )
      throw new Error("Missing or invalid word timestamps.");
    if (word.speaker_id != null && typeof word.speaker_id !== "string")
      throw new Error("Invalid speaker label.");
    return [
      {
        text: word.text.trim(),
        start: word.start,
        end: word.end,
        speaker:
          typeof word.speaker_id === "string" ? word.speaker_id : undefined,
      },
    ];
  });
}
export function timestamp(seconds: number) {
  const ms = Math.round(seconds * 1000);
  const pad = (n: number, size = 2) => String(n).padStart(size, "0");
  return `${pad(Math.floor(ms / 3600000))}:${pad(Math.floor(ms / 60000) % 60)}:${pad(Math.floor(ms / 1000) % 60)},${pad(ms % 1000, 3)}`;
}
export function render(value: unknown, subtitles: boolean, diarize: boolean) {
  const response = transcript(value);
  if (!subtitles && !diarize) return `${response.text}\n`;
  const offset =
    typeof response._audio_start_seconds === "number" &&
    Number.isFinite(response._audio_start_seconds)
      ? Math.max(0, response._audio_start_seconds)
      : 0;
  const tokens = words(response).map((word) => ({
    ...word,
    start: word.start + offset,
    end: word.end + offset,
  }));
  if (!tokens.length && String(response.text).trim())
    throw new Error("Response contains text but no timed words.");
  if (diarize && tokens.length && !tokens.some((word) => word.speaker))
    throw new Error(
      "Diarization requested, but the response contains no speaker labels. Use --save-json to retain the response.",
    );
  const segments: Word[] = [];
  for (const word of tokens) {
    const last = segments.at(-1);
    const speaker = diarize ? word.speaker : undefined;
    if (
      !last ||
      speaker !== last.speaker ||
      (subtitles &&
        (word.start - last.end > 0.8 ||
          word.end - last.start > 6 ||
          last.text.length + word.text.length > 80 ||
          /[.!?…]["')\]]?$/.test(last.text)))
    ) {
      segments.push({ ...word, speaker });
    } else {
      last.text += /^[,.;:!?]/.test(word.text) ? word.text : ` ${word.text}`;
      last.end = Math.max(last.end, word.end);
    }
  }
  return (
    segments
      .map((segment, i) => {
        const text = `${segment.speaker ? `${segment.speaker}: ` : ""}${segment.text}`;
        if (!subtitles) return text;
        // SRT cues must have a positive duration, including rounded zero-length words.
        const end = Math.max(
          segment.end,
          Math.round(segment.start * 1000) / 1000 + 0.001,
        );
        return `${i + 1}\n${timestamp(segment.start)} --> ${timestamp(end)}\n${text}\n`;
      })
      .join("\n") + (segments.length ? "\n" : "")
  );
}

type Log = (message: string) => void;
// Status goes to stderr so plain-text transcripts on stdout stay pipeable.
// Bun's console.error paints everything red on a terminal, so write directly.
const tty = Boolean(process.stderr.isTTY);
let drawn = "";
function clearBar() {
  if (drawn) process.stderr.write("\r\x1b[K");
  drawn = "";
}
const status: Log = (message) => {
  clearBar();
  process.stderr.write(`superwhisper: ${message}\n`);
};
function bar(fraction: number, label: string) {
  if (!tty) return;
  const width = 24;
  const filled = Math.round(Math.min(1, Math.max(0, fraction)) * width);
  const line = `superwhisper: [${"█".repeat(filled)}${"░".repeat(width - filled)}] ${label}`;
  if (line === drawn) return;
  process.stderr.write(`\r\x1b[K${line}`);
  drawn = line;
}
export function megabytes(bytes: number) {
  return `${(bytes / 1e6).toFixed(1)} MB`;
}
export function duration(seconds: number) {
  const s = Math.round(seconds);
  const pad = (n: number) => String(n).padStart(2, "0");
  return s >= 3600
    ? `${Math.floor(s / 3600)}:${pad(Math.floor(s / 60) % 60)}:${pad(s % 60)}`
    : `${Math.floor(s / 60)}:${pad(s % 60)}`;
}
function since(start: number) {
  const seconds = (performance.now() - start) / 1000;
  return seconds < 60 ? `${seconds.toFixed(1)}s` : duration(seconds);
}

class RequestError extends Error {
  constructor(
    message: string,
    readonly retryable: boolean,
  ) {
    super(message);
  }
}
async function jsonRequest(
  url: string | URL,
  init: RequestInit = {},
): Promise<unknown> {
  let response: Response;
  const timeout = AbortSignal.timeout(30 * 60 * 1000);
  try {
    response = await fetch(url, {
      ...init,
      redirect: "error",
      signal: init.signal ? AbortSignal.any([init.signal, timeout]) : timeout,
    });
  } catch {
    throw new RequestError(
      "Superwhisper request failed or timed out.",
      !init.signal?.aborted,
    );
  }
  if (!response.ok) {
    const detail = (await response.text().catch(() => "")).trim();
    throw new RequestError(
      `Superwhisper returned HTTP ${response.status}${detail && detail.length < 200 ? `: ${detail}` : "."}${[401, 403].includes(response.status) ? " Refresh your Superwhisper credentials." : ""}`,
      response.status === 429 || response.status >= 500,
    );
  }
  try {
    return await response.json();
  } catch {
    throw new Error("Superwhisper returned invalid JSON.");
  }
}
const CREDENTIAL_KEYS = [
  ["SW_X_ID", "superwhisper_x_id"],
  ["SW_X_LICENSE", "superwhisper_x_license"],
  ["SW_X_SIGNATURE", "superwhisper_x_signature"],
] as const;

export function handyConfigPath(
  env: Record<string, string | undefined>,
  platform = process.platform,
  home = homedir(),
) {
  const base =
    platform === "darwin"
      ? join(home, "Library", "Application Support")
      : platform === "win32"
        ? env.APPDATA || join(home, "AppData", "Roaming")
        : env.XDG_DATA_HOME || join(home, ".local", "share");
  return join(base, "com.pais.handy", "settings_store.json");
}

export async function loadCredentials(
  env: Record<string, string | undefined>,
  path = handyConfigPath(env),
) {
  const resolved = { ...env };
  if (CREDENTIAL_KEYS.every(([key]) => env[key]?.trim()))
    return credentials(resolved);
  let content: string;
  try {
    content = await readFile(path, "utf8");
  } catch (error: unknown) {
    if (record(error) && error.code === "ENOENT") return credentials(resolved);
    throw new Error(
      "Could not read Handy settings. Check permissions or use --handy-config.",
    );
  }
  let store: unknown;
  try {
    store = JSON.parse(content);
  } catch {
    throw new Error(
      "Handy settings contain invalid JSON. Set all three SW_X_* variables or use --handy-config.",
    );
  }
  const settings =
    record(store) && record(store.settings) ? store.settings : undefined;
  const keys =
    settings && record(settings.transcription_api_keys)
      ? settings.transcription_api_keys
      : undefined;
  for (const [envKey, configKey] of CREDENTIAL_KEYS) {
    if (!env[envKey]?.trim() && typeof keys?.[configKey] === "string")
      resolved[envKey] = keys[configKey];
  }
  return credentials(resolved);
}

export function credentials(env: Record<string, string | undefined>) {
  const headers = new Headers({
    "User-Agent": USER_AGENT,
    "X-Platform": "macos",
    Accept: "application/json",
    "Accept-Language": "en-CA,en-US;q=0.9,en;q=0.8",
  });
  for (const [name, header] of [
    ["SW_X_ID", "X-ID"],
    ["SW_X_LICENSE", "X-License"],
    ["SW_X_SIGNATURE", "X-Signature"],
  ]) {
    const value = env[name!]?.trim();
    if (!value || !/^[\x21-\x7e]+$/.test(value))
      throw new Error(
        `Missing or invalid ${name}. Set it in the environment or configure Superwhisper in Handy.`,
      );
    headers.set(header!, value);
  }
  return headers;
}
export function scribeForm(audio: Blob, name: string, opts: Options) {
  const form = new FormData();
  form.set("file", audio, name);
  if (opts.language !== "auto") form.set("language_code", opts.language);
  if (opts.diarize) form.set("diarize", "true");
  if (opts["tag-audio-events"] || opts["no-tag-audio-events"])
    form.set("tag_audio_events", String(Boolean(opts["tag-audio-events"])));
  return form;
}
export function inferenceHost(host: unknown) {
  if (typeof host !== "string") throw new Error("Invalid S1 inference host.");
  const url = new URL(host);
  if (
    url.protocol !== "https:" ||
    !url.hostname.endsWith(".superwhisper.com") ||
    url.username ||
    url.password ||
    url.port ||
    url.pathname !== "/" ||
    url.search ||
    url.hash
  )
    throw new Error("Invalid S1 inference host.");
  return url;
}
type Send = (
  audio: Blob,
  name: string,
  sent: (bytes: number) => void,
  signal: AbortSignal,
) => Promise<unknown>;
// Serialize the form up front so the upload can report bytes as fetch pulls them.
async function streamedForm(form: FormData, sent: (bytes: number) => void) {
  const encoded = new Response(form);
  // Bun drops this header once the body is read, so take it first.
  const type = encoded.headers.get("Content-Type")!;
  const bytes = new Uint8Array(await encoded.arrayBuffer());
  let offset = 0;
  const body = new ReadableStream(
    {
      pull(controller) {
        if (offset >= bytes.length) return controller.close();
        const piece = bytes.slice(offset, offset + 64 * 1024);
        offset += piece.length;
        sent(piece.length);
        controller.enqueue(piece);
      },
    },
    { highWaterMark: 0 },
  );
  return {
    body,
    duplex: "half",
    headers: {
      "Content-Type": type,
      "Content-Length": String(bytes.length),
    },
  };
}
async function transcriber(
  opts: Options,
  headers: Headers,
  log: Log,
): Promise<Send> {
  if (opts.model === "scribe")
    return async (audio, name, sent, signal) => {
      const upload = await streamedForm(scribeForm(audio, name, opts), sent);
      return jsonRequest(`${API}/elevenlabs/v1/transcribe`, {
        ...upload,
        method: "POST",
        headers: { ...Object.fromEntries(headers), ...upload.headers },
        signal,
      });
    };
  log("Requesting S1 region and inference key...");
  const regions = await jsonRequest(`${API}/v2/inference/regions`, {
    headers: { "User-Agent": USER_AGENT, "X-Platform": "macos" },
  });
  if (!record(regions) || !Array.isArray(regions.regions))
    throw new Error("Invalid S1 region response.");
  const region: unknown = regions.regions.find(
    (r: unknown) => record(r) && r.id === regions.default,
  );
  if (!record(region) || typeof region.id !== "string")
    throw new Error("No default S1 region.");
  const host = inferenceHost(region.host);
  const keyHeaders = new Headers(headers);
  keyHeaders.set("Content-Type", "application/json");
  const key = await jsonRequest(
    `${API}/v2/inference/key?region=${encodeURIComponent(region.id)}`,
    { method: "POST", headers: keyHeaders, body: "{}" },
  );
  if (!record(key) || typeof key.key !== "string" || !key.key.trim())
    throw new Error("Missing S1 inference key.");
  return async (audio, name, sent, signal) => {
    const form = new FormData();
    form.set("audio", audio, name);
    form.set("language", opts.language);
    form.set("asr_prompt", "");
    form.set("enable_word_timestamps", "true");
    form.set("enable_audio_vocab", "false");
    const upload = await streamedForm(form, sent);
    return jsonRequest(new URL("generate", host), {
      ...upload,
      method: "POST",
      headers: { Authorization: `Bearer ${key.key}`, ...upload.headers },
      signal,
    });
  };
}
type Chunk = { path: string; mime: string; start: number };
const PARALLEL_UPLOADS = 3;
async function transcribeChunks(chunks: Chunk[], send: Send, log: Log) {
  const files = chunks.map((chunk) =>
    Bun.file(chunk.path, { type: chunk.mime }),
  );
  const total = files.reduce((sum, file) => sum + file.size, 0);
  const sent = chunks.map(() => 0);
  const results: unknown[] = [];
  let done = 0;
  const draw = () => {
    const bytes = sent.reduce((sum, n) => sum + n, 0);
    const uploaded = `${megabytes(bytes)} / ${megabytes(total)} uploaded`;
    bar(
      bytes / total,
      chunks.length > 1
        ? `${uploaded}, ${done}/${chunks.length} chunks transcribed`
        : bytes < total
          ? uploaded
          : `${megabytes(total)} uploaded, waiting for the transcript...`,
    );
  };
  log(
    chunks.length > 1
      ? `Uploading ${chunks.length} chunks (${megabytes(total)}), ${PARALLEL_UPLOADS} at a time...`
      : `Uploading ${megabytes(total)}...`,
  );
  const abort = new AbortController();
  let next = 0;
  const worker = async () => {
    while (next < chunks.length && !abort.signal.aborted) {
      const i = next++;
      const name =
        chunks.length > 1 ? `chunk ${i + 1}/${chunks.length}` : "Upload";
      for (let attempt = 1; ; attempt++) {
        sent[i] = 0;
        try {
          results[i] = await send(
            files[i]!,
            basename(chunks[i]!.path),
            (bytes) => {
              sent[i]! += bytes;
              draw();
            },
            abort.signal,
          );
          break;
        } catch (error) {
          if (
            attempt >= 3 ||
            abort.signal.aborted ||
            !(error instanceof RequestError && error.retryable)
          )
            throw error;
          log(`${name} failed (${error.message}) Retrying...`);
          await Bun.sleep(attempt * 5000);
        }
      }
      done++;
      if (chunks.length > 1) {
        if (!tty) log(`Transcribed chunk ${i + 1}/${chunks.length}.`);
        draw();
      }
    }
  };
  try {
    draw();
    await Promise.all(
      Array.from({ length: Math.min(PARALLEL_UPLOADS, chunks.length) }, () =>
        worker().catch((error: unknown) => {
          // Stop the other uploads so a failure doesn't keep consuming usage.
          abort.abort();
          throw error;
        }),
      ),
    );
  } finally {
    clearBar();
  }
  return chunks.length > 1
    ? mergeChunks(
        results.map((response, i) => ({ response, start: chunks[i]!.start })),
      )
    : results[0];
}
export function mergeChunks(parts: { response: unknown; start: number }[]) {
  const responses = parts.map(({ response }) => transcript(response));
  const timed = responses.every((response) => Array.isArray(response.words));
  return {
    ...responses[0],
    text: responses
      .map((response) => String(response.text).trim())
      .filter(Boolean)
      .join(" "),
    words: timed
      ? parts.flatMap(({ start }, i) =>
          (responses[i]!.words as unknown[]).map((word) =>
            record(word) &&
            typeof word.start === "number" &&
            typeof word.end === "number"
              ? { ...word, start: word.start + start, end: word.end + start }
              : word,
          ),
        )
      : undefined,
    _chunk_starts: parts.map(({ start }) => start),
  };
}

export function uploadFormat(codec: string) {
  switch (codec) {
    case "opus":
      return { extension: "ogg", mime: "audio/ogg", copy: true };
    case "mp3":
      return { extension: "mp3", mime: "audio/mpeg", copy: true };
    case "aac":
      return { extension: "m4a", mime: "audio/mp4", copy: true };
    case "flac":
      return { extension: "flac", mime: "audio/flac", copy: true };
    case "pcm_s16le":
      return { extension: "wav", mime: "audio/wav", copy: true };
    // Lossless PCM fallback avoids introducing another lossy encoding generation.
    default:
      return { extension: "wav", mime: "audio/wav", copy: false };
  }
}
// Superwhisper rejects recordings over 15 minutes with HTTP 413, so longer
// audio is split at silences into chunks of at most CHUNK_MAX seconds.
export const CHUNK_MAX = 14 * 60;
// Uncompressed WAV fallbacks can be huge; re-encode those instead of uploading.
export const UPLOAD_LIMIT = 50 * 1000 * 1000;
const OPUS = ["-ac", "1", "-c:a", "libopus", "-b:a", "64k"];
type Silence = { start: number; end: number };
export function silences(log: string): Silence[] {
  return [
    ...log.matchAll(/silence_end: ([\d.]+) \| silence_duration: ([\d.]+)/g),
  ].map((match) => ({
    start: Number(match[1]) - Number(match[2]),
    end: Number(match[1]),
  }));
}
// Choose all cuts together (shortest path over the silences) so an early cut
// never strands a later chunk without one. In order of priority: avoid cutting
// outside a silence, use as few chunks as possible, prefer longer silences,
// then keep chunk lengths even. Cuts outside a silence only happen where a
// silence-free stretch is longer than CHUNK_MAX, and are spread evenly there.
const HARD_CUT = 10;
const CHUNK_COST = 1;
const SHORT_SILENCE = 0.2;
const UNEVEN = 0.1;
export function cutPoints(length: number, quiet: Silence[]) {
  if (length <= CHUNK_MAX) return [];
  const silent = quiet
    .map((silence) => ({
      at: (silence.start + silence.end) / 2,
      cost:
        SHORT_SILENCE * (1 - Math.min(silence.end - silence.start, 1.5) / 1.5),
    }))
    .filter(({ at }) => at > 0 && at < length)
    .sort((a, b) => a.at - b.at);
  const points: { at: number; cost: number }[] = [];
  for (const point of [
    { at: 0, cost: 0 },
    ...silent,
    { at: length, cost: 0 },
  ]) {
    const last = points.at(-1);
    const extra = last ? Math.ceil((point.at - last.at) / CHUNK_MAX) - 1 : 0;
    for (let k = 1; k <= extra; k++)
      points.push({
        at: last!.at + ((point.at - last!.at) * k) / (extra + 1),
        cost: HARD_CUT,
      });
    points.push(point);
  }
  const target = length / Math.ceil(length / CHUNK_MAX);
  const best = points.map(() => Infinity);
  const previous = points.map(() => -1);
  best[0] = 0;
  for (let j = 1; j < points.length; j++)
    for (let i = j - 1; i >= 0; i--) {
      const size = points[j]!.at - points[i]!.at;
      if (size > CHUNK_MAX) break;
      const cost =
        best[i]! +
        CHUNK_COST +
        points[j]!.cost +
        UNEVEN * ((size - target) / CHUNK_MAX) ** 2;
      if (cost < best[j]!) {
        best[j] = cost;
        previous[j] = i;
      }
    }
  const cuts: number[] = [];
  for (let i = previous[points.length - 1]!; i > 0; i = previous[i]!)
    cuts.unshift(points[i]!.at);
  return cuts;
}
async function ffmpeg(args: string[]) {
  const child = Bun.spawn(["ffmpeg", "-hide_banner", "-nostdin", ...args], {
    stdout: "ignore",
    stderr: "pipe",
  });
  const log = await new Response(child.stderr).text();
  if ((await child.exited) !== 0)
    throw new Error("ffmpeg could not extract the audio track.");
  return log;
}
export async function prepareAudio(
  input: string,
  directory: string,
  log: Log = () => {},
): Promise<{ chunks: Chunk[]; offset: number }> {
  log("Inspecting audio track...");
  const probe = Bun.spawn(
    [
      "ffprobe",
      "-v",
      "error",
      "-select_streams",
      "a:0",
      "-show_entries",
      "stream=codec_name,start_time:format=start_time,duration",
      "-of",
      "json",
      resolve(input),
    ],
    { stdout: "pipe", stderr: "ignore" },
  );
  const raw = await new Response(probe.stdout).text();
  if ((await probe.exited) !== 0)
    throw new Error("ffprobe could not inspect the input.");
  const info: unknown = JSON.parse(raw);
  if (
    !record(info) ||
    !Array.isArray(info.streams) ||
    !record(info.streams[0]) ||
    typeof info.streams[0].codec_name !== "string"
  )
    throw new Error("Input has no usable audio track.");
  const stream = info.streams[0];
  const format = uploadFormat(stream.codec_name as string);
  const trackStart = Number(stream.start_time);
  const mediaStart = record(info.format) ? Number(info.format.start_time) : 0;
  const offset =
    Number.isFinite(trackStart) && Number.isFinite(mediaStart)
      ? Math.max(0, trackStart - mediaStart)
      : 0;
  const length = record(info.format) ? Number(info.format.duration) : NaN;
  const known = Number.isFinite(length) && length > 0;
  const source = [
    "-loglevel",
    "error",
    "-i",
    resolve(input),
    "-map",
    "0:a:0",
    "-vn",
    "-map_metadata",
    "-1",
  ];
  let start = performance.now();
  if (known && length > CHUNK_MAX) {
    log(
      `Found ${stream.codec_name} audio, ${duration(length)} long. Superwhisper accepts at most 15 minutes, so finding silences to split at...`,
    );
    const detection = await ffmpeg([
      ...source.slice(2),
      "-af",
      "aformat=channel_layouts=mono,silencedetect=noise=-35dB:d=0.4",
      "-f",
      "null",
      "-",
    ]);
    const cuts = cutPoints(length, silences(detection));
    log(
      `Found silences in ${since(start)}. Encoding ${cuts.length + 1} mono Opus chunks...`,
    );
    start = performance.now();
    const list = join(directory, "chunks.csv");
    await ffmpeg([
      ...source,
      ...OPUS,
      "-f",
      "segment",
      "-segment_format",
      "ogg",
      "-segment_times",
      cuts.map((cut) => cut.toFixed(3)).join(","),
      "-reset_timestamps",
      "1",
      "-segment_list",
      list,
      "-segment_list_type",
      "csv",
      join(directory, "chunk%03d.ogg"),
    ]);
    const chunks = (await readFile(list, "utf8"))
      .trim()
      .split("\n")
      .map((line) => {
        const [name, begin] = line.split(",");
        return {
          path: join(directory, name!),
          mime: "audio/ogg",
          start: Number(begin),
        };
      });
    log(
      `Encoded ${chunks.length} chunks in ${since(start)}: ${chunks
        .map((chunk, i) =>
          duration((chunks[i + 1]?.start ?? length) - chunk.start),
        )
        .join(", ")}.`,
    );
    return { chunks, offset };
  }
  log(
    `Found ${stream.codec_name} audio${known ? `, ${duration(length)} long` : ""}. ${
      format.copy ? "Extracting without re-encoding" : "Converting to WAV"
    }...`,
  );
  const path = join(directory, `audio.${format.extension}`);
  await ffmpeg([...source, "-c:a", format.copy ? "copy" : "pcm_s16le", path]);
  const size = Bun.file(path).size;
  log(`Extracted ${megabytes(size)} in ${since(start)}.`);
  if (size <= UPLOAD_LIMIT)
    return { chunks: [{ path, mime: format.mime, start: 0 }], offset };
  await rm(path, { force: true });
  log(`Over ${megabytes(UPLOAD_LIMIT)}. Re-encoding to mono Opus...`);
  start = performance.now();
  const opus = join(directory, "audio.ogg");
  await ffmpeg([...source, ...OPUS, opus]);
  log(`Re-encoded to ${megabytes(Bun.file(opus).size)} in ${since(start)}.`);
  return { chunks: [{ path: opus, mime: "audio/ogg", start: 0 }], offset };
}

async function main() {
  const opts = options(process.argv.slice(2));
  if (opts.help) {
    console.log(HELP);
    return;
  }
  const input = Bun.file(opts.input);
  if (!(await input.exists()) || input.size === 0)
    throw new Error("Input file is missing or empty.");
  const headers = opts["from-json"]
    ? undefined
    : await loadCredentials(process.env, opts["handy-config"]);
  const destinations = [opts.output, opts["save-json"]].filter(
    (p): p is string => p !== undefined,
  );
  const paths = destinations.map((p) => resolve(p));
  if (
    paths.includes(resolve(opts.input)) ||
    new Set(paths).size !== paths.length
  )
    throw new Error("Input and output paths must be distinct.");
  // Reserve output names before consuming account usage. Exclusive creation also rejects symlinks.
  const outputs = new Map<string, Awaited<ReturnType<typeof open>>>();
  const completed = new Set<string>();
  let temporary: string | undefined;
  try {
    for (const path of destinations)
      outputs.set(path, await open(path, "wx", 0o600));
    let response: unknown;
    if (opts["from-json"]) {
      try {
        response = await input.json();
      } catch {
        throw new Error("Input is not valid JSON.");
      }
    } else {
      let chunks: Chunk[] = [{ path: opts.input, mime: input.type, start: 0 }];
      let offset = 0;
      if (!opts["no-convert"]) {
        temporary = await mkdtemp(join(tmpdir(), "superwhisper-"));
        ({ chunks, offset } = await prepareAudio(
          opts.input,
          temporary,
          status,
        ));
      }
      if (chunks.length > 1 && opts.diarize)
        status("Speaker labels restart in every chunk and may not match.");
      const send = await transcriber(opts, headers!, status);
      const start = performance.now();
      response = await transcribeChunks(chunks, send, status);
      status(`Transcribed in ${since(start)}.`);
      // Preserve a delayed audio track's position on the video's playback timeline.
      if (record(response) && offset > 0)
        response._audio_start_seconds = offset;
    }
    if (opts["save-json"]) {
      await outputs
        .get(opts["save-json"])!
        .writeFile(JSON.stringify(response, null, 2) + "\n");
      completed.add(opts["save-json"]);
      status(`Saved response to ${opts["save-json"]}.`);
    }
    const result = render(
      response,
      Boolean(opts.subtitles),
      Boolean(opts.diarize),
    );
    if (opts.output) {
      await outputs.get(opts.output)!.writeFile(result);
      completed.add(opts.output);
      status(`Wrote ${opts.output}.`);
    } else {
      await Bun.write(Bun.stdout, result);
    }
  } finally {
    for (const [path, handle] of outputs) {
      await handle.close();
      if (!completed.has(path)) await rm(path, { force: true });
    }
    if (temporary) await rm(temporary, { recursive: true, force: true });
  }
}
if (import.meta.main) {
  main().catch((error: unknown) => {
    clearBar();
    console.error(
      `superwhisper: ${error instanceof Error ? error.message : "Operation failed."}`,
    );
    process.exitCode = 1;
  });
}
