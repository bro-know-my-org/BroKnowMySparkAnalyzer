use super::{obj_at, path, str_at};
use crate::{Report, StackHotspot};
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Debug, Clone)]
struct SourceGroup {
    attributed: bool,
    source_id: String,
    name: String,
    version: Option<String>,
    samples: f64,
    max_percent: f64,
    frames: Vec<StackHotspot>,
}

fn label_parts(label: &str) -> (String, String) {
    let no_line = label
        .rsplit_once(':')
        .filter(|(_, line)| line.parse::<i64>().is_ok())
        .map_or(label, |(value, _)| value);
    match no_line.rsplit_once('.') {
        Some((class, method)) => (class.to_owned(), method.to_owned()),
        None => (no_line.to_owned(), no_line.to_owned()),
    }
}

fn resolve_source(report: &Report, hotspot: &StackHotspot) -> Option<(String, Value)> {
    let (fallback_class, fallback_method) = label_parts(&hotspot.label);
    let class_name = hotspot.class_name.as_deref().unwrap_or(&fallback_class);
    let method_name = hotspot.method_name.as_deref().unwrap_or(&fallback_method);
    crate::hot_paths::resolve_source_metadata(
        report,
        class_name,
        method_name,
        hotspot.method_desc.as_deref(),
        hotspot.line_number,
    )
}

pub(super) fn resolve_source_id(report: &Report, hotspot: &StackHotspot) -> Option<String> {
    resolve_source(report, hotspot)
        .map(|(id, _)| id)
        .filter(|id| !id.is_empty() && id != "unknown")
}

pub(super) fn has_source_maps(report: &Report) -> bool {
    ["classSources", "methodSources", "lineSources"]
        .into_iter()
        .any(|key| obj_at(&report.raw, key).is_some_and(|map| !map.is_empty()))
}

fn group_json(group: &SourceGroup) -> Value {
    json!({
        "sourceId": group.source_id,
        "name": group.name,
        "version": group.version,
        "samples": group.samples,
        "maxPercent": group.max_percent,
        "frames": group.frames,
    })
}

pub(super) fn mod_sources(report: &Report, limit: usize) -> Value {
    let metadata = obj_at(&report.raw, "metadata.sources");
    let mut by_source: HashMap<Option<String>, SourceGroup> = HashMap::new();
    let mut unresolved_hotspots = Vec::new();

    for hotspot in &report.summary.top_hotspots {
        let resolved = resolve_source(report, hotspot);
        let source_key = resolved
            .as_ref()
            .map(|(id, _)| id.clone())
            .filter(|id| !id.is_empty() && id != "unknown");
        if source_key.is_none() && unresolved_hotspots.len() < 12 {
            unresolved_hotspots.push(hotspot.clone());
        }
        let source_meta = resolved.as_ref().map(|(_, metadata)| metadata).or_else(|| {
            source_key
                .as_ref()
                .and_then(|id| metadata.and_then(|sources| sources.get(id)))
        });
        let entry = by_source
            .entry(source_key.clone())
            .or_insert_with(|| SourceGroup {
                attributed: source_key.is_some(),
                source_id: source_key
                    .clone()
                    .unwrap_or_else(|| "unattributed".to_owned()),
                name: source_meta
                    .and_then(|value| str_at(value, "name"))
                    .map(str::to_owned)
                    .or_else(|| source_key.clone())
                    .unwrap_or_else(|| "unattributed frames".to_owned()),
                version: source_meta
                    .and_then(|value| str_at(value, "version"))
                    .map(str::to_owned),
                samples: 0.0,
                max_percent: 0.0,
                frames: Vec::new(),
            });
        entry.samples += hotspot.samples;
        entry.max_percent = entry.max_percent.max(hotspot.percent);
        if entry.frames.len() < 8 {
            entry.frames.push(hotspot.clone());
        }
    }

    let mut all_sources: Vec<_> = by_source.into_values().collect();
    all_sources.sort_by(|left, right| {
        right
            .samples
            .total_cmp(&left.samples)
            .then_with(|| right.max_percent.total_cmp(&left.max_percent))
            .then_with(|| left.source_id.cmp(&right.source_id))
    });
    let unresolved = all_sources.iter().find(|source| !source.attributed);
    let resolved: Vec<_> = all_sources
        .iter()
        .filter(|source| source.attributed)
        .collect();
    let mut notable: Vec<_> = resolved
        .iter()
        .copied()
        .filter(|source| source.max_percent >= 3.0)
        .collect();
    notable.sort_by(|left, right| {
        right
            .max_percent
            .total_cmp(&left.max_percent)
            .then_with(|| right.samples.total_cmp(&left.samples))
            .then_with(|| left.source_id.cmp(&right.source_id))
    });

    let resolved_count = resolved.len();
    json!({
        "sourceMapAvailable": has_source_maps(report),
        "sourceCount": metadata.map_or(0, |sources| sources.len()),
        "resolvedSourceCount": resolved_count,
        "unresolvedHotspotCount": report.summary.top_hotspots.len().saturating_sub(
            report.summary.top_hotspots.iter().filter(|hotspot| resolve_source_id(report, hotspot).is_some()).count()
        ),
        "scope": "summary.topHotspots",
        "scopeNote": "Attribution is computed from the capped summary.topHotspots projection, not every raw stack frame.",
        "verdict": if resolved_count > 0 {
            format!("source map resolved {resolved_count} non-unknown source groups; do not report mod_sources as all unknown.")
        } else {
            "no non-unknown source groups were resolved from current hotspot frames.".to_owned()
        },
        "topSources": resolved.into_iter().take(limit).map(group_json).collect::<Vec<_>>(),
        "unresolvedFrameBucket": unresolved.map(|source| json!({
            "sourceId": source.source_id,
            "name": "unattributed frames",
            "version": source.version,
            "samples": source.samples,
            "maxPercent": source.max_percent,
            "frames": source.frames,
            "note": "These frames have no source mapping in the report and may belong to mods, application code, frameworks, or the runtime. They are not attributed to a specific source."
        })),
        "notableSources": notable.into_iter().take(limit).map(group_json).collect::<Vec<_>>(),
        "unresolvedHotspots": unresolved_hotspots,
        "mappingCounts": {
            "classes": obj_at(&report.raw, "classSources").map_or(0, |map| map.len()),
            "methods": obj_at(&report.raw, "methodSources").map_or(0, |map| map.len()),
            "lines": obj_at(&report.raw, "lineSources").map_or(0, |map| map.len()),
        },
        "sourceMetadataPresent": path(&report.raw, "metadata.sources").is_some(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ReportKind, ReportSummary};

    #[test]
    fn preserves_resolved_source_contract() {
        let hotspot = StackHotspot {
            label: "dev.example.Tick.work:42".into(),
            samples: 12.0,
            percent: 6.0,
            thread: "Server thread".into(),
            source: None,
            class_name: Some("dev.example.Tick".into()),
            method_name: Some("work".into()),
            method_desc: Some("()V".into()),
            line_number: Some(42),
        };
        let report = Report {
            kind: ReportKind::Sampler,
            source: "fixture".into(),
            raw: json!({
                "lineSources": {"dev.example.Tick;42": "example"},
                "metadata": {"sources": {"example": {"name": "Example Mod", "version": "1.0"}}}
            }),
            summary: ReportSummary {
                title: "fixture".into(),
                top_hotspots: vec![hotspot],
                ..Default::default()
            },
        };
        let value = mod_sources(&report, 24);
        assert_eq!(value["resolvedSourceCount"], 1);
        assert_eq!(value["topSources"][0]["name"], "Example Mod");
        assert_eq!(value["notableSources"][0]["sourceId"], "example");
        assert!(value["verdict"].as_str().unwrap().contains("resolved 1"));
    }
}
