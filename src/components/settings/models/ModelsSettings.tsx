import React, { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ask } from "@tauri-apps/plugin-dialog";
import { ChevronDown, Cloud, Globe, RefreshCw, Search } from "lucide-react";
import type { ModelCardStatus } from "@/components/onboarding";
import { ModelCard } from "@/components/onboarding";
import { useModelStore } from "@/stores/modelStore";
import {
  getLanguageLabel,
  MODEL_CAPABILITY_LANGUAGES,
  supportsLanguageCode,
} from "@/lib/constants/languages.ts";
import type { ModelInfo } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";

// check if model supports a language based on its supported_languages list
const modelSupportsLanguage = (model: ModelInfo, langCode: string): boolean => {
  return supportsLanguageCode(model.supported_languages, langCode);
};

// Legacy models are the blob (Url-sourced) .bin/ONNX downloads, superseded by
// the catalog GGUFs. They stay runnable when already on disk, but we no longer
// advertise the download.
const isLegacyModel = (model: ModelInfo): boolean =>
  typeof model.source === "object" && "Url" in model.source;

export const ModelsSettings: React.FC = () => {
  const { t } = useTranslation();
  const {
    settings,
    isUpdating,
    setTranscriptionProvider,
    updateCodexAsrBaseUrl,
    updateTranscriptionApiKey,
  } = useSettings();
  const [codexBaseUrl, setCodexBaseUrl] = useState("");
  const [codexError, setCodexError] = useState<string | null>(null);
  const [elevenLabsApiKey, setElevenLabsApiKey] = useState("");
  const [elevenLabsError, setElevenLabsError] = useState<string | null>(null);
  const [switchingModelId, setSwitchingModelId] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [languageFilter, setLanguageFilter] = useState("all");
  const [languageDropdownOpen, setLanguageDropdownOpen] = useState(false);
  const [languageSearch, setLanguageSearch] = useState("");
  const languageDropdownRef = useRef<HTMLDivElement>(null);
  const languageSearchInputRef = useRef<HTMLInputElement>(null);
  const {
    models,
    currentModel,
    downloadingModels,
    downloadProgress,
    downloadStats,
    verifyingModels,
    extractingModels,
    loading,
    isRescanning,
    downloadModel,
    cancelDownload,
    selectModel,
    deleteModel,
    rescanLocalModels,
  } = useModelStore();

  const activeProvider = settings?.selected_transcription_provider ?? "local";
  const codexIsActive = activeProvider === "codex_asr";
  const elevenLabsIsActive = activeProvider === "elevenlabs_scribe";

  useEffect(() => {
    setCodexBaseUrl(settings?.codex_asr_base_url ?? "http://127.0.0.1:8788");
  }, [settings?.codex_asr_base_url]);

  useEffect(() => {
    setElevenLabsApiKey(
      settings?.transcription_api_keys?.elevenlabs_scribe ?? "",
    );
  }, [settings?.transcription_api_keys?.elevenlabs_scribe]);

  // click outside handler for language dropdown
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (
        languageDropdownRef.current &&
        !languageDropdownRef.current.contains(event.target as Node)
      ) {
        setLanguageDropdownOpen(false);
        setLanguageSearch("");
      }
    };
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, []);

  // focus search input when dropdown opens
  useEffect(() => {
    if (languageDropdownOpen && languageSearchInputRef.current) {
      languageSearchInputRef.current.focus();
    }
  }, [languageDropdownOpen]);

  // filtered languages for dropdown (exclude "auto")
  const filteredLanguages = useMemo(() => {
    return MODEL_CAPABILITY_LANGUAGES.filter((lang) =>
      lang.label.toLowerCase().includes(languageSearch.toLowerCase()),
    );
  }, [languageSearch]);

  // Get selected language label
  const selectedLanguageLabel = useMemo(() => {
    if (languageFilter === "all") {
      return t("settings.models.filters.allLanguages");
    }
    return getLanguageLabel(languageFilter) || "";
  }, [languageFilter, t]);

  const getModelStatus = (modelId: string): ModelCardStatus => {
    if (modelId in extractingModels) {
      return "extracting";
    }
    if (modelId in verifyingModels) {
      return "verifying";
    }
    if (modelId in downloadingModels) {
      return "downloading";
    }
    if (switchingModelId === modelId) {
      return "switching";
    }
    if (activeProvider === "local" && modelId === currentModel) {
      return "active";
    }
    const model = models.find((m: ModelInfo) => m.id === modelId);
    if (model?.is_downloaded) {
      return "available";
    }
    return "downloadable";
  };

  const getDownloadProgress = (modelId: string): number | undefined => {
    const progress = downloadProgress[modelId];
    return progress?.percentage;
  };

  const getDownloadSpeed = (modelId: string): number | undefined => {
    const stats = downloadStats[modelId];
    return stats?.speed;
  };

  const saveCodexBaseUrl = async (): Promise<boolean> => {
    try {
      await updateCodexAsrBaseUrl(codexBaseUrl);
      setCodexError(null);
      return true;
    } catch {
      setCodexError(t("settings.models.cloud.invalidUrl"));
      return false;
    }
  };

  const handleCodexSelect = async () => {
    if (!(await saveCodexBaseUrl())) return;
    try {
      await setTranscriptionProvider("codex_asr");
      setCodexError(null);
    } catch {
      setCodexError(t("modelSelector.providerError"));
    }
  };

  const saveElevenLabsApiKey = async (): Promise<boolean> => {
    try {
      await updateTranscriptionApiKey("elevenlabs_scribe", elevenLabsApiKey);
      if (!elevenLabsApiKey.trim()) {
        if (elevenLabsIsActive) await setTranscriptionProvider("local");
        setElevenLabsError(null);
        return false;
      }
      setElevenLabsError(null);
      return true;
    } catch {
      setElevenLabsError(t("settings.models.cloud.elevenlabs.keySaveError"));
      return false;
    }
  };

  const handleElevenLabsSelect = async () => {
    if (!(await saveElevenLabsApiKey())) return;
    try {
      await setTranscriptionProvider("elevenlabs_scribe");
      setElevenLabsError(null);
    } catch {
      setElevenLabsError(t("modelSelector.providerError"));
    }
  };

  const handleModelSelect = async (modelId: string) => {
    setSwitchingModelId(modelId);
    try {
      await selectModel(modelId);
    } finally {
      setSwitchingModelId(null);
    }
  };

  const handleModelDownload = async (modelId: string) => {
    await downloadModel(modelId);
  };

  const handleModelDelete = async (modelId: string) => {
    const model = models.find((m: ModelInfo) => m.id === modelId);
    const modelName = model?.name || modelId;
    const isActive = activeProvider === "local" && modelId === currentModel;

    const confirmed = await ask(
      isActive
        ? t("settings.models.deleteActiveConfirm", { modelName })
        : t("settings.models.deleteConfirm", { modelName }),
      {
        title: t("settings.models.deleteTitle"),
        kind: "warning",
      },
    );

    if (confirmed) {
      try {
        await deleteModel(modelId);
      } catch (err) {
        console.error(`Failed to delete model ${modelId}:`, err);
      }
    }
  };

  const handleModelCancel = async (modelId: string) => {
    try {
      await cancelDownload(modelId);
    } catch (err) {
      console.error(`Failed to cancel download for ${modelId}:`, err);
    }
  };

  // Filter models by search query (name + description) and language filter
  const filteredModels = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    return models.filter((model: ModelInfo) => {
      // Hide deprecated legacy (.bin/ONNX) downloads unless already on disk.
      if (isLegacyModel(model) && !model.is_downloaded) return false;
      if (languageFilter !== "all") {
        if (!modelSupportsLanguage(model, languageFilter)) return false;
      }
      if (q) {
        const haystack = `${model.name} ${model.description}`.toLowerCase();
        if (!haystack.includes(q)) return false;
      }
      return true;
    });
  }, [models, languageFilter, searchQuery]);

  // Split filtered models into downloaded (including custom) and available sections
  const { downloadedModels, availableModels } = useMemo(() => {
    const downloaded: ModelInfo[] = [];
    const available: ModelInfo[] = [];

    for (const model of filteredModels) {
      if (
        model.is_custom ||
        model.is_downloaded ||
        model.id in downloadingModels ||
        model.id in extractingModels
      ) {
        downloaded.push(model);
      } else {
        available.push(model);
      }
    }

    // Sort: active model first, then non-custom, then custom at the bottom
    downloaded.sort((a, b) => {
      if (a.id === currentModel) return -1;
      if (b.id === currentModel) return 1;
      if (a.is_custom !== b.is_custom) return a.is_custom ? 1 : -1;
      return 0;
    });

    return {
      downloadedModels: downloaded,
      availableModels: available,
    };
  }, [filteredModels, downloadingModels, extractingModels, currentModel]);

  if (loading) {
    return (
      <div className="max-w-3xl w-full mx-auto">
        <div className="flex items-center justify-center py-16">
          <div className="w-8 h-8 border-2 border-logo-primary border-t-transparent rounded-full animate-spin" />
        </div>
      </div>
    );
  }

  return (
    <div className="max-w-3xl w-full mx-auto space-y-4">
      <div className="mb-4">
        <h1 className="text-xl font-semibold mb-2">
          {t("settings.models.title")}
        </h1>
        <p className="text-sm text-text/60">
          {t("settings.models.description")}
        </p>
      </div>

      <section className="space-y-3 rounded-xl border border-mid-gray/30 bg-mid-gray/5 p-4">
        <div className="flex items-center gap-2">
          <Cloud className="h-4 w-4 text-logo-primary" />
          <h2 className="text-sm font-semibold">
            {t("settings.models.cloud.title")}
          </h2>
        </div>
        <div className="rounded-lg border border-mid-gray/30 bg-background p-4 space-y-3">
          <div className="flex items-start justify-between gap-4">
            <div>
              <div className="text-sm font-medium">
                {t("settings.models.cloud.codex.name")}
              </div>
              <p className="mt-1 text-xs text-text/55">
                {t("settings.models.cloud.codex.description")}
              </p>
            </div>
            <button
              type="button"
              onClick={() => void handleCodexSelect()}
              disabled={
                codexIsActive || isUpdating("selected_transcription_provider")
              }
              className={`rounded-lg px-3 py-1.5 text-xs font-medium transition-colors disabled:cursor-default ${
                codexIsActive
                  ? "bg-logo-primary/15 text-logo-primary"
                  : "bg-logo-primary text-white hover:bg-logo-primary/90 disabled:opacity-50"
              }`}
            >
              {codexIsActive
                ? t("settings.models.cloud.active")
                : t("settings.models.cloud.use")}
            </button>
          </div>
          <label className="block space-y-1.5">
            <span className="text-xs font-medium text-text/65">
              {t("settings.models.cloud.baseUrl")}
            </span>
            <input
              type="url"
              value={codexBaseUrl}
              onChange={(event) => setCodexBaseUrl(event.target.value)}
              onBlur={() => {
                if (codexIsActive) void saveCodexBaseUrl();
              }}
              disabled={isUpdating("codex_asr_base_url")}
              className="w-full rounded-lg border border-mid-gray/40 bg-mid-gray/10 px-3 py-2 text-sm focus:outline-none focus:ring-1 focus:ring-logo-primary disabled:opacity-50"
            />
          </label>
          {codexError && (
            <p className="text-xs text-red-500" role="alert">
              {codexError}
            </p>
          )}
          <p className="text-xs text-text/45">
            {t("settings.models.cloud.codex.setup")}
          </p>
        </div>

        <div className="rounded-lg border border-mid-gray/30 bg-background p-4 space-y-3">
          <div className="flex items-start justify-between gap-4">
            <div>
              <div className="text-sm font-medium">
                {t("settings.models.cloud.elevenlabs.name")}
              </div>
              <p className="mt-1 text-xs text-text/55">
                {t("settings.models.cloud.elevenlabs.description")}
              </p>
              <div className="mt-2 flex flex-wrap gap-1.5">
                <span className="rounded-full bg-mid-gray/15 px-2 py-0.5 text-[10px] font-medium text-text/60">
                  {t("settings.models.cloud.elevenlabs.model")}
                </span>
                <span className="rounded-full bg-logo-primary/10 px-2 py-0.5 text-[10px] font-medium text-logo-primary">
                  {t("settings.models.cloud.elevenlabs.audioEvents")}
                </span>
              </div>
            </div>
            <button
              type="button"
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => void handleElevenLabsSelect()}
              disabled={
                elevenLabsIsActive ||
                !elevenLabsApiKey.trim() ||
                isUpdating("selected_transcription_provider")
              }
              className={`rounded-lg px-3 py-1.5 text-xs font-medium transition-colors disabled:cursor-default ${
                elevenLabsIsActive
                  ? "bg-logo-primary/15 text-logo-primary"
                  : "bg-logo-primary text-white hover:bg-logo-primary/90 disabled:opacity-50"
              }`}
            >
              {elevenLabsIsActive
                ? t("settings.models.cloud.active")
                : t("settings.models.cloud.use")}
            </button>
          </div>
          <label className="block space-y-1.5">
            <span className="text-xs font-medium text-text/65">
              {t("settings.models.cloud.apiKey")}
            </span>
            <input
              type="password"
              value={elevenLabsApiKey}
              onChange={(event) => setElevenLabsApiKey(event.target.value)}
              onBlur={() => void saveElevenLabsApiKey()}
              autoComplete="off"
              disabled={isUpdating("transcription_api_key:elevenlabs_scribe")}
              className="w-full rounded-lg border border-mid-gray/40 bg-mid-gray/10 px-3 py-2 text-sm focus:outline-none focus:ring-1 focus:ring-logo-primary disabled:opacity-50"
            />
          </label>
          {elevenLabsError && (
            <p className="text-xs text-red-500" role="alert">
              {elevenLabsError}
            </p>
          )}
        </div>
      </section>

      <div className="pt-2">
        <h2 className="text-sm font-semibold">
          {t("settings.models.local.title")}
        </h2>
      </div>

      {/* Search bar — filter the local catalog by name or description */}
      <div className="relative">
        <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-text/40 pointer-events-none" />
        <input
          type="text"
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          placeholder={t("settings.models.searchPlaceholder")}
          className="w-full pl-9 pr-3 py-2 text-sm bg-mid-gray/10 border border-mid-gray/40 rounded-lg focus:outline-none focus:ring-1 focus:ring-logo-primary placeholder:text-text/40"
        />
      </div>

      {filteredModels.length > 0 ? (
        <div className="space-y-6">
          {/* Downloaded Models Section — header always visible so filter stays accessible */}
          <div className="space-y-3">
            <div className="flex items-center justify-between">
              <h2 className="text-sm font-medium text-text/60">
                {t("settings.models.yourModels")}
              </h2>
              <div className="flex items-center gap-2">
                {/* Rescan local sources for models added outside Handy */}
                <button
                  type="button"
                  onClick={() => rescanLocalModels()}
                  disabled={isRescanning}
                  title={t("settings.models.rescan.tooltip")}
                  className="flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-lg bg-mid-gray/10 text-text/60 hover:bg-mid-gray/20 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  <RefreshCw
                    className={`w-3.5 h-3.5 ${isRescanning ? "animate-spin" : ""}`}
                  />
                  <span>{t("settings.models.rescan.label")}</span>
                </button>
                {/* Language filter dropdown */}
                <div className="relative" ref={languageDropdownRef}>
                  <button
                    type="button"
                    onClick={() =>
                      setLanguageDropdownOpen(!languageDropdownOpen)
                    }
                    className={`flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-lg transition-colors ${
                      languageFilter !== "all"
                        ? "bg-logo-primary/20 text-logo-primary"
                        : "bg-mid-gray/10 text-text/60 hover:bg-mid-gray/20"
                    }`}
                  >
                    <Globe className="w-3.5 h-3.5" />
                    <span className="max-w-[120px] truncate">
                      {selectedLanguageLabel}
                    </span>
                    <ChevronDown
                      className={`w-3.5 h-3.5 transition-transform ${
                        languageDropdownOpen ? "rotate-180" : ""
                      }`}
                    />
                  </button>

                  {languageDropdownOpen && (
                    <div className="absolute top-full right-0 mt-1 w-56 bg-background border border-mid-gray/80 rounded-lg shadow-lg z-50 overflow-hidden">
                      <div className="p-2 border-b border-mid-gray/40">
                        <input
                          ref={languageSearchInputRef}
                          type="text"
                          value={languageSearch}
                          onChange={(e) => setLanguageSearch(e.target.value)}
                          onKeyDown={(e) => {
                            if (
                              e.key === "Enter" &&
                              filteredLanguages.length > 0
                            ) {
                              setLanguageFilter(filteredLanguages[0].value);
                              setLanguageDropdownOpen(false);
                              setLanguageSearch("");
                            } else if (e.key === "Escape") {
                              setLanguageDropdownOpen(false);
                              setLanguageSearch("");
                            }
                          }}
                          placeholder={t(
                            "settings.general.language.searchPlaceholder",
                          )}
                          className="w-full px-2 py-1 text-sm bg-mid-gray/10 border border-mid-gray/40 rounded-md focus:outline-none focus:ring-1 focus:ring-logo-primary"
                        />
                      </div>
                      <div className="max-h-48 overflow-y-auto">
                        <button
                          type="button"
                          onClick={() => {
                            setLanguageFilter("all");
                            setLanguageDropdownOpen(false);
                            setLanguageSearch("");
                          }}
                          className={`w-full px-3 py-1.5 text-sm text-left transition-colors ${
                            languageFilter === "all"
                              ? "bg-logo-primary/20 text-logo-primary font-semibold"
                              : "hover:bg-mid-gray/10"
                          }`}
                        >
                          {t("settings.models.filters.allLanguages")}
                        </button>
                        {filteredLanguages.map((lang) => (
                          <button
                            key={lang.value}
                            type="button"
                            onClick={() => {
                              setLanguageFilter(lang.value);
                              setLanguageDropdownOpen(false);
                              setLanguageSearch("");
                            }}
                            className={`w-full px-3 py-1.5 text-sm text-left transition-colors ${
                              languageFilter === lang.value
                                ? "bg-logo-primary/20 text-logo-primary font-semibold"
                                : "hover:bg-mid-gray/10"
                            }`}
                          >
                            {lang.label}
                          </button>
                        ))}
                        {filteredLanguages.length === 0 && (
                          <div className="px-3 py-2 text-sm text-text/50 text-center">
                            {t("settings.general.language.noResults")}
                          </div>
                        )}
                      </div>
                    </div>
                  )}
                </div>
              </div>
            </div>
            {downloadedModels.map((model: ModelInfo) => (
              <ModelCard
                key={model.id}
                model={model}
                status={getModelStatus(model.id)}
                onSelect={handleModelSelect}
                onDownload={handleModelDownload}
                onDelete={handleModelDelete}
                onCancel={handleModelCancel}
                downloadProgress={getDownloadProgress(model.id)}
                downloadSpeed={getDownloadSpeed(model.id)}
                showRecommended={false}
              />
            ))}
          </div>

          {/* Available Models Section */}
          {availableModels.length > 0 && (
            <div className="space-y-3">
              <h2 className="text-sm font-medium text-text/60">
                {t("settings.models.availableModels")}
              </h2>
              {availableModels.map((model: ModelInfo) => (
                <ModelCard
                  key={model.id}
                  model={model}
                  status={getModelStatus(model.id)}
                  onSelect={handleModelSelect}
                  onDownload={handleModelDownload}
                  onDelete={handleModelDelete}
                  onCancel={handleModelCancel}
                  downloadProgress={getDownloadProgress(model.id)}
                  downloadSpeed={getDownloadSpeed(model.id)}
                  showRecommended={true}
                />
              ))}
            </div>
          )}
        </div>
      ) : (
        <div className="text-center py-8 text-text/50">
          {t("settings.models.noModelsMatch")}
        </div>
      )}
    </div>
  );
};
