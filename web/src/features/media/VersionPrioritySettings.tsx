import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Check } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { LuxSelect } from "../../components/LuxSelect";
import { api } from "../../lib/api/client";
import type { VersionPriorityRule } from "../../lib/api/types";
import {
  VersionPriorityEditor,
  VersionPriorityPreviewPanel,
  normalizeVersionPriorityRule,
} from "./VersionPriorityEditor";

const ALL_LIBRARIES = "*";

function errorMessage(error: unknown, fallback: string) {
  return error instanceof Error ? error.message : fallback;
}

/** The signed-in user's own version priority, for every library or one library. */
export function UserVersionPrioritySettings({
  libraries,
}: {
  libraries: Array<{ id: string; name: string }>;
}) {
  const queryClient = useQueryClient();
  const settings = useQuery({ queryKey: ["version-priority", "me"], queryFn: () => api.versionPriority() });
  const [scope, setScope] = useState(ALL_LIBRARIES);
  const [rule, setRule] = useState<VersionPriorityRule>({ mode: "inherit" });
  const [notice, setNotice] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!settings.data) return;
    setRule(settings.data.rules?.[scope] ?? { mode: "inherit" });
  }, [scope, settings.data]);

  const scopeOptions = useMemo(
    () => [
      { value: ALL_LIBRARIES, label: "全部媒体库" },
      ...libraries.map((library) => ({ value: library.id, label: library.name })),
    ],
    [libraries],
  );

  if (settings.isPending) return <p className="lux-setting-hint">正在加载版本优先设置…</p>;
  if (settings.error) return <p className="lux-error-copy" role="alert">版本优先设置暂时无法加载：{settings.error.message}</p>;
  if (!settings.data?.canCustomize) return null;

  const save = async () => {
    setSaving(true);
    setNotice(null);
    try {
      const updated = await api.updateVersionPriority(scope, normalizeVersionPriorityRule(rule));
      queryClient.setQueryData(["version-priority", "me"], updated);
      setNotice("已保存，新的版本顺序会在下次打开条目时生效。");
    } catch (error) {
      setNotice(errorMessage(error, "保存失败"));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="lux-version-priority-settings" data-version-priority="user">
      <label className="lux-setting-field">
        <span>适用范围</span>
        <LuxSelect value={scope} options={scopeOptions} aria-label="版本优先适用范围" onChange={setScope} />
      </label>
      <p className="lux-setting-hint">
        {scope === ALL_LIBRARIES
          ? "作用于所有媒体库；单个媒体库的设置会覆盖这里。"
          : "只作用于这个媒体库；选“跟随媒体库设置”时使用“全部媒体库”的设置或管理员的媒体库设置。"}
      </p>
      <VersionPriorityEditor value={rule} onChange={setRule} allowInherit disabled={saving} />
      <div className="lux-setting-actions">
        <button type="button" className="lux-button lux-button-compact" disabled={saving} onClick={() => void save()}>
          保存版本优先
        </button>
      </div>
      {notice ? <p className="lux-account-notice" role="status">{notice}</p> : null}
      <VersionPriorityPreviewPanel
        preview={(itemId) =>
          api.previewVersionPriority(itemId, rule.mode === "inherit" ? undefined : normalizeVersionPriorityRule(rule))
        }
      />
    </div>
  );
}

/** Administrator setting of one library. */
export function LibraryVersionPrioritySettings({ libraryId }: { libraryId: string }) {
  const settings = useQuery({
    queryKey: ["version-priority", "library", libraryId],
    queryFn: () => api.adminLibraryVersionPriority(libraryId),
  });
  const [rule, setRule] = useState<VersionPriorityRule>({ mode: "default" });
  const [notice, setNotice] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (settings.data) setRule(settings.data.rule ?? { mode: "default" });
  }, [settings.data]);

  if (settings.isPending) return <p className="lux-setting-hint">正在加载多版本优先设置…</p>;
  if (settings.error) return <p className="lux-error-copy" role="alert">多版本优先设置暂时无法加载：{settings.error.message}</p>;

  const save = async () => {
    setSaving(true);
    setNotice(null);
    try {
      const updated = await api.updateAdminLibraryVersionPriority(libraryId, normalizeVersionPriorityRule(rule));
      setRule(updated.rule ?? { mode: "default" });
      setNotice("已保存，后台正在按新规则更新默认版本。");
      await settings.refetch();
    } catch (error) {
      setNotice(errorMessage(error, "保存失败"));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="lux-version-priority-settings" data-version-priority="library">
      <p className="lux-setting-hint">决定一个条目有多个版本时默认播放哪一个、版本的排列顺序，以及列表上显示哪个版本的技术信息。用户可以在个人设置里覆盖。</p>
      <VersionPriorityEditor value={rule} onChange={setRule} disabled={saving} />
      <div className="lux-setting-actions">
        <button type="button" className="lux-button lux-button-compact" disabled={saving} onClick={() => void save()}>
          保存多版本优先
        </button>
      </div>
      {notice ? <p className="lux-account-notice" role="status">{notice}</p> : null}
      <VersionPriorityPreviewPanel
        preview={(itemId) => api.previewAdminLibraryVersionPriority(libraryId, itemId, normalizeVersionPriorityRule(rule))}
      />
    </div>
  );
}

/** Administrator switch that lets one user customize version priority. */
export function UserVersionPriorityPermission({ userId, isAdmin }: { userId: string; isAdmin: boolean }) {
  const queryClient = useQueryClient();
  const settings = useQuery({
    queryKey: ["version-priority", "user", userId],
    queryFn: () => api.adminUserVersionPriority(userId),
    enabled: !isAdmin,
  });
  const [saving, setSaving] = useState(false);
  // Administrators can always customize their own version priority.
  if (isAdmin) return null;
  const checked = settings.data?.canCustomize ?? true;
  return (
    <label className="lux-admin-permission-toggle">
      <input
        type="checkbox"
        checked={checked}
        disabled={saving || settings.isPending}
        onChange={async (event) => {
          setSaving(true);
          try {
            const updated = await api.updateAdminUserVersionPriority(userId, event.target.checked);
            queryClient.setQueryData(["version-priority", "user", userId], {
              canCustomize: updated.canCustomize,
              rules: settings.data?.rules ?? {},
            });
          } finally {
            setSaving(false);
          }
        }}
      />
      <span>{checked ? <Check size={13} /> : null}</span>
      自定义版本优先
    </label>
  );
}

/** Library dialog section; the settings load only when the administrator opens it. */
export function LibraryVersionPrioritySection({ libraryId }: { libraryId: string }) {
  const [open, setOpen] = useState(false);
  return (
    <section className="lux-library-dialog-section lux-library-version-priority-section">
      <div className="lux-library-dialog-section-heading">
        <h3>多版本优先</h3>
        <button
          className="lux-library-toolbar-button"
          type="button"
          aria-expanded={open}
          onClick={() => setOpen((value) => !value)}
        >
          {open ? "收起" : "设置"}
        </button>
      </div>
      {open ? <LibraryVersionPrioritySettings libraryId={libraryId} /> : null}
    </section>
  );
}
