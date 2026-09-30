import { platform } from "@tauri-apps/plugin-os";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { SuperwhisperModel } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import {
  hasSuperwhisperCredentials,
  superwhisperFields,
} from "@/lib/superwhisper";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { Select } from "@/components/ui/Select";
import { SettingContainer } from "@/components/ui/SettingContainer";
import {
  CloudProviderCard,
  CloudProviderFooter,
  CloudProviderRow,
} from "./CloudProviderCard";

const emptyCredentials = {
  superwhisper_x_id: "",
  superwhisper_x_license: "",
  superwhisper_x_signature: "",
};

const models = [
  { value: "scribe", label: "settings.models.cloud.superwhisper.scribe" },
  { value: "s1_voice", label: "settings.models.cloud.superwhisper.s1Voice" },
] as const satisfies readonly { value: SuperwhisperModel; label: string }[];

// `null` omits `tag_audio_events`, leaving the choice to Superwhisper.
const audioEventOptions = [
  {
    value: "default",
    setting: null,
    label: "settings.models.cloud.superwhisper.audioEventsDefault",
  },
  {
    value: "true",
    setting: true,
    label: "settings.models.cloud.superwhisper.audioEventsOn",
  },
  {
    value: "false",
    setting: false,
    label: "settings.models.cloud.superwhisper.audioEventsOff",
  },
] as const;

export function SuperwhisperSettings() {
  const { t } = useTranslation();
  const { settings } = useSettings();
  const model = models.find(
    ({ value }) => value === (settings?.superwhisper_model ?? "scribe"),
  );

  return (
    <CloudProviderCard
      provider="superwhisper_scribe"
      name={t("settings.models.cloud.superwhisper.name")}
      description={t("settings.models.cloud.superwhisper.description")}
      summary={model && t(model.label)}
      configured={hasSuperwhisperCredentials(settings?.transcription_api_keys)}
    >
      <SuperwhisperFields />
    </CloudProviderCard>
  );
}

function SuperwhisperFields() {
  const { t } = useTranslation();
  const {
    settings,
    updateSetting,
    updateSuperwhisperCredentials,
    importSuperwhisperCredentials,
    isUpdating,
  } = useSettings();
  const [credentials, setCredentials] = useState(emptyCredentials);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [imported, setImported] = useState(false);
  const isMac = platform() === "macos";
  const keys = settings?.transcription_api_keys;
  useEffect(() => {
    setCredentials({
      superwhisper_x_id: keys?.superwhisper_x_id ?? "",
      superwhisper_x_license: keys?.superwhisper_x_license ?? "",
      superwhisper_x_signature: keys?.superwhisper_x_signature ?? "",
    });
  }, [
    keys?.superwhisper_x_id,
    keys?.superwhisper_x_license,
    keys?.superwhisper_x_signature,
  ]);
  const busy =
    isUpdating("superwhisper_credentials") ||
    isUpdating("selected_transcription_provider");
  const dirty = superwhisperFields.some(
    ({ key }) => credentials[key].trim() !== (keys?.[key]?.trim() ?? ""),
  );
  const selectedModel = settings?.superwhisper_model ?? "scribe";
  const audioEvents = audioEventOptions.find(
    ({ setting }) => setting === (settings?.superwhisper_audio_events ?? null),
  );

  async function save(clear = false) {
    setError(null);
    setSaved(false);
    setImported(false);
    const values = clear ? emptyCredentials : credentials;
    try {
      await updateSuperwhisperCredentials(
        values.superwhisper_x_id,
        values.superwhisper_x_license,
        values.superwhisper_x_signature,
      );
      setCredentials(values);
      setSaved(!clear);
    } catch {
      setError(t("settings.models.cloud.superwhisper.saveError"));
    }
  }

  async function importCredentials() {
    setError(null);
    setSaved(false);
    setImported(false);
    try {
      const importedKeys = await importSuperwhisperCredentials();
      setCredentials({
        superwhisper_x_id: importedKeys?.superwhisper_x_id ?? "",
        superwhisper_x_license: importedKeys?.superwhisper_x_license ?? "",
        superwhisper_x_signature: importedKeys?.superwhisper_x_signature ?? "",
      });
      setImported(true);
    } catch (error) {
      setError(
        t(
          error instanceof Error && error.message === "no_credentials"
            ? "settings.models.cloud.superwhisper.importMissing"
            : "settings.models.cloud.superwhisper.importError",
        ),
      );
    }
  }

  return (
    <>
      {superwhisperFields.map(({ key, label }) => (
        <CloudProviderRow key={key} label={t(label)}>
          <Input
            type="password"
            variant="compact"
            autoComplete="off"
            spellCheck={false}
            value={credentials[key]}
            disabled={busy}
            onChange={(event) => {
              setCredentials((previous) => ({
                ...previous,
                [key]: event.target.value,
              }));
              setSaved(false);
              setImported(false);
            }}
            className="flex-1 min-w-0"
          />
        </CloudProviderRow>
      ))}
      <CloudProviderFooter>
        <p>{t("settings.models.cloud.superwhisper.storageWarning")}</p>
        <div className="flex items-center gap-2 text-text">
          {isMac && (
            <Button
              variant="secondary"
              size="sm"
              onClick={() => void importCredentials()}
              disabled={busy}
            >
              {t("settings.models.cloud.superwhisper.import")}
            </Button>
          )}
          <Button
            variant="ghost"
            size="sm"
            onClick={() => void save(true)}
            disabled={
              busy ||
              !superwhisperFields.some(
                ({ key }) => credentials[key] || keys?.[key],
              )
            }
            className="ms-auto"
          >
            {t("settings.models.cloud.superwhisper.clear")}
          </Button>
          <Button
            variant="primary-soft"
            size="sm"
            onClick={() => void save()}
            disabled={
              busy || !dirty || !hasSuperwhisperCredentials(credentials)
            }
          >
            {t("settings.models.cloud.superwhisper.save")}
          </Button>
        </div>
        {error && (
          <p role="alert" className="text-red-500">
            {error}
          </p>
        )}
        {imported && (
          <p role="status">
            {t("settings.models.cloud.superwhisper.imported")}
          </p>
        )}
        {saved && (
          <p role="status">{t("settings.models.cloud.superwhisper.saved")}</p>
        )}
      </CloudProviderFooter>
      <CloudProviderRow label={t("settings.models.cloud.superwhisper.model")}>
        <Select
          value={selectedModel}
          options={models.map(({ value, label }) => ({
            value,
            label: t(label),
          }))}
          isClearable={false}
          disabled={isUpdating("superwhisper_model")}
          onChange={(value) => {
            const model = models.find((option) => option.value === value);
            if (model) void updateSetting("superwhisper_model", model.value);
          }}
          className="min-w-0 flex-1 text-sm"
        />
      </CloudProviderRow>
      <SettingContainer
        title={t("settings.models.cloud.superwhisper.audioEvents")}
        description={t(
          "settings.models.cloud.superwhisper.audioEventsDescription",
        )}
        grouped
        disabled={selectedModel === "s1_voice"}
      >
        <Select
          value={audioEvents?.value ?? "default"}
          options={audioEventOptions.map(({ value, label }) => ({
            value,
            label: t(label),
          }))}
          isClearable={false}
          disabled={
            isUpdating("superwhisper_audio_events") ||
            selectedModel === "s1_voice"
          }
          onChange={(value) => {
            const option = audioEventOptions.find(
              (candidate) => candidate.value === value,
            );
            if (option) {
              void updateSetting("superwhisper_audio_events", option.setting);
            }
          }}
          className="w-72 text-sm"
        />
      </SettingContainer>
    </>
  );
}
