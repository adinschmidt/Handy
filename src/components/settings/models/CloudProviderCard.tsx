import React, { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, ChevronDown } from "lucide-react";
import type { TranscriptionProvider } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import Badge from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";

interface CloudProviderCardProps {
  provider: Exclude<TranscriptionProvider, "local">;
  name: string;
  description: string;
  /** Short saved-state detail shown in the header, such as the model. */
  summary?: string;
  /** Saved settings are complete enough to select this provider. */
  configured: boolean;
  /** Setup fields and options. They unmount on collapse, discarding drafts. */
  children: React.ReactNode;
}

export const CloudProviderCard: React.FC<CloudProviderCardProps> = ({
  provider,
  name,
  description,
  summary,
  configured,
  children,
}) => {
  const { t } = useTranslation();
  const { settings, isUpdating, setTranscriptionProvider } = useSettings();
  const [open, setOpen] = useState(false);
  const [selectFailed, setSelectFailed] = useState(false);
  const panelId = useId();
  const active = settings?.selected_transcription_provider === provider;

  const select = async () => {
    setSelectFailed(false);
    try {
      await setTranscriptionProvider(provider);
    } catch {
      setSelectFailed(true);
    }
  };

  return (
    <div
      className={`rounded-xl border-2 transition-colors ${
        active
          ? "border-logo-primary/50 bg-logo-primary/10"
          : "border-mid-gray/20"
      }`}
    >
      <div className="flex items-center gap-3 px-4 py-3">
        <button
          type="button"
          onClick={() => setOpen((value) => !value)}
          aria-expanded={open}
          aria-controls={panelId}
          className="group flex min-w-0 flex-1 items-center gap-2 rounded-md text-start focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
        >
          <ChevronDown
            className={`h-4 w-4 shrink-0 text-text/50 transition-transform ${
              open ? "" : "-rotate-90 rtl:rotate-90"
            }`}
          />
          <span className="text-base font-semibold transition-colors group-hover:text-logo-primary">
            {name}
          </span>
          {active ? (
            <Badge variant="primary">
              <Check className="me-1 h-3 w-3" />
              {t("settings.models.cloud.active")}
            </Badge>
          ) : (
            !configured && (
              <Badge variant="secondary">
                {t("settings.models.cloud.needsSetup")}
              </Badge>
            )
          )}
          {summary && (
            <span className="ms-auto truncate ps-2 text-xs text-text/50">
              {summary}
            </span>
          )}
        </button>
        {configured && !active && (
          <Button
            variant="primary-soft"
            size="sm"
            onClick={() => void select()}
            disabled={isUpdating("selected_transcription_provider")}
          >
            {t("settings.models.cloud.use")}
          </Button>
        )}
      </div>
      {selectFailed && (
        <p role="alert" className="px-4 pb-3 text-xs text-red-500">
          {t("modelSelector.providerError")}
        </p>
      )}
      {open && (
        <div id={panelId}>
          <p className="px-4 pb-3 text-sm text-text/60">{description}</p>
          <div className="divide-y divide-mid-gray/20 border-t border-mid-gray/20">
            {children}
          </div>
        </div>
      )}
    </div>
  );
};

/** A labeled field. Controls fill a fixed-width column so rows line up. */
export const CloudProviderRow: React.FC<{
  label: string;
  children: React.ReactNode;
}> = ({ label, children }) => (
  <label className="flex min-h-12 items-center justify-between gap-4 px-4 py-2">
    <span className="text-sm font-medium">{label}</span>
    <span className="flex w-72 shrink-0 items-center gap-2">{children}</span>
  </label>
);

/** Notes, actions, and status messages below a provider's fields. */
export const CloudProviderFooter: React.FC<{ children: React.ReactNode }> = ({
  children,
}) => (
  <div className="space-y-2 px-4 py-3 text-xs text-text/55">{children}</div>
);
