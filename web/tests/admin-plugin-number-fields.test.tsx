// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AdminPluginsPage } from "../src/features/admin/AdminPluginsPage";
import { api } from "../src/lib/api/client";
import type { AdminPlugin } from "../src/lib/api/types";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function mediaInfoPlugin(configValues: Record<string, unknown>): AdminPlugin {
  return {
    id: "org.lux.strm-media-info",
    name: "strm媒体信息提取",
    description: "提取媒体信息",
    category: "MEDIA",
    version: "0.2.8",
    runtime: "process",
    capabilities: ["media.probe"],
    status: "READY",
    running: true,
    lastError: null,
    installed: true,
    enabled: true,
    configured: true,
    available: true,
    configurable: true,
    configFields: [
      { key: "concurrency", label: "并发数", type: "number", required: true, sensitive: false, defaultValue: 2, minimum: 1, maximum: 64 },
      { key: "ffprobeTimeoutSeconds", label: "ffprobe 单次超时（秒）", type: "number", required: false, sensitive: false, defaultValue: 30, minimum: 10, maximum: 600, description: "冷读取网盘大文件建议调大。" },
      { key: "ffmpegTimeoutSeconds", label: "ffmpeg 截图超时（秒）", type: "number", required: false, sensitive: false, defaultValue: 60, minimum: 10, maximum: 900 },
      { key: "writeSidecars", label: "写入 mediainfo.json", type: "toggle", required: false, sensitive: false, defaultValue: true },
    ],
    configValues,
    configSource: "PLUGIN_CONFIG",
  } as AdminPlugin;
}

describe("AdminPluginsPage generic number fields", () => {
  let container: HTMLDivElement;
  let root: Root;

  async function openConfig(plugin: AdminPlugin) {
    vi.spyOn(api, "adminPlugins").mockResolvedValue({ plugins: [plugin] });
    vi.spyOn(api, "adminInstalledPlugins").mockResolvedValue({ plugins: [plugin] });
    vi.spyOn(api, "adminPluginStore").mockResolvedValue({ url: "https://example.com/index.json", defaultUrl: "https://example.com/index.json" });
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    await act(async () => {
      root.render(
        <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
          <MemoryRouter><AdminPluginsPage /></MemoryRouter>
        </QueryClientProvider>,
      );
    });
    await act(async () => {
      await vi.waitFor(() => expect(container.querySelector('button[aria-label^="配置 "]')).not.toBeNull());
    });
    await act(async () => {
      container.querySelector<HTMLButtonElement>('button[aria-label^="配置 "]')?.click();
    });
  }

  function setNumber(id: string, value: string) {
    const input = document.getElementById(id) as HTMLInputElement;
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set?.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  }

  beforeEach(() => {
    vi.spyOn(api, "updateAdminPluginConfig").mockResolvedValue({ plugin: {} as AdminPlugin });
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    vi.restoreAllMocks();
  });

  it("renders number fields the dialog has no dedicated control for, with limits and the saved value", async () => {
    await openConfig(mediaInfoPlugin({ concurrency: 2, ffprobeTimeoutSeconds: 90, ffmpegTimeoutSeconds: 120 }));

    const ffprobe = document.getElementById("plugin-config-org.lux.strm-media-info-ffprobeTimeoutSeconds") as HTMLInputElement;
    const ffmpeg = document.getElementById("plugin-config-org.lux.strm-media-info-ffmpegTimeoutSeconds") as HTMLInputElement;
    expect(ffprobe).not.toBeNull();
    expect(ffprobe.type).toBe("number");
    expect(ffprobe.min).toBe("10");
    expect(ffprobe.max).toBe("600");
    expect(ffprobe.value).toBe("90");
    expect(ffmpeg.value).toBe("120");
    expect(document.body.textContent).toContain("冷读取网盘大文件建议调大");
  });

  it("falls back to the manifest default when nothing is stored", async () => {
    await openConfig(mediaInfoPlugin({ concurrency: 2 }));
    expect((document.getElementById("plugin-config-org.lux.strm-media-info-ffprobeTimeoutSeconds") as HTMLInputElement).value).toBe("30");
    expect((document.getElementById("plugin-config-org.lux.strm-media-info-ffmpegTimeoutSeconds") as HTMLInputElement).value).toBe("60");
  });

  it("saves the edited value together with the stored ones and keeps untouched fields unchanged", async () => {
    await openConfig(mediaInfoPlugin({ concurrency: 2, ffprobeTimeoutSeconds: 90, ffmpegTimeoutSeconds: 120 }));
    await act(async () => { setNumber("plugin-config-org.lux.strm-media-info-ffprobeTimeoutSeconds", "100"); });
    await act(async () => {
      document.querySelector<HTMLFormElement>("form.lux-admin-plugin-dialog-form")?.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });
    expect(api.updateAdminPluginConfig).toHaveBeenCalledTimes(1);
    const body = vi.mocked(api.updateAdminPluginConfig).mock.calls[0][1] as Record<string, unknown>;
    expect(body.ffprobeTimeoutSeconds).toBe(100);
    expect(body.ffmpegTimeoutSeconds).toBe(120);
  });

  it("refuses to save a value outside the manifest range", async () => {
    await openConfig(mediaInfoPlugin({ concurrency: 2, ffprobeTimeoutSeconds: 90 }));
    await act(async () => { setNumber("plugin-config-org.lux.strm-media-info-ffprobeTimeoutSeconds", "5"); });
    const input = document.getElementById("plugin-config-org.lux.strm-media-info-ffprobeTimeoutSeconds") as HTMLInputElement;
    expect(input.getAttribute("aria-invalid")).toBe("true");
    const submit = document.querySelector<HTMLButtonElement>('form.lux-admin-plugin-dialog-form button[type="submit"]');
    expect(submit?.disabled).toBe(true);
    await act(async () => {
      document.querySelector<HTMLFormElement>("form.lux-admin-plugin-dialog-form")?.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });
    expect(api.updateAdminPluginConfig).not.toHaveBeenCalled();
  });
});
