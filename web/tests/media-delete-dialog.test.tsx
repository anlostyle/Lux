// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MediaDeleteDialog } from "../src/features/media/MediaDeleteDialog";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

describe("MediaDeleteDialog", () => {
  let container: HTMLDivElement;
  let root: Root;

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  it("warns that deleting a series removes every episode", () => {
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);

    act(() => {
      root.render(
        <MediaDeleteDialog
          item={{ id: "series-1", title: "示例剧集", itemType: "SERIES" }}
          onClose={() => undefined}
          onConfirm={async () => undefined}
        />,
      );
    });

    expect(container.textContent).toContain("整部剧及所有分集");
    expect(container.textContent).toContain("所有季度和分集的视频文件");
  });

  it("keeps the current-version warning for a single media item", () => {
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);

    act(() => {
      root.render(
        <MediaDeleteDialog
          item={{ id: "movie-1", title: "示例电影", itemType: "MOVIE" }}
          onClose={() => undefined}
          onConfirm={async () => undefined}
        />,
      );
    });

    expect(container.textContent).toContain("当前视频版本");
    expect(container.textContent).not.toContain("所有季度和分集的视频文件");
  });

  const twoVersions = [
    { id: "source-1", editionName: "cd1", container: "strm", size: 1_500_000_000, isDefault: true },
    { id: "source-2", editionName: "cd2", container: "strm", size: 1_200_000_000 },
  ];

  function mount(element: React.ReactElement) {
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    act(() => root.render(element));
  }

  async function click(selector: string) {
    await act(async () => {
      (container.querySelector(selector) as HTMLElement).click();
    });
  }

  it("names the version being deleted and offers a delete-all option for multi-version items", async () => {
    const onConfirm = vi.fn(async () => undefined);
    const onDeleted = vi.fn();
    mount(
      <MediaDeleteDialog
        item={{ id: "movie-2", title: "FC2-4979551", itemType: "MOVIE", mediaSources: twoVersions }}
        targetSourceId="source-2"
        onClose={() => undefined}
        onConfirm={onConfirm}
        onDeleted={onDeleted}
      />,
    );

    expect(container.textContent).toContain("共有 2 个视频版本");
    expect(container.textContent).toContain("仅删除此版本：cd2");
    expect(container.textContent).toContain("删除全部 2 个版本");
    expect(container.textContent).not.toContain("当前视频版本");

    await click('[data-action="delete-confirm"]');
    expect(onConfirm).toHaveBeenCalledWith({ mode: "source", sourceId: "source-2" });
    expect(onDeleted).toHaveBeenCalledWith({
      mode: "source", sourceId: "source-2", versionLabel: "cd2（1.1 GB）", remaining: 1,
    });
  });

  it("deletes every version when the user picks the delete-all scope", async () => {
    const onConfirm = vi.fn(async () => undefined);
    const onDeleted = vi.fn();
    mount(
      <MediaDeleteDialog
        item={{ id: "movie-2", title: "FC2-4979551", itemType: "MOVIE", mediaSources: twoVersions }}
        targetSourceId="source-1"
        onClose={() => undefined}
        onConfirm={onConfirm}
        onDeleted={onDeleted}
      />,
    );

    await click('input[value="all"]');
    await click('[data-action="delete-confirm"]');
    expect(onConfirm).toHaveBeenCalledWith({ mode: "all" });
    expect(onDeleted).toHaveBeenCalledWith({ mode: "all", sourceId: undefined, versionLabel: undefined, remaining: 0 });
  });

  it("keeps the single-version wording and deletes that version's source", async () => {
    const onConfirm = vi.fn(async () => undefined);
    const onDeleted = vi.fn();
    mount(
      <MediaDeleteDialog
        item={{ id: "movie-3", title: "示例电影", itemType: "MOVIE", mediaSources: [twoVersions[0]] }}
        targetSourceId="source-1"
        onClose={() => undefined}
        onConfirm={onConfirm}
        onDeleted={onDeleted}
      />,
    );

    expect(container.textContent).toContain("当前视频版本");
    expect(container.querySelector('input[name="lux-delete-scope"]')).toBeNull();
    await click('[data-action="delete-confirm"]');
    expect(onConfirm).toHaveBeenCalledWith({ mode: "source", sourceId: "source-1" });
    expect(onDeleted).toHaveBeenCalledWith(expect.objectContaining({ remaining: 0 }));
  });

  it("shows the server error and stays open when a stale version cannot be deleted", async () => {
    const onClose = vi.fn();
    mount(
      <MediaDeleteDialog
        item={{ id: "movie-2", title: "FC2-4979551", itemType: "MOVIE", mediaSources: twoVersions }}
        targetSourceId="source-1"
        onClose={onClose}
        onConfirm={async () => { throw new Error("媒体文件不存在"); }}
      />,
    );

    await click('[data-action="delete-confirm"]');
    expect(container.textContent).toContain("媒体文件不存在");
    expect(onClose).not.toHaveBeenCalled();
  });
});
