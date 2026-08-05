use crate::analysis::summarize;
use crate::proto::{HealthData, HeapData, SamplerData};
use crate::{Report, ReportKind, SparkError};
use prost::Message;
use serde::Serialize;
use serde_json::{json, Value};

const MAX_REPORT_BYTES: usize = 64 * 1024 * 1024;
const MAX_TEXT_REPORT_BYTES: usize = 16 * 1024 * 1024;
const MAX_PROTOBUF_FIELDS: usize = 250_000;

#[derive(Clone, Copy)]
enum WireSchema {
    SamplerRoot,
    SamplerMetadata,
    ThreadNode,
    StackTraceNode,
    HealthRoot,
    HealthMetadata,
    HeapRoot,
    HeapMetadata,
    PlatformMetadata,
    SystemStatistics,
    Cpu,
    SystemMemory,
    PlatformStatistics,
    PlatformMemory,
    PlatformMemoryPool,
    Mspt,
    Ping,
    WorldStatistics,
    World,
    Region,
    Chunk,
    GameRule,
    NetInterface,
    ScalarMap,
    GcMap,
    PluginMap,
    WindowMap,
    NetMap,
    Leaf,
}

impl WireSchema {
    fn nested(self, field: u64) -> Option<Self> {
        match self {
            Self::SamplerRoot => match field {
                1 => Some(Self::SamplerMetadata),
                2 => Some(Self::ThreadNode),
                3..=5 => Some(Self::ScalarMap),
                7 => Some(Self::WindowMap),
                8 => Some(Self::Leaf),
                _ => None,
            },
            Self::SamplerMetadata => match field {
                1 | 4 | 5 => Some(Self::Leaf),
                7 => Some(Self::PlatformMetadata),
                8 => Some(Self::PlatformStatistics),
                9 => Some(Self::SystemStatistics),
                10 | 14 => Some(Self::ScalarMap),
                13 => Some(Self::PluginMap),
                _ => None,
            },
            Self::ThreadNode => (field == 3).then_some(Self::StackTraceNode),
            Self::HealthRoot => match field {
                1 => Some(Self::HealthMetadata),
                2 => Some(Self::WindowMap),
                _ => None,
            },
            Self::HealthMetadata | Self::HeapMetadata => match field {
                1 => Some(Self::Leaf),
                2 => Some(Self::PlatformMetadata),
                3 => Some(Self::PlatformStatistics),
                4 => Some(Self::SystemStatistics),
                6 | 8 => Some(Self::ScalarMap),
                7 => Some(Self::PluginMap),
                _ => None,
            },
            Self::HeapRoot => match field {
                1 => Some(Self::HeapMetadata),
                2 => Some(Self::Leaf),
                _ => None,
            },
            Self::SystemStatistics => match field {
                1 => Some(Self::Cpu),
                2 => Some(Self::SystemMemory),
                3 => Some(Self::GcMap),
                4 | 5 | 6 | 9 => Some(Self::Leaf),
                8 => Some(Self::NetMap),
                _ => None,
            },
            Self::Cpu => matches!(field, 2 | 3).then_some(Self::Leaf),
            Self::SystemMemory => matches!(field, 1 | 2).then_some(Self::Leaf),
            Self::PlatformStatistics => match field {
                1 => Some(Self::PlatformMemory),
                2 => Some(Self::GcMap),
                4 => Some(Self::Leaf),
                5 => Some(Self::Mspt),
                6 => Some(Self::Ping),
                8 => Some(Self::WorldStatistics),
                _ => None,
            },
            Self::PlatformMemory => match field {
                1 | 2 => Some(Self::Leaf),
                3 => Some(Self::PlatformMemoryPool),
                _ => None,
            },
            Self::PlatformMemoryPool => matches!(field, 2 | 3).then_some(Self::Leaf),
            Self::Mspt => matches!(field, 1 | 2).then_some(Self::Leaf),
            Self::Ping => (field == 1).then_some(Self::Leaf),
            Self::WorldStatistics => match field {
                2 => Some(Self::ScalarMap),
                3 => Some(Self::World),
                4 => Some(Self::GameRule),
                5 => Some(Self::Leaf),
                _ => None,
            },
            Self::World => (field == 3).then_some(Self::Region),
            Self::Region => (field == 2).then_some(Self::Chunk),
            Self::Chunk => (field == 4).then_some(Self::ScalarMap),
            Self::GameRule => (field == 3).then_some(Self::ScalarMap),
            Self::NetInterface => matches!(field, 1..=4).then_some(Self::Leaf),
            Self::PluginMap => (field == 2).then_some(Self::Leaf),
            Self::GcMap => (field == 2).then_some(Self::Leaf),
            Self::WindowMap => (field == 2).then_some(Self::Leaf),
            Self::NetMap => (field == 2).then_some(Self::NetInterface),
            Self::StackTraceNode | Self::PlatformMetadata | Self::ScalarMap | Self::Leaf => None,
        }
    }
}

fn read_varint(bytes: &[u8], cursor: &mut usize) -> Result<u64, SparkError> {
    let mut value = 0u64;
    for shift in (0..70).step_by(7) {
        let byte = *bytes
            .get(*cursor)
            .ok_or_else(|| SparkError::Decode("truncated protobuf varint".into()))?;
        *cursor += 1;
        if shift == 63 && byte > 1 {
            return Err(SparkError::Decode("protobuf varint overflow".into()));
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(SparkError::Decode("protobuf varint overflow".into()))
}

fn scan_protobuf_message(
    bytes: &[u8],
    schema: WireSchema,
    fields: &mut usize,
) -> Result<bool, SparkError> {
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        *fields += 1;
        if *fields > MAX_PROTOBUF_FIELDS {
            return Err(SparkError::Decode(format!(
                "protobuf report exceeds the {MAX_PROTOBUF_FIELDS} field limit"
            )));
        }
        let Ok(tag) = read_varint(bytes, &mut cursor) else {
            return Ok(false);
        };
        let field = tag >> 3;
        if field == 0 {
            return Ok(false);
        }
        match tag & 0x07 {
            0 => {
                if read_varint(bytes, &mut cursor).is_err() {
                    return Ok(false);
                }
            }
            1 | 5 => {
                let payload_len = if tag & 0x07 == 1 { 8 } else { 4 };
                let Some(end) = cursor
                    .checked_add(payload_len)
                    .filter(|end| *end <= bytes.len())
                else {
                    return Ok(false);
                };
                cursor = end;
            }
            2 => {
                let Ok(payload_len) = read_varint(bytes, &mut cursor).and_then(|length| {
                    usize::try_from(length)
                        .map_err(|_| SparkError::Decode("protobuf field length overflow".into()))
                }) else {
                    return Ok(false);
                };
                let Some(end) = cursor
                    .checked_add(payload_len)
                    .filter(|end| *end <= bytes.len())
                else {
                    return Ok(false);
                };
                if let Some(nested_schema) = schema.nested(field) {
                    if !scan_protobuf_message(&bytes[cursor..end], nested_schema, fields)? {
                        return Ok(false);
                    }
                }
                cursor = end;
            }
            _ => return Ok(false),
        }
    }
    Ok(true)
}

fn validate_protobuf_wire_budget(bytes: &[u8], schema: WireSchema) -> Result<(), SparkError> {
    let mut fields = 0usize;
    if scan_protobuf_message(bytes, schema, &mut fields)? {
        Ok(())
    } else {
        Err(SparkError::Decode("invalid protobuf wire format".into()))
    }
}

fn decode_sampler(bytes: &[u8]) -> Result<SamplerData, String> {
    validate_protobuf_wire_budget(bytes, WireSchema::SamplerRoot)
        .map_err(|error| error.to_string())?;
    SamplerData::decode(bytes).map_err(|error| format!("SamplerData: {error}"))
}

fn decode_health(bytes: &[u8]) -> Result<HealthData, String> {
    validate_protobuf_wire_budget(bytes, WireSchema::HealthRoot)
        .map_err(|error| error.to_string())?;
    HealthData::decode(bytes).map_err(|error| format!("HealthData: {error}"))
}

fn decode_heap(bytes: &[u8]) -> Result<HeapData, String> {
    validate_protobuf_wire_budget(bytes, WireSchema::HeapRoot)
        .map_err(|error| error.to_string())?;
    HeapData::decode(bytes).map_err(|error| format!("HeapData: {error}"))
}

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
    if let Some(kind) = hint {
        let hinted = match kind {
            ReportKind::Sampler => decode_sampler(bytes).and_then(|value| {
                to_value(&value)
                    .map(|raw| {
                        let score = sampler_score(&raw);
                        (raw, score)
                    })
                    .map_err(|error| error.to_string())
            }),
            ReportKind::Health => decode_health(bytes).and_then(|value| {
                to_value(&value)
                    .map(|raw| {
                        let score = health_score(&raw);
                        (raw, score)
                    })
                    .map_err(|error| error.to_string())
            }),
            ReportKind::Heap => decode_heap(bytes).and_then(|value| {
                to_value(&value)
                    .map(|raw| {
                        let score = heap_score(&raw);
                        (raw, score)
                    })
                    .map_err(|error| error.to_string())
            }),
            ReportKind::Text => unreachable!(),
        };
        if let Ok((raw, score)) = hinted {
            if score > 0 {
                let summary = summarize(kind, &raw, &source);
                return Ok(Report {
                    kind,
                    source,
                    raw,
                    summary,
                });
            }
        }
    }
    let mut best: Option<(ReportKind, Value, usize)> = None;
    let mut tied_kind = None;
    let mut errors = vec![];
    let mut consider = |kind: ReportKind, raw: Value, score: usize| {
        if score == 0 {
            return;
        }
        let candidate_preferred = hint == Some(kind);
        match best.as_ref() {
            None => best = Some((kind, raw, score)),
            Some((best_kind, _, best_score)) => {
                let best_preferred = hint == Some(*best_kind);
                if (score, candidate_preferred) > (*best_score, best_preferred) {
                    best = Some((kind, raw, score));
                    tied_kind = None;
                } else if score == *best_score && candidate_preferred == best_preferred {
                    tied_kind = Some(kind);
                }
            }
        }
    };
    {
        match decode_sampler(bytes) {
            Ok(v) => match to_value(&v) {
                Ok(raw) => {
                    let score = sampler_score(&raw);
                    consider(ReportKind::Sampler, raw, score)
                }
                Err(e) => errors.push(e.to_string()),
            },
            Err(e) => errors.push(e),
        }
    }
    {
        match decode_health(bytes) {
            Ok(v) => match to_value(&v) {
                Ok(raw) => {
                    let score = health_score(&raw);
                    consider(ReportKind::Health, raw, score)
                }
                Err(e) => errors.push(e.to_string()),
            },
            Err(e) => errors.push(e),
        }
    }
    {
        match decode_heap(bytes) {
            Ok(v) => match to_value(&v) {
                Ok(raw) => {
                    let score = heap_score(&raw);
                    consider(ReportKind::Heap, raw, score)
                }
                Err(e) => errors.push(e.to_string()),
            },
            Err(e) => errors.push(e),
        }
    }
    if let (Some((best_kind, _, _)), Some(other_kind)) = (best.as_ref(), tied_kind) {
        return Err(SparkError::Decode(format!(
            "ambiguous protobuf report: {:?} and {:?} have equal evidence scores",
            best_kind, other_kind
        )));
    }
    let Some((kind, raw, score)) = best else {
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
    fn rejects_excessive_top_level_field_count_before_decoding() {
        let bytes = [0x08, 0x00].repeat(MAX_PROTOBUF_FIELDS + 1);
        let error = parse_report_bytes(&bytes, "hostile", "").unwrap_err();
        assert!(error.to_string().contains("field limit"));
    }
    #[test]
    fn rejects_excessive_nested_field_count_before_decoding() {
        let nested = [0x1a, 0x00].repeat(MAX_PROTOBUF_FIELDS + 1);
        let mut bytes = vec![0x12];
        prost::encoding::encode_varint(nested.len() as u64, &mut bytes);
        bytes.extend(nested);
        let error = parse_report_bytes(&bytes, "hostile", "").unwrap_err();
        assert!(error.to_string().contains("field limit"));
    }
    #[test]
    fn does_not_treat_opaque_string_bytes_as_nested_messages() {
        let data = SamplerData {
            metadata: Some(crate::proto::SamplerMetadata {
                start_time: 123,
                comment: String::from_utf8([0x08, 0x00].repeat(MAX_PROTOBUF_FIELDS + 1)).unwrap(),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            parse_report_bytes(&data.encode_to_vec(), "comment", "profile")
                .unwrap()
                .kind,
            ReportKind::Sampler
        );
    }
    #[test]
    fn does_not_scan_strings_inside_common_metadata_messages() {
        let data = SamplerData {
            metadata: Some(crate::proto::SamplerMetadata {
                start_time: 123,
                platform_metadata: Some(crate::proto::PlatformMetadata {
                    name: String::from_utf8([0x08, 0x00].repeat(MAX_PROTOBUF_FIELDS + 1)).unwrap(),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            parse_report_bytes(&data.encode_to_vec(), "platform", "profile")
                .unwrap()
                .kind,
            ReportKind::Sampler
        );
    }
    #[test]
    fn rejects_excessive_fields_in_deep_common_metadata_messages() {
        fn length_field(tag: u8, payload: Vec<u8>) -> Vec<u8> {
            let mut encoded = vec![tag];
            prost::encoding::encode_varint(payload.len() as u64, &mut encoded);
            encoded.extend(payload);
            encoded
        }
        let cpu = [0x08, 0x00].repeat(MAX_PROTOBUF_FIELDS + 1);
        let system_statistics = length_field(0x0a, cpu);
        let mut metadata = vec![0x10, 0x01];
        metadata.extend(length_field(0x4a, system_statistics));
        let error =
            parse_report_bytes(&length_field(0x0a, metadata), "hostile", "profile").unwrap_err();
        assert!(error.to_string().contains("field limit"));
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
