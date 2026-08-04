use super::{arr_at, i64_at, path, str_at};
use crate::Report;
use serde_json::{json, Value};

fn number(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        .or_else(|| value.as_str()?.parse::<i64>().ok())
        .filter(|value| *value >= 0)
}

fn resource_location(value: &str) -> Option<(&str, &str)> {
    let (namespace, entity) = value.split_once(':')?;
    if namespace.is_empty()
        || entity.is_empty()
        || entity.contains(':')
        || !namespace.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || "_.-".contains(character)
        })
        || !entity.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || "_./-".contains(character)
        })
    {
        return None;
    }
    Some((namespace, entity))
}

fn sorted_entities(value: Option<&Value>, limit: usize) -> Vec<Value> {
    let mut entities: Vec<_> = value
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(name, value)| {
            number(value).map(|value| json!({"name": name, "value": value}))
        })
        .collect();
    entities.sort_by_key(|item| std::cmp::Reverse(item["value"].as_i64().unwrap_or_default()));
    entities.truncate(limit);
    entities
}

fn risk_signals(counts: Option<&Value>) -> Vec<String> {
    let mut item = Vec::new();
    let mut mods = Vec::new();
    let mut many = Vec::new();
    for (name, value) in counts.and_then(Value::as_object).into_iter().flatten() {
        let Some(count) = number(value) else {
            continue;
        };
        if count <= 0 {
            continue;
        }
        if count >= 50 && many.len() < 24 {
            many.push(format!("many:{name}={count}"));
        }
        let Some((namespace, entity_id)) = resource_location(name) else {
            continue;
        };
        if entity_id.eq_ignore_ascii_case("item") && item.len() < 20 {
            item.push(format!("item_entity:{name}={count}"));
        }
        if namespace != "minecraft" && mods.len() < 20 {
            mods.push(format!("mod_entity:{name}={count}"));
        }
    }
    item.into_iter().chain(mods).chain(many).take(64).collect()
}

pub(super) fn entity_chunks(report: &Report, limit: usize) -> Value {
    let world = path(&report.raw, "metadata.platformStatistics.world");
    let mut chunks = Vec::new();
    for world_entry in world
        .and_then(|value| arr_at(value, "worlds"))
        .into_iter()
        .flatten()
    {
        let world_name = str_at(world_entry, "name").unwrap_or("unknown");
        for region in arr_at(world_entry, "regions").into_iter().flatten() {
            for chunk in arr_at(region, "chunks").into_iter().flatten() {
                let counts = path(chunk, "entityCounts");
                chunks.push(json!({
                    "world": world_name,
                    "x": i64_at(chunk, "x"),
                    "z": i64_at(chunk, "z"),
                    "totalEntities": i64_at(chunk, "totalEntities"),
                    "topEntities": sorted_entities(counts, 12),
                    "riskSignals": risk_signals(counts),
                }));
                if chunks.len() >= 4_096 {
                    chunks.sort_by_key(|chunk| {
                        std::cmp::Reverse(i64_at(chunk, "totalEntities").unwrap_or_default())
                    });
                    chunks.truncate(2_048.max(limit));
                }
            }
        }
    }
    let has_chunks = !chunks.is_empty();
    chunks
        .sort_by_key(|chunk| std::cmp::Reverse(i64_at(chunk, "totalEntities").unwrap_or_default()));
    chunks.truncate(limit);

    json!({
        "totalEntities": world.and_then(|value| i64_at(value, "totalEntities")),
        "topEntityTypes": sorted_entities(world.and_then(|value| path(value, "entityCounts")), 20),
        "topChunks": chunks,
        "note": if has_chunks {
            "High entity chunks identify where to inspect in-game; they do not prove CPU cost without matching hotspot frames."
        } else {
            "Report does not include per-chunk entity data."
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ReportKind, ReportSummary};

    #[test]
    fn restores_entity_chunk_contract() {
        let report = Report {
            kind: ReportKind::Sampler,
            source: "fixture".into(),
            raw: json!({"metadata":{"platformStatistics":{"world":{
                "totalEntities": 90,
                "entityCounts": {"minecraft:zombie": 10, "example:worker": 80},
                "worlds":[{"name":"world","regions":[{"chunks":[{"x":1,"z":2,"totalEntities":80,"entityCounts":{"example:worker":80}}]}]}]
            }}}}),
            summary: ReportSummary {
                title: "fixture".into(),
                ..Default::default()
            },
        };
        let value = entity_chunks(&report, 24);
        assert_eq!(value["totalEntities"], 90);
        assert_eq!(value["topEntityTypes"][0]["name"], "example:worker");
        assert_eq!(value["topChunks"][0]["topEntities"][0]["value"], 80.0);
        assert!(
            value["topChunks"][0]["riskSignals"]
                .as_array()
                .unwrap()
                .len()
                >= 2
        );
    }
}
