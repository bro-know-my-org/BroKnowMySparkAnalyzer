use crate::analysis::summarize;
use crate::proto::{HealthData, HeapData, SamplerData};
use crate::{Report, ReportKind, SparkError};
use prost::Message;
use serde::Serialize;
use serde_json::{json, Value};

const MAX_REPORT_BYTES: usize = 256 * 1024 * 1024;
const MAX_TEXT_REPORT_BYTES: usize = 16 * 1024 * 1024;

fn hinted_kind(hint: &str) -> Option<ReportKind> {
    hint.split_whitespace().find_map(|part| {
        let token = part
            .split_once(';')
            .map_or(part, |(value, _)| value)
            .trim()
            .to_ascii_lowercase();
        let path = token
            .split_once('?')
            .map_or(token.as_str(), |(value, _)| value);
        let extension = path.rsplit_once('.').map(|(_, value)| value);
        match token.as_str() {
            "profile"
            | "sparkprofile"
            | "application/x-spark-profile"
            | "application/x-spark-sampler" => Some(ReportKind::Sampler),
            "health" | "sparkhealth" | "application/x-spark-health" => Some(ReportKind::Health),
            "heap" | "sparkheap" | "application/x-spark-heap" => Some(ReportKind::Heap),
            _ => match extension {
                Some("sparkprofile" | "profile") => Some(ReportKind::Sampler),
                Some("sparkhealth" | "health") => Some(ReportKind::Health),
                Some("sparkheap" | "heap") => Some(ReportKind::Heap),
                _ => None,
            },
        }
    })
}

fn to_value<T: Serialize>(value: &T) -> Result<Value, SparkError> {
    Ok(serde_json::to_value(value)?)
}

fn sampler_score(v: &Value) -> usize {
    v.get("threads")
        .and_then(Value::as_array)
        .map_or(0, |a| a.len() * 100)
        + v.pointer("/metadata/startTime")
            .and_then(Value::as_i64)
            .is_some_and(|v| v > 0) as usize
            * 20
        + v.pointer("/metadata/endTime")
            .and_then(Value::as_i64)
            .is_some_and(|v| v > 0) as usize
            * 20
        + v.get("timeWindowStatistics")
            .and_then(Value::as_object)
            .map_or(0, |m| m.len() * 40)
        + v.get("classSources")
            .and_then(Value::as_object)
            .map_or(0, |m| m.len() * 5)
        + v.get("methodSources")
            .and_then(Value::as_object)
            .map_or(0, |m| m.len() * 5)
        + v.get("lineSources")
            .and_then(Value::as_object)
            .map_or(0, |m| m.len() * 5)
        + v.get("timeWindows")
            .and_then(Value::as_array)
            .map_or(0, |items| items.len() * 20)
        + v.get("channelInfo").is_some_and(|value| !value.is_null()) as usize * 10
        + platform_score(v)
}
fn health_score(v: &Value) -> usize {
    v.get("timeWindowStatistics")
        .and_then(Value::as_object)
        .map_or(0, |m| m.len() * 100)
        + v.pointer("/metadata/generatedTime")
            .and_then(Value::as_i64)
            .is_some_and(|v| v > 0) as usize
            * 20
        + platform_score(v)
}
fn heap_score(v: &Value) -> usize {
    v.get("entries")
        .and_then(Value::as_array)
        .map_or(0, |a| a.len() * 100)
        + v.pointer("/metadata/generatedTime")
            .and_then(Value::as_i64)
            .is_some_and(|v| v > 0) as usize
            * 20
        + platform_score(v)
}
fn platform_score(v: &Value) -> usize {
    ["name", "version", "minecraftVersion"]
        .into_iter()
        .filter(|field| {
            v.pointer(&format!("/metadata/platformMetadata/{field}"))
                .and_then(Value::as_str)
                .is_some_and(|s| !s.is_empty())
        })
        .count()
        * 5
}

pub fn parse_report_bytes(
    bytes: &[u8],
    source: impl Into<String>,
    hint: impl AsRef<str>,
) -> Result<Report, SparkError> {
    if bytes.len() > MAX_REPORT_BYTES {
        return Err(SparkError::Decode(format!(
            "report exceeds the {} MiB decode limit",
            MAX_REPORT_BYTES / 1024 / 1024
        )));
    }
    let source = source.into();
    let hint = hinted_kind(hint.as_ref());
    let mut candidates: Vec<(ReportKind, Value, usize)> = vec![];
    let mut errors = vec![];
    if hint.is_none_or(|kind| kind == ReportKind::Sampler) {
        match SamplerData::decode(bytes) {
            Ok(v) => match to_value(&v) {
                Ok(raw) => {
                    let score = sampler_score(&raw);
                    candidates.push((ReportKind::Sampler, raw, score))
                }
                Err(e) => errors.push(e.to_string()),
            },
            Err(e) => errors.push(format!("SamplerData: {e}")),
        }
    }
    if hint.is_none_or(|kind| kind == ReportKind::Health) {
        match HealthData::decode(bytes) {
            Ok(v) => match to_value(&v) {
                Ok(raw) => {
                    let score = health_score(&raw);
                    candidates.push((ReportKind::Health, raw, score))
                }
                Err(e) => errors.push(e.to_string()),
            },
            Err(e) => errors.push(format!("HealthData: {e}")),
        }
    }
    if hint.is_none_or(|kind| kind == ReportKind::Heap) {
        match HeapData::decode(bytes) {
            Ok(v) => match to_value(&v) {
                Ok(raw) => {
                    let score = heap_score(&raw);
                    candidates.push((ReportKind::Heap, raw, score))
                }
                Err(e) => errors.push(e.to_string()),
            },
            Err(e) => errors.push(format!("HeapData: {e}")),
        }
    }
    candidates.retain(|(_, _, score)| *score > 0);
    candidates.sort_by_key(|(_, _, score)| std::cmp::Reverse(*score));
    if candidates.len() > 1 && candidates[0].2 == candidates[1].2 {
        return Err(SparkError::Decode(format!(
            "ambiguous protobuf report: {:?} and {:?} have equal evidence scores",
            candidates[0].0, candidates[1].0
        )));
    }
    let Some((kind, raw, score)) = candidates.into_iter().next() else {
        let detail = if errors.is_empty() {
            "protobuf decoded but contained no report-specific evidence".to_owned()
        } else {
            errors.join(" | ")
        };
        return Err(SparkError::Decode(detail));
    };
    debug_assert!(score > 0);
    let summary = summarize(kind, &raw, &source);
    Ok(Report {
        kind,
        source,
        raw,
        summary,
    })
}

pub fn parse_text_report(
    text: impl Into<String>,
    source: impl Into<String>,
) -> Result<Report, SparkError> {
    let text = text.into();
    if text.len() > MAX_TEXT_REPORT_BYTES {
        return Err(SparkError::Decode(
            "text report exceeds the 16 MiB limit".into(),
        ));
    }
    let source = source.into();
    let raw = json!({"text":text});
    let summary = summarize(ReportKind::Text, &raw, &source);
    Ok(Report {
        kind: ReportKind::Text,
        source,
        raw,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_sampler_with_report_evidence() {
        let data = SamplerData {
            metadata: Some(crate::proto::SamplerMetadata {
                start_time: 123,
                ..Default::default()
            }),
            threads: vec![],
            class_sources: Default::default(),
            method_sources: Default::default(),
            line_sources: Default::default(),
            time_windows: vec![],
            time_window_statistics: Default::default(),
            channel_info: None,
        };
        let bytes = data.encode_to_vec();
        let r = parse_report_bytes(&bytes, "fixture", "profile").unwrap();
        assert_eq!(r.kind, ReportKind::Sampler);
    }
    #[test]
    fn rejects_empty_wire_message() {
        assert!(parse_report_bytes(&[], "empty", "").is_err());
    }
    #[test]
    fn hint_cannot_turn_empty_protobuf_into_a_report() {
        assert!(parse_report_bytes(&[], "empty.sparkprofile", "profile").is_err());
        assert!(parse_report_bytes(&[], "empty.sparkheap", "heap").is_err());
        assert!(parse_report_bytes(&[], "empty.health", "health").is_err());
    }
    #[test]
    fn hint_requires_an_exact_extension_or_content_type() {
        let data = SamplerData {
            metadata: Some(crate::proto::SamplerMetadata {
                start_time: 123,
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            parse_report_bytes(
                &data.encode_to_vec(),
                "fixture",
                "https://example.invalid/not-a-profile-name"
            )
            .unwrap()
            .kind,
            ReportKind::Sampler
        );
    }
    #[test]
    fn recognizes_health_and_heap_from_intrinsic_fields() {
        let health = HealthData {
            metadata: Some(crate::proto::HealthMetadata {
                generated_time: 123,
                ..Default::default()
            }),
            time_window_statistics: Default::default(),
        };
        assert_eq!(
            parse_report_bytes(&health.encode_to_vec(), "health", "health")
                .unwrap()
                .kind,
            ReportKind::Health
        );

        let heap = HeapData {
            metadata: None,
            entries: vec![crate::proto::HeapEntry {
                order: 1,
                instances: 2,
                size: 3,
                r#type: "example.Type".into(),
            }],
        };
        assert_eq!(
            parse_report_bytes(&heap.encode_to_vec(), "heap", "")
                .unwrap()
                .kind,
            ReportKind::Heap
        );
    }
    #[test]
    fn text_report_is_structured() {
        let r = parse_text_report("can't keep up", "log").unwrap();
        assert_eq!(r.kind, ReportKind::Text);
        assert_eq!(r.summary.findings.len(), 2);
    }
}
