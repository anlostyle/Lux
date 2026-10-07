import { LoaderCircle, Trash2, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { MediaItem, MediaSource } from "../../lib/api/types";
import { mediaTitle } from "../home/media";
import "./MediaDeleteDialog.css";

/** What the user asked to delete: one version (source) or every version of the item. */
export type MediaDeleteChoice = { mode: "source"; sourceId: string } | { mode: "all" };

export type MediaDeleteResult = {
  mode: "source" | "all";
  sourceId?: string;
  /** Human readable name of the deleted version, when the item has several. */
  versionLabel?: string;
  /** Versions left on the item after the deletion (0 once the item itself is gone). */
  remaining: number;
};

type MediaDeleteDialogProps = {
  item: MediaItem;
  /** Version the surrounding page is showing; preselected for deletion. */
  targetSourceId?: string;
  /** Overrides `item.mediaSources`, e.g. after some versions were already deleted. */
  sources?: MediaSource[];
  onClose: () => void;
  onConfirm: (choice?: MediaDeleteChoice) => Promise<void>;
  onDeleted?: (result: MediaDeleteResult) => void;
};

export function mediaSourceLabel(source: MediaSource, index: number): string {
  const name = source.editionName?.trim() || source.qualityLabel?.trim() || `版本 ${index + 1}`;
  const details = [
    source.editionName?.trim() && source.qualityLabel?.trim() ? source.qualityLabel.trim() : undefined,
    source.container?.trim() && source.container.trim().toLowerCase() !== "strm" ? source.container.trim().toUpperCase() : undefined,
    source.size ? formatSourceSize(source.size) : undefined,
    source.durationTicks ? `${Math.max(1, Math.round(source.durationTicks / 600_000_000))} 分钟` : undefined,
  ].filter(Boolean);
  return details.length ? `${name}（${details.join(" · ")}）` : name;
}

function formatSourceSize(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${Math.round(bytes / 1024 ** 2)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

export function MediaDeleteDialog({ item, targetSourceId, sources: sourcesOverride, onClose, onConfirm, onDeleted }: MediaDeleteDialogProps) {
  const closeRef = useRef<HTMLButtonElement>(null);
  const [deleting, setDeleting] = useState(false);
  const [error, setError] = useState<string>();
  const isSeries = item.itemType === "SERIES";
  const sources = sourcesOverride ?? item.mediaSources ?? [];
  const hasVersions = !isSeries && sources.length >= 2;
  const targetIndex = Math.max(0, sources.findIndex((source) => source.id === targetSourceId));
  const target = sources[targetIndex] as MediaSource | undefined;
  const targetLabel = target ? mediaSourceLabel(target, targetIndex) : undefined;
  const [scope, setScope] = useState<"source" | "all">("source");

  useEffect(() => {
    closeRef.current?.focus();
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    const closeOnEscape = (event: KeyboardEvent) => {
      if (!deleting && event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("keydown", closeOnEscape);
      document.body.style.overflow = previousOverflow;
    };
  }, [deleting, onClose]);

  async function confirm() {
    setDeleting(true);
    setError(undefined);
    try {
      const deleteAll = hasVersions && scope === "all";
      const choice: MediaDeleteChoice | undefined = deleteAll
        ? { mode: "all" }
        : target ? { mode: "source", sourceId: target.id } : undefined;
      await onConfirm(choice);
      onDeleted?.({
        mode: choice?.mode === "source" ? "source" : "all",
        sourceId: choice?.mode === "source" ? choice.sourceId : undefined,
        versionLabel: hasVersions && !deleteAll ? targetLabel : undefined,
        remaining: choice?.mode === "source" ? Math.max(0, sources.length - 1) : 0,
      });
      onClose();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "删除失败，请重试。");
    } finally {
      setDeleting(false);
    }
  }

  return (
    <div className="lux-media-editor-backdrop" role="presentation" onMouseDown={(event) => { if (!deleting && event.target === event.currentTarget) onClose(); }}>
      <section className="lux-media-editor lux-delete-dialog" role="alertdialog" aria-modal="true" aria-labelledby="lux-delete-title" aria-describedby="lux-delete-description">
        <header className="lux-media-editor-header">
          <div>
            <h2 id="lux-delete-title">删除媒体</h2>
          </div>
          <button ref={closeRef} className="lux-media-editor-close" type="button" aria-label="关闭删除确认" disabled={deleting} onClick={onClose}><X size={18} /></button>
        </header>
        <div className="lux-delete-dialog-body">
          <div className="lux-delete-dialog-icon" aria-hidden="true"><Trash2 size={26} /></div>
          <p id="lux-delete-description">
            {isSeries
              ? `确定要删除“${mediaTitle(item)}”整部剧及所有分集吗？`
              : hasVersions
                ? `“${mediaTitle(item)}”共有 ${sources.length} 个视频版本，请选择要删除的范围。`
                : `确定要删除“${mediaTitle(item)}”的当前视频版本吗？`}
          </p>
          {hasVersions ? (
            <fieldset className="lux-delete-dialog-scope" disabled={deleting}>
              <legend>删除范围</legend>
              <label>
                <input type="radio" name="lux-delete-scope" value="source" checked={scope === "source"} onChange={() => setScope("source")} />
                <span>仅删除此版本：{targetLabel}</span>
              </label>
              <label>
                <input type="radio" name="lux-delete-scope" value="all" checked={scope === "all"} onChange={() => setScope("all")} />
                <span>删除全部 {sources.length} 个版本</span>
              </label>
            </fieldset>
          ) : null}
          <small>{isSeries ? "整部剧下所有季度和分集的视频文件，以及同名的字幕、NFO 和图片旁车文件都会被删除。这个操作无法撤销。" : "视频文件以及同名的字幕、NFO 和图片旁车文件都会被删除。这个操作无法撤销。"}</small>
          {error ? <p className="lux-editor-error" role="alert">{error}</p> : null}
          <div className="lux-delete-dialog-actions">
            <button className="lux-button lux-button-secondary" type="button" disabled={deleting} onClick={onClose}>取消</button>
            <button className="lux-button lux-button-danger" data-action="delete-confirm" type="button" disabled={deleting} onClick={() => void confirm()}>
              {deleting ? <LoaderCircle className="lux-spin" size={16} /> : <Trash2 size={16} />}
              {deleting ? "删除中…" : "确认删除"}
            </button>
          </div>
        </div>
      </section>
    </div>
  );
}
