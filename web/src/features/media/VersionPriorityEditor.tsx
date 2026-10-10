import { ChevronDown, ChevronUp, GripVertical, Plus, Search, Trash2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { LuxSelect } from "../../components/LuxSelect";
import "./VersionPriorityEditor.css";
import { api } from "../../lib/api/client";
import type {
  MediaItem,
  VersionPriorityMode,
  VersionPriorityPreview,
  VersionPriorityRule,
  VersionPrioritySubtitle,
  VersionPriorityTieBreaker,
} from "../../lib/api/types";

const tieBreakerLabels: Record<VersionPriorityTieBreaker, string> = {
  resolution: "分辨率",
  hdr: "HDR / 杜比视界",
  codec: "视频编码",
  bitrate: "码率",
  size: "文件大小",
};
const allTieBreakers: VersionPriorityTieBreaker[] = ["resolution", "hdr", "codec", "bitrate", "size"];
const defaultTieBreakers: VersionPriorityTieBreaker[] = ["resolution", "hdr", "bitrate", "size"];

type EditableCustom = NonNullable<VersionPriorityRule["custom"]>;

function emptyCustom(): EditableCustom {
  return { keywordGroups: [], subtitle: "ignore", subtitleKeywords: [], tieBreakers: [...defaultTieBreakers] };
}

function splitKeywords(value: string) {
  return value
    .split(/[,，、]/)
    .map((keyword) => keyword.trim())
    .filter(Boolean);
}

/** Drops empty keyword groups and the custom block for non-custom modes before saving. */
export function normalizeVersionPriorityRule(rule: VersionPriorityRule): VersionPriorityRule {
  if (rule.mode !== "custom") return { mode: rule.mode };
  const custom = rule.custom ?? emptyCustom();
  return {
    mode: "custom",
    custom: {
      keywordGroups: custom.keywordGroups.map((group) => group.filter(Boolean)).filter((group) => group.length),
      subtitle: custom.subtitle,
      subtitleKeywords: custom.subtitleKeywords.filter(Boolean),
      tieBreakers: custom.tieBreakers,
    },
  };
}

export function VersionPriorityEditor({
  value,
  onChange,
  allowInherit = false,
  disabled = false,
}: {
  value: VersionPriorityRule;
  onChange: (rule: VersionPriorityRule) => void;
  allowInherit?: boolean;
  disabled?: boolean;
}) {
  const custom = { ...emptyCustom(), ...value.custom };
  const [draggedGroup, setDraggedGroup] = useState<number | null>(null);
  // Keep the raw text of each keyword group so commas can be typed freely.
  const [groupDrafts, setGroupDrafts] = useState<string[]>(() => custom.keywordGroups.map((group) => group.join(", ")));
  useEffect(() => {
    setGroupDrafts(custom.keywordGroups.map((group) => group.join(", ")));
  }, [value.mode, custom.keywordGroups.length]);

  const modeOptions = useMemo(
    () => [
      ...(allowInherit ? [{ value: "inherit", label: "跟随媒体库设置" }] : []),
      { value: "default", label: "保持默认（先入库的版本优先）" },
      { value: "quality", label: "画质优先（分辨率 → HDR → 码率 → 大小）" },
      { value: "custom", label: "自定义" },
    ],
    [allowInherit],
  );

  const setCustom = (next: Partial<EditableCustom>) => onChange({ mode: "custom", custom: { ...custom, ...next } });
  const setGroups = (groups: string[][], drafts?: string[]) => {
    if (drafts) setGroupDrafts(drafts);
    setCustom({ keywordGroups: groups });
  };
  const moveGroup = (from: number, to: number) => {
    if (to < 0 || to >= custom.keywordGroups.length || from === to) return;
    const groups = [...custom.keywordGroups];
    const drafts = [...groupDrafts];
    const [group] = groups.splice(from, 1);
    const [draft] = drafts.splice(from, 1);
    groups.splice(to, 0, group);
    drafts.splice(to, 0, draft ?? group.join(", "));
    setGroups(groups, drafts);
  };
  const toggleTieBreaker = (tieBreaker: VersionPriorityTieBreaker) => {
    const enabled = custom.tieBreakers.includes(tieBreaker);
    setCustom({
      tieBreakers: enabled
        ? custom.tieBreakers.filter((candidate) => candidate !== tieBreaker)
        : [...custom.tieBreakers, tieBreaker],
    });
  };
  const moveTieBreaker = (tieBreaker: VersionPriorityTieBreaker, offset: number) => {
    const index = custom.tieBreakers.indexOf(tieBreaker);
    const target = index + offset;
    if (index < 0 || target < 0 || target >= custom.tieBreakers.length) return;
    const tieBreakers = [...custom.tieBreakers];
    tieBreakers.splice(index, 1);
    tieBreakers.splice(target, 0, tieBreaker);
    setCustom({ tieBreakers });
  };

  return (
    <div className="lux-version-priority-editor" data-version-priority-mode={value.mode}>
      <label className="lux-setting-field">
        <span>版本优先方式</span>
        <LuxSelect
          value={value.mode}
          options={modeOptions}
          disabled={disabled}
          aria-label="版本优先方式"
          onChange={(mode) =>
            onChange(mode === "custom" ? { mode: "custom", custom } : { mode: mode as VersionPriorityMode })
          }
        />
      </label>
      {value.mode === "custom" ? (
        <>
          <div className="lux-setting-block">
            <div className="lux-setting-block-heading">
              <div>
                <strong>版本名关键词</strong>
                <p>靠前的一组优先；同一组里的词用逗号分隔，优先级相同。例如：导演剪辑版、加长版、IMAX。</p>
              </div>
              <span className="lux-setting-hint">可拖拽排序</span>
            </div>
            <div className="lux-account-library-list" role="list" aria-label="版本名关键词顺序">
              {custom.keywordGroups.map((group, index) => (
                <div
                  className="lux-account-library-row"
                  key={index}
                  role="listitem"
                  draggable={!disabled}
                  onDragStart={() => setDraggedGroup(index)}
                  onDragEnd={() => setDraggedGroup(null)}
                  onDragOver={(event) => event.preventDefault()}
                  onDrop={() => {
                    if (draggedGroup !== null) moveGroup(draggedGroup, index);
                    setDraggedGroup(null);
                  }}
                >
                  <GripVertical className="lux-drag-handle" size={17} aria-hidden="true" />
                  <div className="lux-account-library-index" aria-hidden="true">{String(index + 1).padStart(2, "0")}</div>
                  <input
                    className="lux-version-priority-keywords"
                    aria-label={`第 ${index + 1} 组关键词`}
                    value={groupDrafts[index] ?? group.join(", ")}
                    disabled={disabled}
                    onChange={(event) => {
                      const drafts = [...groupDrafts];
                      drafts[index] = event.target.value;
                      const groups = [...custom.keywordGroups];
                      groups[index] = splitKeywords(event.target.value);
                      setGroups(groups, drafts);
                    }}
                  />
                  <div className="lux-account-library-actions">
                    <button type="button" aria-label={`上移第 ${index + 1} 组`} disabled={disabled || index === 0} onClick={() => moveGroup(index, index - 1)}><ChevronUp size={16} /></button>
                    <button type="button" aria-label={`下移第 ${index + 1} 组`} disabled={disabled || index === custom.keywordGroups.length - 1} onClick={() => moveGroup(index, index + 1)}><ChevronDown size={16} /></button>
                    <button
                      type="button"
                      aria-label={`删除第 ${index + 1} 组`}
                      disabled={disabled}
                      onClick={() => setGroups(
                        custom.keywordGroups.filter((_, candidate) => candidate !== index),
                        groupDrafts.filter((_, candidate) => candidate !== index),
                      )}
                    ><Trash2 size={16} /></button>
                  </div>
                </div>
              ))}
            </div>
            <button
              type="button"
              className="lux-button lux-button-compact lux-button-secondary"
              disabled={disabled || custom.keywordGroups.length >= 32}
              onClick={() => setGroups([...custom.keywordGroups, []], [...groupDrafts, ""])}
            >
              <Plus size={16} aria-hidden="true" /> 添加一组关键词
            </button>
          </div>
          <div className="lux-setting-form-grid">
            <label className="lux-setting-field">
              <span>字幕</span>
              <LuxSelect
                value={custom.subtitle}
                aria-label="字幕偏好"
                disabled={disabled}
                options={[
                  { value: "ignore", label: "不考虑" },
                  { value: "prefer", label: "优先带字幕的版本" },
                  { value: "avoid", label: "优先不带字幕的版本" },
                ]}
                onChange={(subtitle) => setCustom({ subtitle: subtitle as VersionPrioritySubtitle })}
              />
            </label>
            <label className="lux-setting-field">
              <span>表示带字幕的版本名关键词（可选，逗号分隔）</span>
              <input
                value={custom.subtitleKeywords.join(", ")}
                disabled={disabled || custom.subtitle === "ignore"}
                onChange={(event) => setCustom({ subtitleKeywords: splitKeywords(event.target.value) })}
              />
            </label>
          </div>
          <div className="lux-setting-block">
            <div className="lux-setting-block-heading">
              <div>
                <strong>画质兜底</strong>
                <p>关键词和字幕都相同时，依次比较勾选的项目。</p>
              </div>
            </div>
            <div className="lux-version-priority-tie-breakers" role="group" aria-label="画质兜底顺序">
              {[...custom.tieBreakers, ...allTieBreakers.filter((tieBreaker) => !custom.tieBreakers.includes(tieBreaker))].map((tieBreaker) => {
                const position = custom.tieBreakers.indexOf(tieBreaker);
                return (
                  <div className="lux-version-priority-tie-breaker" key={tieBreaker}>
                    <label>
                      <input
                        type="checkbox"
                        checked={position >= 0}
                        disabled={disabled}
                        onChange={() => toggleTieBreaker(tieBreaker)}
                      />
                      <span>{tieBreakerLabels[tieBreaker]}</span>
                    </label>
                    {position >= 0 ? (
                      <span className="lux-account-library-actions">
                        <button type="button" aria-label={`上移${tieBreakerLabels[tieBreaker]}`} disabled={disabled || position === 0} onClick={() => moveTieBreaker(tieBreaker, -1)}><ChevronUp size={16} /></button>
                        <button type="button" aria-label={`下移${tieBreakerLabels[tieBreaker]}`} disabled={disabled || position === custom.tieBreakers.length - 1} onClick={() => moveTieBreaker(tieBreaker, 1)}><ChevronDown size={16} /></button>
                      </span>
                    ) : null}
                  </div>
                );
              })}
            </div>
          </div>
        </>
      ) : null}
    </div>
  );
}

/** Pick an item and show how its versions would be ordered by the rule being edited. */
export function VersionPriorityPreviewPanel({
  preview,
}: {
  preview: (itemId: string) => Promise<VersionPriorityPreview>;
}) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<MediaItem[]>([]);
  const [result, setResult] = useState<VersionPriorityPreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const search = async () => {
    if (!query.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const page = await api.search(query.trim());
      const items = page.items ?? [];
      setResults(items.slice(0, 6));
      if (!items.length) setError("没有找到条目");
    } catch (searchError) {
      setError(searchError instanceof Error ? searchError.message : "搜索失败");
    } finally {
      setBusy(false);
    }
  };
  const choose = async (itemId: string) => {
    setBusy(true);
    setError(null);
    try {
      setResult(await preview(itemId));
    } catch (previewError) {
      setError(previewError instanceof Error ? previewError.message : "预览失败");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="lux-setting-block lux-version-priority-preview">
      <div className="lux-setting-block-heading">
        <div>
          <strong>预览</strong>
          <p>选一个有多个版本的条目，查看按当前设置排出的版本顺序。</p>
        </div>
      </div>
      <form
        className="lux-version-priority-search"
        onSubmit={(event) => {
          event.preventDefault();
          void search();
        }}
      >
        <input value={query} placeholder="搜索条目" aria-label="搜索预览条目" onChange={(event) => setQuery(event.target.value)} />
        <button type="submit" className="lux-button lux-button-compact lux-button-secondary" disabled={busy}>
          <Search size={16} aria-hidden="true" /> 搜索
        </button>
      </form>
      {results.length ? (
        <div className="lux-version-priority-results" role="list" aria-label="预览条目">
          {results.map((item) => (
            <button type="button" role="listitem" key={item.id} onClick={() => void choose(item.id)} disabled={busy}>
              {item.title}
            </button>
          ))}
        </div>
      ) : null}
      {error ? <p className="lux-error-copy" role="alert">{error}</p> : null}
      {result ? (
        <ol className="lux-version-priority-order" aria-label={`${result.title} 的版本顺序`}>
          {result.sources.map((source) => (
            <li key={source.id} data-default={source.isDefault ? "true" : undefined}>
              <strong>{source.editionName || source.qualityLabel || source.fileName || source.id}</strong>
              {source.partIndex ? <span>分段 {source.partIndex}</span> : null}
              {source.isDefault ? <span className="lux-version-priority-default">默认播放</span> : null}
            </li>
          ))}
        </ol>
      ) : null}
    </div>
  );
}
