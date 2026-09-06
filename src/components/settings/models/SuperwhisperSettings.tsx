import { platform } from "@tauri-apps/plugin-os";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "@/hooks/useSettings";
import {
  hasSuperwhisperCredentials,
  superwhisperFields,
} from "@/lib/superwhisper";

const emptyCredentials = {
  superwhisper_x_id: "",
  superwhisper_x_license: "",
  superwhisper_x_signature: "",
};

export function SuperwhisperSettings() {
  const { t } = useTranslation();
  const {
    settings,
    updateSetting,
    updateSuperwhisperCredentials,
    importSuperwhisperCredentials,
    setTranscriptionProvider,
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
  const active =
    settings?.selected_transcription_provider === "superwhisper_scribe";
  const busy =
    isUpdating("superwhisper_credentials") ||
    isUpdating("selected_transcription_provider");
  const dirty = superwhisperFields.some(
    ({ key }) => credentials[key].trim() !== (keys?.[key]?.trim() ?? ""),
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

  async function select() {
    setError(null);
    try {
      await setTranscriptionProvider("superwhisper_scribe");
    } catch {
      setError(t("settings.models.cloud.superwhisper.selectError"));
    }
  }

  return (
    <div className="rounded-lg border border-mid-gray/30 bg-background p-4 space-y-3">
      <div className="flex items-start justify-between gap-4">
        <div>
          <div className="text-sm font-medium">
            {t("settings.models.cloud.superwhisper.name")}
          </div>
          <p className="mt-1 text-xs text-text/55">
            {t("settings.models.cloud.superwhisper.description")}
          </p>
        </div>
        <button
          type="button"
          onClick={() => void select()}
          disabled={
            active || busy || dirty || !hasSuperwhisperCredentials(keys)
          }
          className="rounded-lg bg-logo-primary px-3 py-1.5 text-xs font-medium text-white disabled:opacity-50"
        >
          {t(
            active
              ? "settings.models.cloud.active"
              : "settings.models.cloud.use",
          )}
        </button>
      </div>
      {isMac && (
        <div className="space-y-1.5">
          <button
            type="button"
            onClick={() => void importCredentials()}
            disabled={busy}
            className="text-sm text-logo-primary disabled:opacity-50"
          >
            {t("settings.models.cloud.superwhisper.import")}
          </button>
        </div>
      )}
      {superwhisperFields.map(({ key, label }) => (
        <label key={key} className="block space-y-1.5">
          <span className="text-xs font-medium text-text/65">{t(label)}</span>
          <input
            type="password"
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
            className="w-full rounded-lg border border-mid-gray/40 bg-mid-gray/10 px-3 py-2 text-sm focus:outline-none focus:ring-1 focus:ring-logo-primary disabled:opacity-50"
          />
        </label>
      ))}
      <p className="text-xs text-text/55">
        {t("settings.models.cloud.superwhisper.storageWarning")}
      </p>
      <div className="flex gap-3">
        <button
          type="button"
          onClick={() => void save()}
          disabled={busy || !dirty || !hasSuperwhisperCredentials(credentials)}
          className="text-sm text-logo-primary disabled:opacity-50"
        >
          {t("settings.models.cloud.superwhisper.save")}
        </button>
        <button
          type="button"
          onClick={() => void save(true)}
          disabled={
            busy ||
            !superwhisperFields.some(
              ({ key }) => credentials[key] || keys?.[key],
            )
          }
          className="text-sm text-text/65 disabled:opacity-50"
        >
          {t("settings.models.cloud.superwhisper.clear")}
        </button>
      </div>
      <label className="block space-y-1.5">
        <span className="text-xs font-medium text-text/65">
          {t("settings.models.cloud.superwhisper.audioEvents")}
        </span>
        <select
          value={
            settings?.superwhisper_audio_events == null
              ? "default"
              : String(settings.superwhisper_audio_events)
          }
          disabled={isUpdating("superwhisper_audio_events")}
          onChange={(event) =>
            void updateSetting(
              "superwhisper_audio_events",
              event.target.value === "default"
                ? null
                : event.target.value === "true",
            )
          }
          className="w-full rounded-lg border border-mid-gray/40 bg-background px-3 py-2 text-sm disabled:opacity-50"
        >
          <option value="default">
            {t("settings.models.cloud.superwhisper.audioEventsDefault")}
          </option>
          <option value="true">
            {t("settings.models.cloud.superwhisper.audioEventsOn")}
          </option>
          <option value="false">
            {t("settings.models.cloud.superwhisper.audioEventsOff")}
          </option>
        </select>
        <span className="block text-xs text-text/55">
          {t("settings.models.cloud.superwhisper.audioEventsDescription")}
        </span>
      </label>
      {error && (
        <p role="alert" className="text-xs text-red-500">
          {error}
        </p>
      )}
      {imported && (
        <p role="status" className="text-xs text-text/65">
          {t("settings.models.cloud.superwhisper.imported")}
        </p>
      )}
      {saved && (
        <p role="status" className="text-xs text-text/65">
          {t("settings.models.cloud.superwhisper.saved")}
        </p>
      )}
    </div>
  );
}
