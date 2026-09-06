import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Pause, Play } from "lucide-react";
import { commands, type HistoryAudioPlaybackState } from "@/bindings";

interface NativeAudioPlayerProps {
  fileName: string;
  className?: string;
}

interface NativeAudioPlayerContextValue {
  playback: HistoryAudioPlaybackState;
  refreshPlayback: () => Promise<void>;
  isLoading: boolean;
  setIsLoading: (loading: boolean) => void;
}

const EMPTY_STATE: HistoryAudioPlaybackState = {
  file_name: null,
  is_playing: false,
  position_seconds: 0,
  duration_seconds: 0,
};

const NativeAudioPlayerContext =
  createContext<NativeAudioPlayerContextValue | null>(null);

export const NativeAudioPlayerGroup: React.FC<React.PropsWithChildren> = ({
  children,
}) => {
  const [playback, setPlayback] = useState(EMPTY_STATE);
  const [isLoading, setIsLoading] = useState(false);

  const refreshPlayback = useCallback(async () => {
    try {
      setPlayback(await commands.getHistoryAudioPlaybackState());
    } catch (error) {
      console.error("Failed to get history audio state:", error);
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;

    const poll = async () => {
      try {
        const state = await commands.getHistoryAudioPlaybackState();
        if (!cancelled) setPlayback(state);
      } catch (error) {
        console.error("Failed to get history audio state:", error);
      } finally {
        if (!cancelled) {
          timer = setTimeout(poll, playback.is_playing ? 100 : 400);
        }
      }
    };

    void poll();
    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
    };
  }, [playback.is_playing]);

  const value = useMemo(
    () => ({
      playback,
      refreshPlayback,
      isLoading,
      setIsLoading,
    }),
    [isLoading, playback, refreshPlayback],
  );

  return (
    <NativeAudioPlayerContext.Provider value={value}>
      {children}
    </NativeAudioPlayerContext.Provider>
  );
};

function useNativeAudioPlayback() {
  const context = useContext(NativeAudioPlayerContext);
  if (!context)
    throw new Error("NativeAudioPlayer requires NativeAudioPlayerGroup");
  return context;
}

export const NativeAudioPlayer: React.FC<NativeAudioPlayerProps> = ({
  fileName,
  className = "",
}) => {
  const { t } = useTranslation();
  const { playback, refreshPlayback, isLoading, setIsLoading } =
    useNativeAudioPlayback();
  const [dragPosition, setDragPosition] = useState<number | null>(null);
  const [isDragging, setIsDragging] = useState(false);
  const dragPositionRef = useRef(0);

  const togglePlay = async () => {
    if (isLoading) return;
    setIsLoading(true);
    try {
      const result =
        playback.file_name === fileName && playback.is_playing
          ? await commands.pauseHistoryAudio(fileName)
          : await commands.playHistoryAudio(fileName);
      if (result.status === "ok") {
        await refreshPlayback();
      } else {
        console.error("History audio playback failed:", result.error);
        toast.error(t("settings.history.audioPlaybackError"));
      }
    } catch (error) {
      console.error("History audio playback failed:", error);
      toast.error(t("settings.history.audioPlaybackError"));
    } finally {
      setIsLoading(false);
    }
  };

  const handleSeek = (event: React.ChangeEvent<HTMLInputElement>) => {
    const position = Number(event.target.value);
    dragPositionRef.current = position;
    setDragPosition(position);
  };

  const commitSeek = useCallback(async () => {
    if (!isDragging) return;
    setIsDragging(false);
    try {
      const result = await commands.seekHistoryAudio(
        fileName,
        dragPositionRef.current,
      );
      if (result.status === "ok") {
        setDragPosition(null);
        await refreshPlayback();
      } else {
        console.error("History audio seek failed:", result.error);
        toast.error(t("settings.history.audioPlaybackError"));
      }
    } catch (error) {
      console.error("History audio seek failed:", error);
      toast.error(t("settings.history.audioPlaybackError"));
    } finally {
      setDragPosition(null);
    }
  }, [fileName, isDragging, refreshPlayback, t]);

  useEffect(() => {
    if (!isDragging) return;
    document.addEventListener("mouseup", commitSeek);
    document.addEventListener("touchend", commitSeek);
    return () => {
      document.removeEventListener("mouseup", commitSeek);
      document.removeEventListener("touchend", commitSeek);
    };
  }, [commitSeek, isDragging]);

  useEffect(() => {
    return () => {
      void commands.stopHistoryAudio(fileName).catch(console.error);
    };
  }, [fileName]);

  const isActive = playback.file_name === fileName;
  const duration = isActive ? playback.duration_seconds : 0;
  const currentTime = Math.min(
    dragPosition ?? (isActive ? playback.position_seconds : 0),
    duration || Infinity,
  );
  const progressPercent =
    duration > 0
      ? Math.min(100, Math.max(0, (currentTime / duration) * 100))
      : 0;

  return (
    <div className={`flex items-center gap-3 ${className}`}>
      <button
        type="button"
        onClick={togglePlay}
        disabled={isLoading}
        className="transition-colors cursor-pointer text-text hover:text-logo-primary disabled:opacity-50"
        aria-label={t(
          isActive && playback.is_playing
            ? "settings.history.audioPause"
            : "settings.history.audioPlay",
        )}
      >
        {isActive && playback.is_playing ? (
          <Pause width={20} height={20} fill="currentColor" />
        ) : (
          <Play width={20} height={20} fill="currentColor" />
        )}
      </button>

      <div className="flex-1 flex items-center gap-2">
        <span className="text-xs text-text/60 min-w-[30px] tabular-nums">
          {formatTime(currentTime)}
        </span>
        <input
          aria-label={t("settings.history.audioSeek")}
          type="range"
          min="0"
          max={duration || 0}
          step="0.01"
          value={currentTime}
          onChange={handleSeek}
          onKeyDown={(event) => {
            if (
              [
                "ArrowLeft",
                "ArrowRight",
                "ArrowUp",
                "ArrowDown",
                "Home",
                "End",
                "PageUp",
                "PageDown",
              ].includes(event.key)
            ) {
              if (!isDragging) dragPositionRef.current = currentTime;
              setIsDragging(true);
            }
          }}
          onKeyUp={() => void commitSeek()}
          onBlur={() => void commitSeek()}
          onMouseDown={() => {
            dragPositionRef.current = currentTime;
            setIsDragging(true);
          }}
          onTouchStart={() => {
            dragPositionRef.current = currentTime;
            setIsDragging(true);
          }}
          disabled={duration <= 0}
          className="flex-1 h-1 rounded-lg appearance-none cursor-pointer focus:outline-none focus:ring-1 focus:ring-logo-primary disabled:cursor-default"
          style={{
            background: `linear-gradient(to right, #FAA2CA 0%, #FAA2CA ${progressPercent}%, rgba(128, 128, 128, 0.2) ${progressPercent}%, rgba(128, 128, 128, 0.2) 100%)`,
          }}
        />
        <span className="text-xs text-text/60 min-w-[30px] tabular-nums">
          {formatTime(duration)}
        </span>
      </div>
    </div>
  );
};

function formatTime(time: number): string {
  if (!Number.isFinite(time)) return "0:00";
  const minutes = Math.floor(time / 60);
  const seconds = Math.floor(time % 60);
  return `${minutes}:${seconds.toString().padStart(2, "0")}`;
}
