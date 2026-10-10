// @vitest-environment jsdom

import { act } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { api } from "../src/lib/api/client";
import { normalizeVersionPriorityRule } from "../src/features/media/VersionPriorityEditor";
import {
  LibraryVersionPrioritySettings,
  UserVersionPrioritySettings,
} from "../src/features/media/VersionPrioritySettings";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let container: HTMLDivElement | null = null;

async function render(element: React.ReactNode) {
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  await act(async () => {
    root?.render(<QueryClientProvider client={queryClient}>{element}</QueryClientProvider>);
  });
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  return container;
}

afterEach(() => {
  if (root) act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
  vi.restoreAllMocks();
});

describe("version priority rules", () => {
  it("normalizes rules before saving", () => {
    expect(normalizeVersionPriorityRule({ mode: "quality", custom: { keywordGroups: [["x"]], subtitle: "ignore", subtitleKeywords: [], tieBreakers: [] } })).toEqual({ mode: "quality" });
    expect(
      normalizeVersionPriorityRule({
        mode: "custom",
        custom: { keywordGroups: [["Extended", ""], []], subtitle: "prefer", subtitleKeywords: ["", "subbed"], tieBreakers: ["resolution"] },
      }),
    ).toEqual({
      mode: "custom",
      custom: { keywordGroups: [["Extended"]], subtitle: "prefer", subtitleKeywords: ["subbed"], tieBreakers: ["resolution"] },
    });
  });

  it("shows the stored library rule with its keyword groups and saves it", async () => {
    const rule = {
      mode: "custom" as const,
      custom: { keywordGroups: [["Directors Cut"], ["Extended", "IMAX"]], subtitle: "ignore" as const, subtitleKeywords: [], tieBreakers: ["resolution" as const, "size" as const] },
    };
    vi.spyOn(api, "adminLibraryVersionPriority").mockResolvedValue({ libraryId: "library-1", rule });
    const update = vi.spyOn(api, "updateAdminLibraryVersionPriority").mockResolvedValue({ libraryId: "library-1", rule });
    const view = await render(<LibraryVersionPrioritySettings libraryId="library-1" />);

    const groups = [...view.querySelectorAll<HTMLInputElement>(".lux-version-priority-keywords")].map((input) => input.value);
    expect(groups).toEqual(["Directors Cut", "Extended, IMAX"]);

    await act(async () => {
      [...view.querySelectorAll("button")].find((button) => button.textContent?.includes("下移") || button.getAttribute("aria-label") === "下移第 1 组")?.click();
    });
    const reordered = [...view.querySelectorAll<HTMLInputElement>(".lux-version-priority-keywords")].map((input) => input.value);
    expect(reordered).toEqual(["Extended, IMAX", "Directors Cut"]);

    await act(async () => {
      [...view.querySelectorAll("button")].find((button) => button.textContent?.includes("保存多版本优先"))?.click();
    });
    expect(update).toHaveBeenCalledWith("library-1", {
      mode: "custom",
      custom: { keywordGroups: [["Extended", "IMAX"], ["Directors Cut"]], subtitle: "ignore", subtitleKeywords: [], tieBreakers: ["resolution", "size"] },
    });
  });

  it("hides the user settings without permission", async () => {
    vi.spyOn(api, "versionPriority").mockResolvedValue({ canCustomize: false, rules: {} });
    const view = await render(<UserVersionPrioritySettings libraries={[]} />);
    expect(view.querySelector("[data-version-priority=user]")).toBeNull();
  });

  it("saves the user's rule for the chosen scope", async () => {
    vi.spyOn(api, "versionPriority").mockResolvedValue({ canCustomize: true, rules: { "*": { mode: "quality" } } });
    const update = vi.spyOn(api, "updateVersionPriority").mockResolvedValue({ canCustomize: true, rules: { "*": { mode: "quality" } } });
    const view = await render(<UserVersionPrioritySettings libraries={[{ id: "library-1", name: "电影" }]} />);
    expect(view.querySelector("[data-version-priority-mode=quality]")).not.toBeNull();
    await act(async () => {
      [...view.querySelectorAll("button")].find((button) => button.textContent?.includes("保存版本优先"))?.click();
    });
    expect(update).toHaveBeenCalledWith("*", { mode: "quality" });
  });
});
