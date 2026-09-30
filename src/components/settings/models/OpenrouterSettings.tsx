import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { RefreshCcw } from "lucide-react";
import { commands, type OpenrouterModel } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { ResetButton } from "@/components/ui/ResetButton";
import { Select } from "@/components/ui/Select";
import { ApiKeyField } from "../PostProcessingSettingsApi/ApiKeyField";
import {
  CloudProviderCard,
  CloudProviderFooter,
  CloudProviderRow,
} from "./CloudProviderCard";

export function OpenrouterSettings() {
  const { t } = useTranslation();
  const { settings } = useSettings();
  const model = settings?.openrouter_model ?? "";

  return (
    <CloudProviderCard
      provider="openrouter"
      name={t("settings.models.cloud.openrouter.name")}
      description={t("settings.models.cloud.openrouter.description")}
      summary={model || undefined}
      configured={Boolean(
        settings?.transcription_api_keys?.openrouter?.trim() && model.trim(),
      )}
    >
      <OpenrouterFields />
    </CloudProviderCard>
  );
}

function OpenrouterFields() {
  const { t } = useTranslation();
  const { settings, isUpdating, updateTranscriptionApiKey, refreshSettings } =
    useSettings();
  const savedKey = settings?.transcription_api_keys?.openrouter ?? "";
  const selectedModel = settings?.openrouter_model ?? "";
  const keyUpdating = isUpdating("transcription_api_key:openrouter");
  const hasKey = savedKey.trim() !== "";
  const [models, setModels] = useState<OpenrouterModel[]>([]);
  const [loading, setLoading] = useState(false);
  const [fetchError, setFetchError] = useState(false);
  const [saveError, setSaveError] = useState(false);
  const [savingModel, setSavingModel] = useState(false);
  const [revision, setRevision] = useState(0);

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

  const saveApiKey = async (apiKey: string) => {
    if (apiKey === savedKey) return;
    setSaveError(false);
    try {
      await updateTranscriptionApiKey("openrouter", apiKey);
    } catch {
      setSaveError(true);
    }
  };

  const saveModel = async (model: string) => {
    setSaveError(false);
    setSavingModel(true);
    try {
      const result = await commands.changeOpenrouterModel(model);
      if (result.status === "error") throw new Error(result.error);
      await refreshSettings();
    } catch {
      setSaveError(true);
    } finally {
      setSavingModel(false);
    }
  };

  return (
    <>
      <CloudProviderRow label={t("settings.models.cloud.apiKey")}>
        <ApiKeyField
          value={savedKey}
          onBlur={(apiKey) => void saveApiKey(apiKey)}
          disabled={keyUpdating}
        />
      </CloudProviderRow>
      <CloudProviderRow label={t("settings.models.cloud.openrouter.model")}>
        <Select
          value={selectedModel || null}
          options={models.map(({ id, name }) => ({ value: id, label: name }))}
          placeholder={t("settings.models.cloud.openrouter.selectModel")}
          isClearable={false}
          isLoading={loading}
          disabled={
            !hasKey || keyUpdating || savingModel || models.length === 0
          }
          onChange={(model) => {
            if (model) void saveModel(model);
          }}
          className="min-w-0 flex-1 text-sm"
        />
        <ResetButton
          onClick={() => setRevision((value) => value + 1)}
          disabled={!hasKey || keyUpdating || loading}
          ariaLabel={t("settings.models.cloud.openrouter.refresh")}
          className="flex h-10 w-10 shrink-0 items-center justify-center"
        >
          <RefreshCcw className={`h-4 w-4 ${loading ? "animate-spin" : ""}`} />
        </ResetButton>
      </CloudProviderRow>
      <CloudProviderFooter>
        <p>{t("settings.models.cloud.openrouter.storage")}</p>
        {fetchError && (
          <p role="alert" className="text-red-500">
            {t("settings.models.cloud.openrouter.fetchError")}
          </p>
        )}
        {!loading &&
          !fetchError &&
          hasKey &&
          !keyUpdating &&
          models.length === 0 && (
            <p>{t("settings.models.cloud.openrouter.empty")}</p>
          )}
        {saveError && (
          <p role="alert" className="text-red-500">
            {t("settings.models.cloud.openrouter.saveError")}
          </p>
        )}
      </CloudProviderFooter>
    </>
  );
}
