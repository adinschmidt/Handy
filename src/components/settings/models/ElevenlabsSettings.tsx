import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "@/hooks/useSettings";
import { ToggleSwitch } from "@/components/ui/ToggleSwitch";
import { ApiKeyField } from "../PostProcessingSettingsApi/ApiKeyField";
import {
  CloudProviderCard,
  CloudProviderFooter,
  CloudProviderRow,
} from "./CloudProviderCard";

export function ElevenlabsSettings() {
  const { t } = useTranslation();
  const { settings } = useSettings();

  return (
    <CloudProviderCard
      provider="elevenlabs_scribe"
      name={t("settings.models.cloud.elevenlabs.name")}
      description={t("settings.models.cloud.elevenlabs.description")}
      summary={t("settings.models.cloud.elevenlabs.model")}
      configured={Boolean(
        settings?.transcription_api_keys?.elevenlabs_scribe?.trim(),
      )}
    >
      <ElevenlabsFields />
    </CloudProviderCard>
  );
}

function ElevenlabsFields() {
  const { t } = useTranslation();
  const {
    settings,
    isUpdating,
    getSetting,
    updateSetting,
    updateTranscriptionApiKey,
    setTranscriptionProvider,
  } = useSettings();
  const savedKey = settings?.transcription_api_keys?.elevenlabs_scribe ?? "";
  const [error, setError] = useState(false);

  const saveApiKey = async (apiKey: string) => {
    if (apiKey === savedKey) return;
    try {
      await updateTranscriptionApiKey("elevenlabs_scribe", apiKey);
      // ElevenLabs can't run without a key, so clearing it falls back to local.
      if (
        !apiKey.trim() &&
        settings?.selected_transcription_provider === "elevenlabs_scribe"
      ) {
        await setTranscriptionProvider("local");
      }
      setError(false);
    } catch {
      setError(true);
    }
  };

  return (
    <>
      <CloudProviderRow label={t("settings.models.cloud.apiKey")}>
        <ApiKeyField
          value={savedKey}
          onBlur={(apiKey) => void saveApiKey(apiKey)}
          disabled={isUpdating("transcription_api_key:elevenlabs_scribe")}
        />
      </CloudProviderRow>
      <ToggleSwitch
        checked={getSetting("elevenlabs_audio_events") ?? true}
        onChange={(enabled) =>
          updateSetting("elevenlabs_audio_events", enabled)
        }
        isUpdating={isUpdating("elevenlabs_audio_events")}
        label={t("settings.models.cloud.elevenlabs.audioEvents")}
        description={t(
          "settings.models.cloud.elevenlabs.audioEventsDescription",
        )}
        grouped
      />
      {error && (
        <CloudProviderFooter>
          <p role="alert" className="text-red-500">
            {t("settings.models.cloud.elevenlabs.keySaveError")}
          </p>
        </CloudProviderFooter>
      )}
    </>
  );
}
