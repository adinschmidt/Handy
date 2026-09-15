#!/usr/bin/env bun
import { parseArgs } from "node:util";
import { open, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
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
      --from-json         Render a saved response without uploading audio
      --no-convert        Upload as-is instead of extracting with ffmpeg
  -h, --help              Show help

Subtitles default to <input-basename>.srt beside the input; text uses stdout.
Requires Bun, ffmpeg and ffprobe unless --no-convert or --from-json is used.
Set SW_X_ID, SW_X_LICENSE, SW_X_SIGNATURE in the environment.
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

async function jsonRequest(
  url: string | URL,
  init: RequestInit = {},
): Promise<unknown> {
  let response: Response;
  try {
    response = await fetch(url, {
      ...init,
      redirect: "error",
      signal: AbortSignal.timeout(30 * 60 * 1000),
    });
  } catch {
    throw new Error("Superwhisper request failed or timed out.");
  }
  if (!response.ok)
    throw new Error(
      `Superwhisper returned HTTP ${response.status}.${[401, 403].includes(response.status) ? " Refresh your Superwhisper credentials." : ""}`,
    );
  try {
    return await response.json();
  } catch {
    throw new Error("Superwhisper returned invalid JSON.");
  }
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
    const value = env[name!];
    if (!value || !/^[\x21-\x7e]+$/.test(value))
      throw new Error(`Missing or invalid ${name}.`);
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
async function transcribe(
  audio: Blob,
  name: string,
  opts: Options,
  headers: Headers,
) {
  if (opts.model === "scribe")
    return jsonRequest(`${API}/elevenlabs/v1/transcribe`, {
      method: "POST",
      headers,
      body: scribeForm(audio, name, opts),
    });
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
  const form = new FormData();
  form.set("audio", audio, name);
  form.set("language", opts.language);
  form.set("asr_prompt", "");
  form.set("enable_word_timestamps", "true");
  form.set("enable_audio_vocab", "false");
  return jsonRequest(new URL("generate", host), {
    method: "POST",
    headers: { Authorization: `Bearer ${key.key}` },
    body: form,
  });
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
export async function prepareAudio(input: string, directory: string) {
  const probe = Bun.spawn(
    [
      "ffprobe",
      "-v",
      "error",
      "-select_streams",
      "a:0",
      "-show_entries",
      "stream=codec_name,start_time:format=start_time",
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
  const path = join(directory, `audio.${format.extension}`);
  const child = Bun.spawn(
    [
      "ffmpeg",
      "-hide_banner",
      "-loglevel",
      "error",
      "-nostdin",
      "-i",
      resolve(input),
      "-map",
      "0:a:0",
      "-vn",
      "-map_metadata",
      "-1",
      "-c:a",
      format.copy ? "copy" : "pcm_s16le",
      path,
    ],
    { stdout: "ignore", stderr: "ignore" },
  );
  if ((await child.exited) !== 0)
    throw new Error("ffmpeg could not extract the audio track.");
  return { path, mime: format.mime, offset };
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
  const headers = opts["from-json"] ? undefined : credentials(process.env);
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
      let audio = input;
      let offset = 0;
      if (!opts["no-convert"]) {
        temporary = await mkdtemp(join(tmpdir(), "superwhisper-"));
        const prepared = await prepareAudio(opts.input, temporary);
        audio = Bun.file(prepared.path, { type: prepared.mime });
        offset = prepared.offset;
      }
      response = await transcribe(
        audio,
        basename(audio.name ?? opts.input),
        opts,
        headers!,
      );
      // Preserve a delayed audio track's position on the video's playback timeline.
      if (record(response) && offset > 0)
        response._audio_start_seconds = offset;
    }
    if (opts["save-json"]) {
      await outputs
        .get(opts["save-json"])!
        .writeFile(JSON.stringify(response, null, 2) + "\n");
      completed.add(opts["save-json"]);
    }
    const result = render(
      response,
      Boolean(opts.subtitles),
      Boolean(opts.diarize),
    );
    if (opts.output) {
      await outputs.get(opts.output)!.writeFile(result);
      completed.add(opts.output);
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
    console.error(
      `superwhisper: ${error instanceof Error ? error.message : "Operation failed."}`,
    );
    process.exitCode = 1;
  });
}
