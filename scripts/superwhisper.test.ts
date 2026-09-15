import { expect, test } from "bun:test";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  prepareAudio,
  inferenceHost,
  options,
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
    const prepared = await prepareAudio(video, dir);
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
    expect((await prepareAudio(other, dir)).mime).toBe("audio/wav");
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});
