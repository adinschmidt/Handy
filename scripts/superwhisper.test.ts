import { expect, test } from "bun:test";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  prepareAudio,
  loadCredentials,
  handyConfigPath,
  inferenceHost,
  options,
  CHUNK_MAX,
  cutPoints,
  mergeChunks,
  silences,
  render,
  scribeForm,
  timestamp,
} from "./superwhisper";

const response = {
  text: "Hello there. Hi! (laughter)",
  words: [
    {
      text: "Hello",
      start: 0.1,
      end: 0.4,
      speaker_id: "speaker_0",
      type: "word",
    },
    { text: " ", type: "spacing" },
    {
      text: "there.",
      start: 0.5,
      end: 1,
      speaker_id: "speaker_0",
      type: "word",
    },
    {
      text: "Hi!",
      start: 1.2,
      end: 1.6,
      speaker_id: "speaker_1",
      type: "word",
    },
    { text: "(laughter)", start: 2, end: 3, type: "audio_event" },
  ],
};
test("SRT retains speaker turns and unlabeled audio events", () => {
  expect(render(response, true, true)).toBe(
    "1\n00:00:00,100 --> 00:00:01,000\nspeaker_0: Hello there.\n\n2\n00:00:01,200 --> 00:00:01,600\nspeaker_1: Hi!\n\n3\n00:00:02,000 --> 00:00:03,000\n(laughter)\n\n",
  );
  expect(render(response, false, true)).toBe(
    "speaker_0: Hello there.\nspeaker_1: Hi!\n(laughter)\n",
  );
  expect(render(response, false, false)).toBe(response.text + "\n");
});
test("subtitle cues split at long pauses and timestamps carry into hours", () => {
  expect(timestamp(3599.9996)).toBe("01:00:00,000");
  const result = render(
    {
      text: "one two",
      words: [
        { text: "one", start: 0, end: 0.2 },
        { text: "two", start: 2, end: 2 },
      ],
    },
    true,
    false,
  );
  expect(result).toContain("2\n00:00:02,000 --> 00:00:02,001\ntwo");
  expect(() => render({ text: "hello" }, true, false)).toThrow("timestamps");
  expect(() =>
    render(
      { text: "hello", words: [{ text: "hello", start: 0, end: 1 }] },
      false,
      true,
    ),
  ).toThrow("no speaker labels");
  expect(() =>
    render(
      { text: "hello", words: [{ text: "hello", start: 0 }] },
      true,
      false,
    ),
  ).toThrow("timestamps");
});
test("CLI fields select Scribe route without inventing a revision selector", () => {
  const defaults = scribeForm(
    new Blob(["audio"]),
    "audio.wav",
    options(["audio.wav"]),
  );
  expect([...defaults.keys()]).toEqual(["file"]);
  const enabled = scribeForm(
    new Blob(["audio"]),
    "audio.wav",
    options([
      "--diarize",
      "--tag-audio-events",
      "--language",
      "en",
      "audio.wav",
    ]),
  );
  expect(enabled.get("diarize")).toBe("true");
  expect(enabled.get("tag_audio_events")).toBe("true");
  expect(enabled.get("language_code")).toBe("en");
  expect(
    scribeForm(new Blob(), "a", options(["--no-tag-audio-events", "a"])).get(
      "tag_audio_events",
    ),
  ).toBe("false");
  expect(() => options(["--model", "s1-voice", "--subtitles", "a"])).toThrow(
    "require --model scribe",
  );
  expect(() => options(["--model", "scribe_v2", "a"])).toThrow("proxy chooses");
  expect(() =>
    inferenceHost("https://superwhisper.com.evil.example/"),
  ).toThrow();
  expect(() =>
    inferenceHost("https://user@us.aws.superwhisper.com/"),
  ).toThrow();
});
test("CLI renders offline, saves JSON, and preserves existing files", async () => {
  const dir = await mkdtemp(join(tmpdir(), "superwhisper-test-"));
  try {
    const input = join(dir, "input.json");
    const output = join(dir, "output.srt");
    const saved = join(dir, "saved.json");
    await Bun.write(input, JSON.stringify(response));
    const run = () =>
      Bun.spawn(
        [
          process.execPath,
          join(import.meta.dir, "superwhisper.ts"),
          "--from-json",
          "--subtitles",
          "--diarize",
          "--output",
          output,
          "--save-json",
          saved,
          input,
        ],
        { stdout: "pipe", stderr: "pipe" },
      );
    expect(await run().exited).toBe(0);
    expect(await Bun.file(output).text()).toBe(render(response, true, true));
    expect(await Bun.file(saved).json()).toEqual(response);
    expect(await run().exited).toBe(1);
    expect(await Bun.file(output).text()).toBe(render(response, true, true));
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("subtitle filenames follow the input and stdout remains explicit", () => {
  expect(options(["/tmp/movie.final.mp4", "--subtitles"]).output).toBe(
    "/tmp/movie.final.srt",
  );
  expect(
    options(["movie.mp4", "--subtitles", "-o", "-"]).output,
  ).toBeUndefined();
});
test("FFmpeg extracts AAC video without re-encoding and converts unsupported codecs", async () => {
  const dir = await mkdtemp(join(tmpdir(), "superwhisper-media-test-"));
  try {
    const video = join(dir, "input.mp4");
    const create = Bun.spawn(
      [
        "ffmpeg",
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "color=s=16x16:d=1",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:duration=1",
        "-c:v",
        "mpeg4",
        "-c:a",
        "aac",
        "-shortest",
        video,
      ],
      { stdout: "ignore", stderr: "pipe" },
    );
    expect(await create.exited).toBe(0);
    const { chunks } = await prepareAudio(video, dir);
    expect(chunks).toHaveLength(1);
    const prepared = chunks[0]!;
    expect(prepared.mime).toBe("audio/mp4");
    const hash = async (path: string) => {
      const child = Bun.spawn(
        [
          "ffmpeg",
          "-v",
          "error",
          "-i",
          path,
          "-map",
          "0:a:0",
          "-c:a",
          "copy",
          "-f",
          "hash",
          "-hash",
          "sha256",
          "-",
        ],
        { stdout: "pipe", stderr: "ignore" },
      );
      const result = await new Response(child.stdout).text();
      expect(await child.exited).toBe(0);
      return result;
    };
    expect(await hash(prepared.path)).toBe(await hash(video));
    const other = join(dir, "input.aiff");
    const createOther = Bun.spawn(
      [
        "ffmpeg",
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "sine=duration=0.1",
        "-c:a",
        "pcm_s16be",
        other,
      ],
      { stdout: "ignore", stderr: "ignore" },
    );
    expect(await createOther.exited).toBe(0);
    expect((await prepareAudio(other, dir)).chunks[0]!.mime).toBe("audio/wav");
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("credentials prefer environment values and fill gaps from Handy without writes", async () => {
  const dir = await mkdtemp(join(tmpdir(), "superwhisper-config-test-"));
  const path = join(dir, "settings_store.json");
  try {
    const content = JSON.stringify({
      settings: {
        transcription_api_keys: {
          superwhisper_x_id: "saved-id",
          superwhisper_x_license: "saved-license",
          superwhisper_x_signature: "saved-signature",
        },
      },
    });
    await Bun.write(path, content);
    const saved = await loadCredentials({}, path);
    expect(saved.get("X-ID")).toBe("saved-id");
    const mixed = await loadCredentials(
      { SW_X_ID: "env-id", SW_X_LICENSE: "" },
      path,
    );
    expect(mixed.get("X-ID")).toBe("env-id");
    expect(mixed.get("X-License")).toBe("saved-license");
    expect(mixed.get("X-Signature")).toBe("saved-signature");
    expect(await Bun.file(path).text()).toBe(content);
    await Bun.write(path, "invalid private content");
    const complete = {
      SW_X_ID: "env-id",
      SW_X_LICENSE: "env-license",
      SW_X_SIGNATURE: "env-signature",
    };
    expect((await loadCredentials(complete, path)).get("X-License")).toBe(
      "env-license",
    );
    await expect(loadCredentials({}, path)).rejects.toThrow("invalid JSON");
    await expect(
      loadCredentials({}, join(dir, "missing.json")),
    ).rejects.toThrow("SW_X_ID");
    expect(handyConfigPath({}, "darwin", "/home/test")).toBe(
      "/home/test/Library/Application Support/com.pais.handy/settings_store.json",
    );
    expect(
      handyConfigPath({ XDG_DATA_HOME: "/data" }, "linux", "/home/test"),
    ).toBe("/data/com.pais.handy/settings_store.json");
    expect(
      handyConfigPath({ APPDATA: "/roaming" }, "win32", "/home/test"),
    ).toBe("/roaming/com.pais.handy/settings_store.json");
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("long audio is cut in silences planned across the whole file", () => {
  const quiet = [
    { start: 500, end: 500.5 },
    { start: 700, end: 702 },
    { start: 830, end: 830.4 },
  ];
  expect(cutPoints(CHUNK_MAX, quiet)).toEqual([]);
  // The 2 second silence beats the shorter ones.
  expect(cutPoints(1200, quiet)).toEqual([701]);
  // Greedy cutting at 830 leaves no silence for the next chunk; planning
  // ahead cuts at 830 and 1300 so every cut stays in a silence.
  const sparse = [400, 830, 1300].map((at) => ({
    start: at - 0.5,
    end: at + 0.5,
  }));
  expect(cutPoints(2100, sparse)).toEqual([830, 1300]);
  // No silence at all: the forced cuts are spread evenly.
  const forced = cutPoints(2000, []);
  expect(forced).toHaveLength(2);
  expect(forced[0]).toBeCloseTo(2000 / 3);
  expect(forced[1]).toBeCloseTo(4000 / 3);
  expect(
    silences(
      "[silencedetect @ 0x1] silence_start: 9.5\n[silencedetect @ 0x1] silence_end: 10.25 | silence_duration: 0.75\n",
    ),
  ).toEqual([{ start: 9.5, end: 10.25 }]);
});

test("chunk transcripts merge with words shifted onto the full timeline", () => {
  const merged = mergeChunks([
    {
      start: 0,
      response: {
        text: "Hello there.",
        language_code: "en",
        words: [{ text: "Hello", start: 0.5, end: 0.9 }],
      },
    },
    {
      start: 600,
      response: {
        text: " Again.",
        words: [{ text: "Again.", start: 1, end: 1.4 }],
      },
    },
  ]);
  expect(merged.text).toBe("Hello there. Again.");
  expect(merged.language_code).toBe("en");
  expect(merged.words).toEqual([
    { text: "Hello", start: 0.5, end: 0.9 },
    { text: "Again.", start: 601, end: 601.4 },
  ]);
  expect(
    mergeChunks([
      { start: 0, response: { text: "a" } },
      { start: 9, response: { text: "b", words: [] } },
    ]).words,
  ).toBeUndefined();
});

test("audio over the limit is split at silences into Opus chunks", async () => {
  const dir = await mkdtemp(join(tmpdir(), "superwhisper-chunk-test-"));
  try {
    const input = join(dir, "long.m4a");
    // 20 minutes of tone with a 2 second gap every 100 seconds.
    const create = Bun.spawn(
      [
        "ffmpeg",
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:sample_rate=8000:duration=1200",
        "-af",
        "volume='if(lt(mod(t,100),98),1,0)':eval=frame",
        "-c:a",
        "aac",
        input,
      ],
      { stdout: "ignore", stderr: "ignore" },
    );
    expect(await create.exited).toBe(0);
    const { chunks } = await prepareAudio(input, dir);
    expect(chunks).toHaveLength(2);
    expect(chunks.every((chunk) => chunk.mime === "audio/ogg")).toBe(true);
    expect(chunks[0]!.start).toBe(0);
    // Even split: the gap nearest the 10 minute midpoint.
    expect(chunks[1]!.start).toBeGreaterThan(598);
    expect(chunks[1]!.start).toBeLessThan(600.1);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}, 30_000);
