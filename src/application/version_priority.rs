//! Version priority: which of an item's media sources (versions) plays by default and in
//! which order the versions are listed.
//!
//! Rules come in three layers, the first one that applies wins:
//! 1. the user's own rule for the item's library, then the user's rule for all libraries
//!    (only when the user may customize version priority);
//! 2. the library rule set by an administrator;
//! 3. the stored order (oldest source first), which is also what happens without rules.
//!
//! Rules only use generic properties (resolution, HDR, codec, bitrate, size, subtitles and
//! keywords the administrator or user types in); Lux ships no keywords of its own.

use std::{cmp::Ordering, collections::HashMap};

use serde::{Deserialize, Serialize};

use crate::application::catalog::{CatalogItem, CatalogSource};

/// Scope id of a user rule that applies to every library.
pub const ALL_LIBRARIES_SCOPE: &str = "*";

const MAX_KEYWORD_GROUPS: usize = 32;
const MAX_KEYWORDS_PER_GROUP: usize = 32;
const MAX_KEYWORD_CHARS: usize = 64;
const MAX_SUBTITLE_KEYWORDS: usize = 16;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum VersionPriorityMode {
    /// Keep the stored order (oldest source first).
    #[default]
    Default,
    /// Resolution, then HDR, then bitrate, then file size.
    Quality,
    /// Administrator- or user-defined keywords, subtitle preference and tie breakers.
    Custom,
    /// User rules only: follow the library rule.
    Inherit,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SubtitlePreference {
    Prefer,
    Avoid,
    #[default]
    Ignore,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TieBreaker {
    Resolution,
    Hdr,
    Codec,
    Bitrate,
    Size,
}

const QUALITY_TIE_BREAKERS: [TieBreaker; 4] = [
    TieBreaker::Resolution,
    TieBreaker::Hdr,
    TieBreaker::Bitrate,
    TieBreaker::Size,
];

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustomVersionPriority {
    /// Ordered keyword groups: a version matching an earlier group wins; words in one
    /// group rank the same. Matched against the version label, case-insensitively.
    #[serde(default)]
    pub keyword_groups: Vec<Vec<String>>,
    #[serde(default)]
    pub subtitle: SubtitlePreference,
    /// Extra words in the version label that mean "has subtitles", besides subtitle tracks.
    #[serde(default)]
    pub subtitle_keywords: Vec<String>,
    /// Applied in order after keywords and subtitles. Empty means resolution, HDR,
    /// bitrate, size.
    #[serde(default)]
    pub tie_breakers: Vec<TieBreaker>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VersionPriorityRule {
    #[serde(default)]
    pub mode: VersionPriorityMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom: Option<CustomVersionPriority>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum VersionPriorityError {
    Invalid(String),
}

impl std::fmt::Display for VersionPriorityError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for VersionPriorityError {}

impl VersionPriorityRule {
    /// Checks limits and normalizes keywords (trimmed, empty ones dropped).
    pub fn validated(mut self, allow_inherit: bool) -> Result<Self, VersionPriorityError> {
        if self.mode == VersionPriorityMode::Inherit && !allow_inherit {
            return Err(VersionPriorityError::Invalid(
                "inherit is only available for user rules".to_owned(),
            ));
        }
        if self.mode != VersionPriorityMode::Custom {
            self.custom = None;
            return Ok(self);
        }
        let mut custom = self.custom.take().unwrap_or_default();
        if custom.keyword_groups.len() > MAX_KEYWORD_GROUPS {
            return Err(VersionPriorityError::Invalid(format!(
                "at most {MAX_KEYWORD_GROUPS} keyword groups are allowed"
            )));
        }
        custom.keyword_groups = custom
            .keyword_groups
            .into_iter()
            .map(|group| normalize_keywords(group, MAX_KEYWORDS_PER_GROUP))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|group| !group.is_empty())
            .collect();
        custom.subtitle_keywords =
            normalize_keywords(custom.subtitle_keywords, MAX_SUBTITLE_KEYWORDS)?;
        let mut seen = Vec::new();
        for tie_breaker in &custom.tie_breakers {
            if seen.contains(tie_breaker) {
                return Err(VersionPriorityError::Invalid(
                    "tie breakers must not repeat".to_owned(),
                ));
            }
            seen.push(*tie_breaker);
        }
        self.custom = Some(custom);
        Ok(self)
    }

    /// Whether this rule changes anything compared with the stored order.
    pub fn is_active(&self) -> bool {
        matches!(
            self.mode,
            VersionPriorityMode::Quality | VersionPriorityMode::Custom
        )
    }
}

fn normalize_keywords(
    keywords: Vec<String>,
    limit: usize,
) -> Result<Vec<String>, VersionPriorityError> {
    if keywords.len() > limit {
        return Err(VersionPriorityError::Invalid(format!(
            "at most {limit} keywords are allowed per group"
        )));
    }
    let mut normalized = Vec::with_capacity(keywords.len());
    for keyword in keywords {
        let keyword = keyword.trim();
        if keyword.is_empty() {
            continue;
        }
        if keyword.chars().count() > MAX_KEYWORD_CHARS {
            return Err(VersionPriorityError::Invalid(format!(
                "keywords are limited to {MAX_KEYWORD_CHARS} characters"
            )));
        }
        if !normalized
            .iter()
            .any(|existing: &String| existing == keyword)
        {
            normalized.push(keyword.to_owned());
        }
    }
    Ok(normalized)
}

/// The rules of one user.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UserVersionPriority {
    pub can_customize: bool,
    /// Keyed by library id or [`ALL_LIBRARIES_SCOPE`].
    pub rules: HashMap<String, VersionPriorityRule>,
}

/// In-memory copy of every stored rule, replaced as a whole after each write.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct VersionPrioritySnapshot {
    pub library_rules: HashMap<String, VersionPriorityRule>,
    /// Users without an entry may customize and have no rules.
    pub users: HashMap<String, UserVersionPriority>,
}

impl VersionPrioritySnapshot {
    pub fn user_can_customize(&self, user_id: &str, is_admin: bool) -> bool {
        is_admin
            || self
                .users
                .get(user_id)
                .is_none_or(|user| user.can_customize)
    }

    /// Resolves the rule that applies to `library_id` for `user` (`None` = no user).
    pub fn resolve(
        &self,
        user: Option<(&str, bool)>,
        library_id: &str,
    ) -> Option<&VersionPriorityRule> {
        if let Some((user_id, is_admin)) = user
            && self.user_can_customize(user_id, is_admin)
            && let Some(user_rules) = self.users.get(user_id)
        {
            for scope in [library_id, ALL_LIBRARIES_SCOPE] {
                if let Some(rule) = user_rules.rules.get(scope)
                    && rule.mode != VersionPriorityMode::Inherit
                {
                    return Some(rule);
                }
            }
        }
        self.library_rules.get(library_id)
    }

    pub fn is_empty(&self) -> bool {
        self.library_rules.values().all(|rule| !rule.is_active())
            && self
                .users
                .values()
                .all(|user| user.rules.values().all(|rule| !rule.is_active()))
    }
}

/// Orders the versions of every item for one viewer.
pub fn apply_version_priority(
    snapshot: &VersionPrioritySnapshot,
    user: Option<(&str, bool)>,
    items: &mut [CatalogItem],
) {
    if snapshot.is_empty() {
        return;
    }
    for item in items {
        if item.media_sources.len() < 2 {
            continue;
        }
        if let Some(rule) = snapshot.resolve(user, &item.library_id) {
            order_sources(&mut item.media_sources, rule);
        }
    }
}

/// Orders `sources` by `rule`. Parts of one multi-part version stay together and in part
/// order; only the first part of the winning version is marked as default.
pub fn order_sources(sources: &mut Vec<CatalogSource>, rule: &VersionPriorityRule) {
    if sources.len() < 2 || rule.mode == VersionPriorityMode::Inherit {
        return;
    }
    if rule.mode == VersionPriorityMode::Default {
        // Stored order (oldest first): the first version's first part is the default, even
        // when a library rule stored another default.
        let groups = crate::application::catalog::group_source_versions(std::mem::take(sources));
        sources.extend(groups.into_iter().flatten());
        for (index, source) in sources.iter_mut().enumerate() {
            source.is_default = index == 0;
        }
        return;
    }
    let mut groups = crate::application::catalog::group_source_versions(std::mem::take(sources));
    let ranks = groups
        .iter()
        .map(|group| SourceRank::of(&group[0], rule))
        .collect::<Vec<_>>();
    let mut order = (0..groups.len()).collect::<Vec<_>>();
    order.sort_by(|left, right| compare_ranks(&ranks[*left], &ranks[*right], rule));
    let mut taken = groups.drain(..).map(Some).collect::<Vec<_>>();
    for index in order {
        if let Some(group) = taken[index].take() {
            sources.extend(group);
        }
    }
    for (index, source) in sources.iter_mut().enumerate() {
        source.is_default = index == 0;
    }
}

#[derive(Debug, Default)]
struct SourceRank {
    keyword_group: usize,
    has_subtitles: bool,
    resolution: i64,
    hdr: i64,
    codec: i64,
    bitrate: i64,
    size: i64,
}

impl SourceRank {
    fn of(source: &CatalogSource, rule: &VersionPriorityRule) -> Self {
        let label_words = label_words(source);
        let custom = rule.custom.as_ref();
        let keyword_group = custom
            .and_then(|custom| {
                custom
                    .keyword_groups
                    .iter()
                    .position(|group| group.iter().any(|word| label_matches(&label_words, word)))
            })
            .unwrap_or(usize::MAX);
        let has_subtitles = source
            .streams
            .iter()
            .any(|stream| stream.stream_type.eq_ignore_ascii_case("subtitle"))
            || custom.is_some_and(|custom| {
                custom
                    .subtitle_keywords
                    .iter()
                    .any(|word| label_matches(&label_words, word))
            });
        let video = source
            .streams
            .iter()
            .find(|stream| stream.stream_type.eq_ignore_ascii_case("video"));
        let probed_resolution = video
            .map(|stream| {
                let height = detail_i64(&stream.details, "Height").unwrap_or(0);
                let width = detail_i64(&stream.details, "Width").unwrap_or(0);
                // Scope and portrait frames: judge by the larger of height and the height a
                // 16:9 frame of the same width would have.
                height.max(width * 9 / 16)
            })
            .unwrap_or(0);
        let resolution = if probed_resolution > 0 {
            probed_resolution
        } else {
            label_resolution(&label_words)
        };
        Self {
            keyword_group,
            has_subtitles,
            resolution,
            hdr: video.map_or_else(|| label_hdr(&label_words), stream_hdr_rank),
            codec: video
                .and_then(|stream| stream.codec.as_deref())
                .map_or(0, codec_rank),
            bitrate: source
                .bitrate
                .or_else(|| video.and_then(|stream| detail_i64(&stream.details, "BitRate")))
                .unwrap_or(0),
            size: source.size.unwrap_or(0),
        }
    }
}

fn compare_ranks(left: &SourceRank, right: &SourceRank, rule: &VersionPriorityRule) -> Ordering {
    let custom = rule.custom.as_ref();
    let mut ordering = Ordering::Equal;
    if rule.mode == VersionPriorityMode::Custom {
        ordering = left.keyword_group.cmp(&right.keyword_group);
        let subtitle = custom.map_or(SubtitlePreference::Ignore, |custom| custom.subtitle);
        ordering = ordering.then_with(|| match subtitle {
            SubtitlePreference::Prefer => right.has_subtitles.cmp(&left.has_subtitles),
            SubtitlePreference::Avoid => left.has_subtitles.cmp(&right.has_subtitles),
            SubtitlePreference::Ignore => Ordering::Equal,
        });
    }
    let tie_breakers = match custom {
        Some(custom)
            if rule.mode == VersionPriorityMode::Custom && !custom.tie_breakers.is_empty() =>
        {
            custom.tie_breakers.as_slice()
        }
        _ => &QUALITY_TIE_BREAKERS,
    };
    for tie_breaker in tie_breakers {
        ordering = ordering.then_with(|| match tie_breaker {
            TieBreaker::Resolution => right.resolution.cmp(&left.resolution),
            TieBreaker::Hdr => right.hdr.cmp(&left.hdr),
            TieBreaker::Codec => right.codec.cmp(&left.codec),
            TieBreaker::Bitrate => right.bitrate.cmp(&left.bitrate),
            TieBreaker::Size => right.size.cmp(&left.size),
        });
    }
    ordering
}

fn label_words(source: &CatalogSource) -> Vec<String> {
    let label = [
        source.edition_name.as_deref(),
        source.quality_label.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ");
    split_words(&label)
}

fn split_words(value: &str) -> Vec<String> {
    value
        .split(|character: char| {
            character.is_whitespace()
                || matches!(character, '-' | '_' | '.' | '[' | ']' | '(' | ')')
        })
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// A keyword matches when its words appear consecutively in the label.
fn label_matches(label_words: &[String], keyword: &str) -> bool {
    let keyword_words = split_words(keyword);
    !keyword_words.is_empty()
        && label_words
            .windows(keyword_words.len())
            .any(|window| window == keyword_words.as_slice())
}

fn label_resolution(words: &[String]) -> i64 {
    words
        .iter()
        .map(|word| match word.as_str() {
            "8k" | "4320p" => 4320,
            "4k" | "uhd" | "2160p" => 2160,
            "1440p" => 1440,
            "1080p" | "1080i" | "fhd" => 1080,
            "720p" => 720,
            "576p" => 576,
            "480p" => 480,
            _ => 0,
        })
        .max()
        .unwrap_or(0)
}

fn label_hdr(words: &[String]) -> i64 {
    words
        .iter()
        .map(|word| match word.as_str() {
            "dv" | "dovi" | "dolbyvision" => 3,
            "hdr10+" | "hdr10plus" => 2,
            "hdr" | "hdr10" | "hlg" => 1,
            _ => 0,
        })
        .max()
        .unwrap_or(0)
}

/// 3 Dolby Vision, 2 HDR10+, 1 other HDR, 0 SDR or unknown.
fn stream_hdr_rank(stream: &crate::application::catalog::CatalogStream) -> i64 {
    let text = |key: &str| {
        stream
            .details
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
            .and_then(|(_, value)| value.as_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default()
    };
    let range_type = text("VideoRangeType");
    let range = text("VideoRange");
    let transfer = text("ColorTransfer");
    let sub_type = text("ExtendedVideoSubType");
    if range_type.contains("dovi") || sub_type.contains("dovi") {
        3
    } else if range_type.contains("hdr10plus") || range_type.contains("hdr10+") {
        2
    } else if range.contains("hdr")
        || range_type.contains("hdr")
        || range_type.contains("hlg")
        || matches!(transfer.as_str(), "smpte2084" | "arib-std-b67")
    {
        1
    } else {
        0
    }
}

/// Newer codecs compress better at the same size, so they rank higher.
fn codec_rank(codec: &str) -> i64 {
    match codec.to_ascii_lowercase().as_str() {
        "av1" => 4,
        "hevc" | "h265" => 3,
        "vp9" => 2,
        "h264" | "avc" => 1,
        _ => 0,
    }
}

fn detail_i64(
    details: &std::collections::BTreeMap<String, serde_json::Value>,
    key: &str,
) -> Option<i64> {
    let value = details
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))?
        .1;
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::application::catalog::CatalogStream;

    fn source(id: &str, edition: Option<&str>, height: Option<i64>, size: i64) -> CatalogSource {
        CatalogSource {
            id: id.to_owned(),
            source_kind: "LOCAL_FILE".to_owned(),
            file_name: Some(format!("{id}.mkv")),
            container: Some("mkv".to_owned()),
            size: Some(size),
            external_url: None,
            edition_name: edition.map(str::to_owned),
            quality_label: None,
            bitrate: None,
            duration_ticks: None,
            is_default: id == "first",
            probe_status: "READY".to_owned(),
            streams: height
                .map(|height| {
                    vec![CatalogStream {
                        index: 0,
                        stream_type: "Video".to_owned(),
                        codec: Some("h264".to_owned()),
                        language: None,
                        title: None,
                        is_external: false,
                        is_default: true,
                        is_forced: false,
                        details: BTreeMap::from([("Height".to_owned(), json!(height))]),
                    }]
                })
                .unwrap_or_default(),
            chapters: Vec::new(),
        }
    }

    fn ids(sources: &[CatalogSource]) -> Vec<&str> {
        sources.iter().map(|source| source.id.as_str()).collect()
    }

    fn custom(groups: &[&[&str]]) -> VersionPriorityRule {
        VersionPriorityRule {
            mode: VersionPriorityMode::Custom,
            custom: Some(CustomVersionPriority {
                keyword_groups: groups
                    .iter()
                    .map(|group| group.iter().map(|word| (*word).to_owned()).collect())
                    .collect(),
                ..CustomVersionPriority::default()
            }),
        }
        .validated(false)
        .expect("valid rule")
    }

    #[test]
    fn default_mode_keeps_the_stored_order_and_defaults_to_the_oldest() {
        let mut sources = vec![
            source("first", Some("A"), Some(1080), 1),
            source("second", Some("B"), Some(2160), 2),
        ];
        sources[0].is_default = false;
        sources[1].is_default = true;
        order_sources(&mut sources, &VersionPriorityRule::default());
        assert_eq!(ids(&sources), ["first", "second"]);
        assert!(sources[0].is_default);
        assert!(!sources[1].is_default);
    }

    #[test]
    fn quality_mode_prefers_resolution_then_size() {
        let mut sources = vec![
            source("first", None, Some(1080), 9),
            source("uhd-small", None, Some(2160), 1),
            source("uhd-large", None, Some(2160), 5),
            source("label-4k", Some("4K"), None, 1),
        ];
        order_sources(
            &mut sources,
            &VersionPriorityRule {
                mode: VersionPriorityMode::Quality,
                custom: None,
            },
        );
        assert_eq!(
            ids(&sources),
            ["uhd-large", "uhd-small", "label-4k", "first"]
        );
        assert!(sources[0].is_default);
        assert_eq!(sources.iter().filter(|source| source.is_default).count(), 1);
    }

    #[test]
    fn custom_keywords_rank_first_and_resolution_breaks_ties() {
        let rule = custom(&[&["Directors Cut"], &["Extended"]]);
        let mut sources = vec![
            source("first", Some("Theatrical"), Some(2160), 1),
            source("extended", Some("Extended"), Some(1080), 1),
            source("dc-hd", Some("Directors Cut"), Some(1080), 1),
            source("dc-uhd", Some("Directors Cut 4K"), Some(2160), 1),
        ];
        order_sources(&mut sources, &rule);
        assert_eq!(ids(&sources), ["dc-uhd", "dc-hd", "extended", "first"]);
    }

    #[test]
    fn keywords_match_whole_words_case_insensitively() {
        let rule = custom(&[&["imax"]]);
        let mut sources = vec![
            source("first", Some("IMAXIMUM"), Some(1080), 1),
            source("imax", Some("IMAX Enhanced"), Some(1080), 1),
        ];
        order_sources(&mut sources, &rule);
        assert_eq!(ids(&sources), ["imax", "first"]);
    }

    #[test]
    fn subtitle_preference_uses_tracks_and_label_keywords() {
        let mut rule = custom(&[]);
        let custom_rule = rule.custom.as_mut().expect("custom");
        custom_rule.subtitle = SubtitlePreference::Prefer;
        custom_rule.subtitle_keywords = vec!["subbed".to_owned()];
        let mut sources = vec![
            source("first", None, Some(2160), 1),
            source("subbed", Some("Subbed"), Some(1080), 1),
        ];
        order_sources(&mut sources, &rule);
        assert_eq!(ids(&sources), ["subbed", "first"]);
        rule.custom.as_mut().expect("custom").subtitle = SubtitlePreference::Avoid;
        order_sources(&mut sources, &rule);
        assert_eq!(ids(&sources), ["first", "subbed"]);
    }

    #[test]
    fn parts_follow_their_version_and_only_the_first_part_is_default() {
        let rule = custom(&[&["B"]]);
        let mut sources = vec![
            source("first", Some("A"), Some(1080), 1),
            source("b-cd2", Some("B cd2"), Some(1080), 1),
            source("b-cd1", Some("B cd1"), Some(1080), 1),
        ];
        order_sources(&mut sources, &rule);
        assert_eq!(ids(&sources), ["b-cd1", "b-cd2", "first"]);
        let defaults = sources
            .iter()
            .filter(|source| source.is_default)
            .map(|source| source.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(defaults, ["b-cd1"]);
    }

    #[test]
    fn user_rules_override_library_rules_only_with_permission() {
        let library_rule = custom(&[&["A"]]);
        let user_rule = custom(&[&["B"]]);
        let mut snapshot = VersionPrioritySnapshot::default();
        snapshot
            .library_rules
            .insert("library".to_owned(), library_rule.clone());
        snapshot.users.insert(
            "user".to_owned(),
            UserVersionPriority {
                can_customize: true,
                rules: HashMap::from([(ALL_LIBRARIES_SCOPE.to_owned(), user_rule.clone())]),
            },
        );
        assert_eq!(
            snapshot.resolve(Some(("user", false)), "library"),
            Some(&user_rule)
        );
        assert_eq!(
            snapshot.resolve(Some(("other", false)), "library"),
            Some(&library_rule)
        );
        assert_eq!(snapshot.resolve(None, "library"), Some(&library_rule));

        snapshot.users.get_mut("user").expect("user").can_customize = false;
        assert_eq!(
            snapshot.resolve(Some(("user", false)), "library"),
            Some(&library_rule)
        );
        // Administrators may always customize.
        assert_eq!(
            snapshot.resolve(Some(("user", true)), "library"),
            Some(&user_rule)
        );
    }

    #[test]
    fn library_scoped_user_rules_win_and_inherit_falls_through() {
        let library_rule = custom(&[&["A"]]);
        let user_all = custom(&[&["B"]]);
        let mut snapshot = VersionPrioritySnapshot::default();
        snapshot
            .library_rules
            .insert("library".to_owned(), library_rule.clone());
        snapshot.users.insert(
            "user".to_owned(),
            UserVersionPriority {
                can_customize: true,
                rules: HashMap::from([
                    (ALL_LIBRARIES_SCOPE.to_owned(), user_all.clone()),
                    (
                        "library".to_owned(),
                        VersionPriorityRule {
                            mode: VersionPriorityMode::Inherit,
                            custom: None,
                        },
                    ),
                ]),
            },
        );
        // The library-scoped "inherit" falls through to the user's all-libraries rule.
        assert_eq!(
            snapshot.resolve(Some(("user", false)), "library"),
            Some(&user_all)
        );
        assert_eq!(
            snapshot.resolve(Some(("user", false)), "other"),
            Some(&user_all)
        );
    }

    #[test]
    fn validation_rejects_oversized_rules_and_inherit_for_libraries() {
        let rule = VersionPriorityRule {
            mode: VersionPriorityMode::Inherit,
            custom: None,
        };
        assert!(rule.clone().validated(false).is_err());
        assert!(rule.validated(true).is_ok());
        let too_many = VersionPriorityRule {
            mode: VersionPriorityMode::Custom,
            custom: Some(CustomVersionPriority {
                keyword_groups: vec![vec!["a".to_owned()]; MAX_KEYWORD_GROUPS + 1],
                ..CustomVersionPriority::default()
            }),
        };
        assert!(too_many.validated(false).is_err());
        let repeated = VersionPriorityRule {
            mode: VersionPriorityMode::Custom,
            custom: Some(CustomVersionPriority {
                tie_breakers: vec![TieBreaker::Size, TieBreaker::Size],
                ..CustomVersionPriority::default()
            }),
        };
        assert!(repeated.validated(false).is_err());
        let parsed: VersionPriorityRule = serde_json::from_value(json!({
            "mode": "custom",
            "custom": {
                "keywordGroups": [[" Extended ", ""], []],
                "subtitle": "ignore",
                "tieBreakers": ["resolution"]
            }
        }))
        .expect("rule json");
        let parsed = parsed.validated(false).expect("valid");
        assert_eq!(
            parsed.custom.expect("custom").keyword_groups,
            vec![vec!["Extended".to_owned()]]
        );
    }
}
