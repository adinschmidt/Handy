import type { AppSettings } from "@/bindings";

export const superwhisperFields = [
  {
    key: "superwhisper_x_id",
    label: "settings.models.cloud.superwhisper.deviceId",
  },
  {
    key: "superwhisper_x_license",
    label: "settings.models.cloud.superwhisper.licenseId",
  },
  {
    key: "superwhisper_x_signature",
    label: "settings.models.cloud.superwhisper.signature",
  },
] as const;

export function hasSuperwhisperCredentials(
  keys: AppSettings["transcription_api_keys"] | undefined,
): boolean {
  const uuid =
    /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
  return (
    uuid.test(keys?.superwhisper_x_id?.trim() ?? "") &&
    uuid.test(keys?.superwhisper_x_license?.trim() ?? "") &&
    /^[0-9a-f]{64}$/.test(keys?.superwhisper_x_signature?.trim() ?? "")
  );
}
