import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "@/hooks/useSettings";
import { Input } from "@/components/ui/Input";
import { ApiKeyField } from "../PostProcessingSettingsApi/ApiKeyField";
import {
  CloudProviderCard,
  CloudProviderFooter,
  CloudProviderRow,
} from "./CloudProviderCard";

export function CodexSettings() {
  const { t } = useTranslation();
  const { settings } = useSettings();

  return (
    <CloudProviderCard
      provider="codex_asr"
      name={t("settings.models.cloud.codex.name")}
      description={t("settings.models.cloud.codex.description")}
      summary={settings?.codex_asr_base_url}
      // Codex ASR authentication is optional and the base URL has a default.
      configured
    >
      <CodexFields />
    </CloudProviderCard>
  );
}

function CodexFields() {
  const { t } = useTranslation();
  const {
    settings,
    isUpdating,
    updateCodexAsrBaseUrl,
    updateTranscriptionApiKey,
  } = useSettings();
  const savedUrl = settings?.codex_asr_base_url ?? "";
  const savedKey = settings?.transcription_api_keys?.codex_asr ?? "";
  // Not synced from settings: a rejected URL rolls the setting back, and the
  // field should keep the user's text beside the error.
  const [baseUrl, setBaseUrl] = useState(savedUrl);
  const [error, setError] = useState<string | null>(null);

  const saveBaseUrl = async () => {
    if (baseUrl === savedUrl) {
      setError(null);
      return;
    }
    try {
      await updateCodexAsrBaseUrl(baseUrl);
      setError(null);
    } catch {
      setError(t("settings.models.cloud.invalidUrl"));
    }
  };

  const saveApiKey = async (apiKey: string) => {
    if (apiKey === savedKey) return;
    try {
      await updateTranscriptionApiKey("codex_asr", apiKey);
      setError(null);
    } catch {
      setError(t("settings.models.cloud.codex.keySaveError"));
    }
  };

  return (
    <>
      <CloudProviderRow label={t("settings.models.cloud.baseUrl")}>
        <Input
          type="url"
          variant="compact"
          value={baseUrl}
          onChange={(event) => setBaseUrl(event.target.value)}
          onBlur={() => void saveBaseUrl()}
          disabled={isUpdating("codex_asr_base_url")}
          className="min-w-0 flex-1"
        />
      </CloudProviderRow>
      <CloudProviderRow label={t("settings.models.cloud.apiKeyOptional")}>
        <ApiKeyField
          value={savedKey}
          onBlur={(apiKey) => void saveApiKey(apiKey)}
          disabled={isUpdating("transcription_api_key:codex_asr")}
        />
      </CloudProviderRow>
      <CloudProviderFooter>
        <p>{t("settings.models.cloud.codex.setup")}</p>
        {error && (
          <p role="alert" className="text-red-500">
            {error}
          </p>
        )}
      </CloudProviderFooter>
    </>
  );
}
