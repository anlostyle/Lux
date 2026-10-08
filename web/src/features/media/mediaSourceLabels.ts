import type { MediaSource } from "../../lib/api/types";

export type MediaSourceDescription = {
  /** Main line: tells the versions of one item apart (cd2, 4K, 有码-C, …). */
  label: string;
  /** Secondary line: resolution/codec, size, duration; absent pieces are omitted. */
  detail: string;
};

type Options = {
  /** Quality text derived from the source's streams/quality label (e.g. "1080p · H.264"). */
  qualityLabel?: (source: MediaSource) => string;
};

const SEPARATORS = new Set([" ", "-", "_", ".", "(", ")", "[", "]", "{", "}", "【", "】", "（", "）", "·", "+", ",", "，"]);

/** File name (without extension) of the source's remote/local target, when the API provides it. */
export function sourceFileStem(source: MediaSource): string | undefined {
  const raw = source.externalUrl?.trim();
  if (!raw) return undefined;
  let path = raw;
  if (/^[a-z][a-z0-9+.-]*:\/\//i.test(raw)) {
    try {
      path = new URL(raw).pathname;
    } catch {
      return undefined;
    }
  }
  let name = path.split("/").filter(Boolean).pop();
  if (!name) return undefined;
  try {
    name = decodeURIComponent(name);
  } catch {
    // keep the raw name
  }
  const dot = name.lastIndexOf(".");
  const stem = dot > 0 && name.length - dot <= 6 ? name.slice(0, dot) : name;
  return stem.trim() || undefined;
}

/**
 * What is left of each name after removing the prefix and suffix they all share, cut at word
 * boundaries ("FC2-1-无码-cd2" / "…-cd3" → "cd2" / "cd3"). `undefined` when the names cannot be
 * told apart this way (identical names, or one name is a prefix of another).
 */
export function distinctNameParts(names: string[]): string[] | undefined {
  if (names.length < 2) return undefined;
  const chars = names.map((name) => Array.from(name));
  const shortest = Math.min(...chars.map((entry) => entry.length));
  let prefix = 0;
  while (prefix < shortest && chars.every((entry) => entry[prefix] === chars[0][prefix])) prefix += 1;
  while (prefix > 0 && !SEPARATORS.has(chars[0][prefix - 1])) prefix -= 1;
  const rest = chars.map((entry) => entry.slice(prefix));
  const restShortest = Math.min(...rest.map((entry) => entry.length));
  let suffix = 0;
  while (
    suffix < restShortest
    && rest.every((entry) => entry[entry.length - 1 - suffix] === rest[0][rest[0].length - 1 - suffix])
  ) suffix += 1;
  while (suffix > 0 && !SEPARATORS.has(rest[0][rest[0].length - suffix])) suffix -= 1;
  const parts = rest.map((entry) => trimSeparators(entry.slice(0, entry.length - suffix).join("")));
  if (parts.some((part) => !part) || new Set(parts).size !== parts.length) return undefined;
  return parts;
}

function trimSeparators(value: string): string {
  const characters = Array.from(value);
  let start = 0;
  let end = characters.length;
  while (start < end && SEPARATORS.has(characters[start])) start += 1;
  while (end > start && SEPARATORS.has(characters[end - 1])) end -= 1;
  return characters.slice(start, end).join("");
}

export function formatSourceSize(bytes?: number | null): string | undefined {
  if (!bytes || bytes <= 0) return undefined;
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${Math.round(bytes / 1024 ** 2)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

export function formatSourceDuration(durationTicks?: number | null): string | undefined {
  if (!durationTicks || durationTicks <= 0) return undefined;
  const minutes = Math.max(1, Math.round(durationTicks / 600_000_000));
  if (minutes < 60) return `${minutes} 分钟`;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest ? `${hours} 小时 ${rest} 分钟` : `${hours} 小时`;
}

function resolutionLabel(source: MediaSource): string | undefined {
  const video = source.streams?.find((stream) => (stream.type ?? "").toUpperCase() === "VIDEO");
  const height = Number(video?.details?.Height ?? video?.details?.height);
  if (!Number.isFinite(height) || height <= 0) return undefined;
  if (height >= 4000) return "8K";
  if (height >= 2000) return "4K";
  return `${height}p`;
}

function allDistinct(values: string[]): boolean {
  return values.every(Boolean) && new Set(values).size === values.length;
}

/**
 * Labels for every version of one item. Preference: the quality label derived from probed
 * streams, then the edition name, then the part of the file name that differs between versions
 * (cd2 / cd3, 4K, 破解), and only then "版本 N". Several versions never share a label.
 */
export function describeMediaSources(sources: MediaSource[], options: Options = {}): MediaSourceDescription[] {
  const quality = sources.map((source) => options.qualityLabel?.(source)?.trim() ?? "");
  const editions = sources.map((source) => source.editionName?.trim() || source.qualityLabel?.trim() || "");
  const stems = sources.map((source) => sourceFileStem(source));
  const fileParts = stems.every((stem): stem is string => Boolean(stem)) ? distinctNameParts(stems) : undefined;

  const labels = sources.map((source, index) => {
    if (sources.length < 2) return quality[index] || editions[index] || (source.container?.toUpperCase() ?? "视频");
    if (allDistinct(quality)) return quality[index];
    if (allDistinct(editions)) return editions[index];
    if (fileParts) return fileParts[index];
    return `${source.container?.toUpperCase() || "视频"} · 版本 ${index + 1}`;
  });

  return sources.map((source, index) => {
    const label = labels[index];
    const size = formatSourceSize(source.size);
    // Versions that only differ by number get their size appended so they can still be told apart.
    const numbered = label.includes("版本 ") && sources.length > 1;
    const detail = [
      quality[index] && quality[index] !== label ? quality[index] : resolutionLabel(source),
      size,
      formatSourceDuration(source.durationTicks),
      source.container && source.container.toLowerCase() === "strm" ? "strm" : undefined,
    ].filter((part, position, all): part is string => Boolean(part) && all.indexOf(part) === position);
    return { label: numbered && size ? `${label} · ${size}` : label, detail: detail.join(" · ") };
  });
}
