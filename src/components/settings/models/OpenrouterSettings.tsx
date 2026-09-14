import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type OpenrouterModel } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";

export function OpenrouterSettings() {
  const { t } = useTranslation();
  const {
    settings,
    isUpdating,
    updateTranscriptionApiKey,
    setTranscriptionProvider,
    refreshSettings,
  } = useSettings();
  const savedKey = settings?.transcription_api_keys?.openrouter ?? "";
  const [apiKey, setApiKey] = useState(savedKey);
  const [models, setModels] = useState<OpenrouterModel[]>([]);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [fetchError, setFetchError] = useState(false);
  const [revision, setRevision] = useState(0);
  const keyUpdating = isUpdating("transcription_api_key:openrouter");
  const dirty = apiKey.trim() !== savedKey.trim();
  const selectedModel = settings?.openrouter_model ?? "";
  const active = settings?.selected_transcription_provider === "openrouter";

  useEffect(() => setApiKey(savedKey), [savedKey]);

  useEffect(() => {
    let cancelled = false;
    setModels([]);
    setFetchError(false);
    if (!savedKey.trim() || keyUpdating) {
      setLoading(false);
      return;
    }
    setLoading(true);
    void commands
      .fetchOpenrouterModels()
      .then((result) => {
        if (cancelled) return;
        if (result.status === "error") {
          setFetchError(true);
        } else {
          setModels(result.data);
        }
      })
      .catch(() => {
        if (!cancelled) setFetchError(true);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [savedKey, keyUpdating, revision]);

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setError("");
    try {
      await action();
    } catch {
      setError(t("settings.models.cloud.openrouter.saveError"));
    } finally {
      setBusy(false);
    }
  };

  const saveModel = (model: string) =>
    run(async () => {
      const result = await commands.changeOpenrouterModel(model);
      if (result.status === "error") throw new Error(result.error);
      await refreshSettings();
    });
  const disabled = busy || keyUpdating;
  const modelAvailable = models.some((model) => model.id === selectedModel);

  return (
    <div className="rounded-lg border border-mid-gray/30 bg-background p-4 space-y-3">
      <div className="flex items-start justify-between gap-4">
        <div>
          <div className="text-sm font-medium">
            {t("settings.models.cloud.openrouter.name")}
          </div>
          <p className="mt-1 text-xs text-text/55">
            {t("settings.models.cloud.openrouter.description")}
          </p>
        </div>
        <button
          type="button"
          disabled={
            disabled ||
            dirty ||
            loading ||
            active ||
            !savedKey.trim() ||
            !modelAvailable
          }
          onClick={() => void run(() => setTranscriptionProvider("openrouter"))}
          className="rounded-lg bg-logo-primary px-3 py-1.5 text-xs font-medium text-white disabled:opacity-50"
        >
          {t(
            active
              ? "settings.models.cloud.active"
              : "settings.models.cloud.use",
          )}
        </button>
      </div>
      <label className="block space-y-1.5">
        <span className="text-xs font-medium text-text/65">
          {t("settings.models.cloud.apiKey")}
        </span>
        <input
          type="password"
          value={apiKey}
          autoComplete="off"
          disabled={disabled}
          onChange={(event) => setApiKey(event.target.value)}
          className="w-full rounded-lg border border-mid-gray/40 bg-mid-gray/10 px-3 py-2 text-sm"
        />
      </label>
      <p className="text-xs text-text/45">
        {t("settings.models.cloud.openrouter.storage")}
      </p>
      <button
        type="button"
        disabled={disabled || !dirty}
        onClick={() =>
          void run(() => updateTranscriptionApiKey("openrouter", apiKey))
        }
        className="rounded-lg border border-mid-gray/30 px-3 py-1.5 text-xs disabled:opacity-50"
      >
        {t("settings.models.cloud.openrouter.saveKey")}
      </button>
      <label className="block space-y-1.5">
        <span className="text-xs font-medium text-text/65">
          {t("settings.models.cloud.openrouter.model")}
        </span>
        <select
          value={selectedModel}
          disabled={
            disabled ||
            dirty ||
            loading ||
            !savedKey.trim() ||
            models.length === 0
          }
          onChange={(event) => void saveModel(event.target.value)}
          className="w-full rounded-lg border border-mid-gray/40 bg-background px-3 py-2 text-sm disabled:opacity-50"
        >
          <option value="" disabled>
            {t("settings.models.cloud.openrouter.selectModel")}
          </option>
          {selectedModel && !modelAvailable && (
            <option value={selectedModel} disabled>
              {selectedModel}
            </option>
          )}
          {models.map((model) => (
            <option key={model.id} value={model.id}>
              {model.name}
            </option>
          ))}
        </select>
      </label>
      <button
        type="button"
        disabled={disabled || dirty || loading || !savedKey.trim()}
        onClick={() => setRevision((value) => value + 1)}
        className="rounded-lg border border-mid-gray/30 px-3 py-1.5 text-xs disabled:opacity-50"
      >
        {t(
          loading
            ? "settings.models.cloud.openrouter.loading"
            : "settings.models.cloud.openrouter.refresh",
        )}
      </button>
      {fetchError && (
        <p className="text-xs text-red-500" role="alert">
          {t("settings.models.cloud.openrouter.fetchError")}
        </p>
      )}
      {!loading &&
        !fetchError &&
        savedKey.trim() &&
        !keyUpdating &&
        models.length === 0 && (
          <p className="text-xs text-text/55">
            {t("settings.models.cloud.openrouter.empty")}
          </p>
        )}
      {error && (
        <p className="text-xs text-red-500" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
