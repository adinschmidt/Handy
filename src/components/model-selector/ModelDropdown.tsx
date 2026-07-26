import React from "react";
import { useTranslation } from "react-i18next";
import type { ModelInfo } from "@/bindings";
import {
  getTranslatedModelName,
  getTranslatedModelDescription,
} from "../../lib/utils/modelTranslation";

interface ModelDropdownProps {
  models: ModelInfo[];
  currentModelId: string;
  activeProvider: "local" | "codex_asr";
  onModelSelect: (modelId: string) => void;
  onProviderSelect: (provider: "codex_asr") => void;
}

const ModelDropdown: React.FC<ModelDropdownProps> = ({
  models,
  currentModelId,
  activeProvider,
  onModelSelect,
  onProviderSelect,
}) => {
  const { t } = useTranslation();
  const downloadedModels = models.filter((m) => m.is_downloaded);

  const handleModelClick = (modelId: string) => {
    onModelSelect(modelId);
  };

  return (
    <div className="absolute bottom-full start-0 mb-2 w-64 max-h-[60vh] overflow-y-auto bg-background border border-mid-gray/20 rounded-lg shadow-lg py-2 z-50">
      <div className="px-3 pb-1 text-[10px] font-semibold uppercase tracking-wide text-text/40">
        {t("settings.models.local.title")}
      </div>
      {downloadedModels.length > 0 ? (
        <div>
          {downloadedModels.map((model) => {
            const isActive =
              activeProvider === "local" && currentModelId === model.id;
            return (
              <div
                key={model.id}
                onClick={() => handleModelClick(model.id)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    handleModelClick(model.id);
                  }
                }}
                tabIndex={0}
                role="button"
                className={`w-full px-3 py-2 text-start hover:bg-mid-gray/10 transition-colors cursor-pointer focus:outline-none ${
                  isActive ? "bg-logo-primary/10 text-logo-primary" : ""
                }`}
              >
                <div className="flex items-center justify-between">
                  <div>
                    <div className="text-sm text-text/80">
                      {getTranslatedModelName(model, t)}
                      {model.is_custom && (
                        <span className="ms-1.5 text-[10px] font-medium text-text/40 uppercase">
                          {t("modelSelector.custom")}
                        </span>
                      )}
                      {model.supports_streaming && (
                        <span className="ms-1.5 text-[10px] font-medium text-logo-primary/70 uppercase">
                          {t("modelSelector.streaming")}
                        </span>
                      )}
                    </div>
                    <div className="text-xs text-text/40 italic pe-4">
                      {getTranslatedModelDescription(model, t)}
                    </div>
                  </div>
                  {isActive && (
                    <div className="text-xs text-logo-primary">
                      {t("modelSelector.active")}
                    </div>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      ) : (
        <div className="px-3 py-2 text-sm text-text/60">
          {t("modelSelector.noModelsAvailable")}
        </div>
      )}

      <div className="my-1 border-t border-mid-gray/20" />
      <div className="px-3 pb-1 pt-1 text-[10px] font-semibold uppercase tracking-wide text-text/40">
        {t("settings.models.cloud.title")}
      </div>
      <button
        type="button"
        onClick={() => onProviderSelect("codex_asr")}
        className={`w-full px-3 py-2 text-start hover:bg-mid-gray/10 transition-colors ${
          activeProvider === "codex_asr"
            ? "bg-logo-primary/10 text-logo-primary"
            : ""
        }`}
      >
        <div className="flex items-center justify-between">
          <div>
            <div className="text-sm text-text/80">
              {t("settings.models.cloud.codex.name")}
            </div>
            <div className="text-xs text-text/40 italic pe-4">
              {t("settings.models.cloud.codex.shortDescription")}
            </div>
          </div>
          {activeProvider === "codex_asr" && (
            <div className="text-xs text-logo-primary">
              {t("modelSelector.active")}
            </div>
          )}
        </div>
      </button>
    </div>
  );
};

export default ModelDropdown;
