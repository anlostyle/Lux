import { describe, expect, it } from "vitest";
import { describeMediaSources, distinctNameParts, sourceFileStem } from "../src/features/media/mediaSourceLabels";
import type { MediaSource } from "../src/lib/api/types";

const cloud = (file: string) => `/CloudNAS/CloudDrive/115/xjj/FC2系列/2026/FC2-4988506/${file}`;

describe("media source labels", () => {
  it("tells cd2 and cd3 apart from the file names and shows size and duration", () => {
    const sources: MediaSource[] = [
      { id: "a", container: "strm", sourceKind: "STRM_URL", externalUrl: cloud("FC2-4988506-无码-cd2.mp4"), size: 1_500_000_000, durationTicks: 36_000_000_000 },
      { id: "b", container: "strm", sourceKind: "STRM_URL", externalUrl: cloud("FC2-4988506-无码-cd3.mp4"), size: 1_200_000_000, durationTicks: 42_000_000_000 },
    ];
    const [first, second] = describeMediaSources(sources);
    expect(first.label).toBe("cd2");
    expect(second.label).toBe("cd3");
    expect(first.detail).toBe("1.4 GB · 1 小时 · strm");
    expect(second.detail).toContain("1.1 GB");
    expect(second.detail).toContain("1 小时 10 分钟");
  });

  it("prefers the edition or quality label when it separates the versions", () => {
    const sources: MediaSource[] = [
      { id: "a", editionName: "导演剪辑版", externalUrl: cloud("X-cd1.mp4") },
      { id: "b", editionName: "加长版", externalUrl: cloud("X-cd2.mp4") },
    ];
    expect(describeMediaSources(sources).map((entry) => entry.label)).toEqual(["导演剪辑版", "加长版"]);
    const sameQuality: MediaSource[] = [
      { id: "a", qualityLabel: "1080p", externalUrl: cloud("X-cd1.mp4") },
      { id: "b", qualityLabel: "1080p", externalUrl: cloud("X-cd2.mp4") },
    ];
    expect(describeMediaSources(sameQuality).map((entry) => entry.label)).toEqual(["cd1", "cd2"]);
  });

  it("keeps the stream-derived quality label from the caller when it is distinct", () => {
    const sources: MediaSource[] = [{ id: "a" }, { id: "b" }];
    const labels = describeMediaSources(sources, { qualityLabel: (source) => (source.id === "a" ? "1080p · H.264" : "2160p · HEVC") });
    expect(labels.map((entry) => entry.label)).toEqual(["1080p · H.264", "2160p · HEVC"]);
  });

  it("strips the shared prefix and suffix at word boundaries, including brackets and tmdb tags", () => {
    expect(distinctNameParts([
      "DSOD-041 (dsod00041)-有码-C",
      "DSOD-041 (dsod00041)-破解-C",
    ])).toEqual(["有码", "破解"]);
    expect(distinctNameParts([
      "男儿无罪 (1992) {tmdb-561377} - 480p",
      "男儿无罪 (1992) {tmdb-561377} - 4K",
    ])).toEqual(["480p", "4K"]);
    expect(distinctNameParts(["part9", "part10"])).toEqual(["part9", "part10"]);
  });

  it("falls back to numbered versions with the size when the names cannot tell them apart", () => {
    const same = cloud("same.mp4");
    const sources: MediaSource[] = [
      { id: "a", container: "mkv", externalUrl: same, size: 2_000_000_000 },
      { id: "b", container: "mkv", externalUrl: same, size: 3_000_000_000 },
    ];
    expect(describeMediaSources(sources).map((entry) => entry.label)).toEqual(["MKV · 版本 1 · 1.9 GB", "MKV · 版本 2 · 2.8 GB"]);
    expect(distinctNameParts(["same", "same"])).toBeUndefined();
    // a name that is the prefix of another one still tells the two apart
    expect(distinctNameParts(["X", "X-4K"])).toEqual(["X", "X-4K"]);
  });

  it("omits probe details that are missing instead of showing placeholders", () => {
    const sources: MediaSource[] = [
      { id: "a", container: "strm", externalUrl: cloud("A-cd1.mp4") },
      { id: "b", container: "strm", externalUrl: cloud("A-cd2.mp4"), size: 1024 * 1024 * 700 },
    ];
    const [first, second] = describeMediaSources(sources);
    expect(first.detail).toBe("strm");
    expect(second.detail).toBe("700 MB · strm");
    expect(`${first.detail}${second.detail}`).not.toMatch(/undefined|null|NaN|· ·/);
  });

  it("reads the file name from paths and URLs with odd characters", () => {
    expect(sourceFileStem({ id: "a", externalUrl: "/CloudNAS/x/男儿无罪 (1992) {tmdb-561377}.mkv" })).toBe("男儿无罪 (1992) {tmdb-561377}");
    expect(sourceFileStem({ id: "a", externalUrl: "https://host.example/a/%E7%94%B7-cd1.mp4?token=1" })).toBe("男-cd1");
    expect(sourceFileStem({ id: "a" })).toBeUndefined();
  });
});
