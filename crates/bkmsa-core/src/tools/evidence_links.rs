use super::source_attribution::mod_sources;
use super::{entity_chunks, f64_at, hotspot_groups};
use crate::analysis::{classify_hotspot, is_generic_frame};
use crate::Report;
use serde_json::{json, Map, Value};
use std::collections::{BTreeSet, HashMap};

fn confidence(checks: impl IntoIterator<Item = bool>) -> &'static str {
    match checks.into_iter().filter(|value| *value).count() {
        3.. => "high",
        2 => "medium",
        _ => "low",
    }
}

pub(super) fn confidence_rank(value: &Value) -> i32 {
    match value.as_str() {
        Some("high") => 3,
        Some("medium") => 2,
        Some("low") => 1,
        _ => 0,
    }
}

fn normalized(value: &str) -> String {
    value
        .chars()
        .filter(|char| char.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn wrapper_source(source_id: &str, source_name: &str) -> bool {
    let value = format!("{source_id} {source_name}").to_ascii_lowercase();
    value
        .split(|char: char| !char.is_ascii_alphanumeric())
        .any(|part| {
            [
                "neruina",
                "observable",
                "mixin",
                "wrapper",
                "bridge",
                "hook",
            ]
            .contains(&part)
        })
}

fn evidence_sources(names: impl IntoIterator<Item = &'static str>) -> Vec<&'static str> {
    names
        .into_iter()
        .filter(|name| !name.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn matching_chunks<'a>(chunks: &'a Value, entity_id: &str) -> Vec<&'a Value> {
    let token = normalized(entity_id.rsplit(':').next().unwrap_or(entity_id));
    let qualified = entity_id.contains(':');
    chunks["topChunks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|chunk| {
            chunk["topEntities"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|entity| {
                    entity["name"].as_str().is_some_and(|name| {
                        name == entity_id
                            || (!qualified
                                && normalized(name.rsplit(':').next().unwrap_or(name)) == token)
                    })
                })
        })
        .collect()
}

fn recurring_family(frame: &Value, label: &str) -> Option<(String, String)> {
    let source_id = frame
        .get("sourceId")
        .or_else(|| frame.get("terminalSourceId"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let source_name = frame
        .get("sourceName")
        .or_else(|| frame.get("terminalSourceName"))
        .and_then(Value::as_str)
        .unwrap_or(source_id);
    if source_id != "unknown" && !wrapper_source(source_id, source_name) {
        return Some((format!("source:{source_id}"), source_name.to_owned()));
    }

    let class_name = frame
        .get("className")
        .and_then(Value::as_str)
        .or_else(|| label.rsplit_once('.').map(|(class, _)| class))?;
    let parts = class_name
        .split('.')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let root = parts.first()?.to_ascii_lowercase();
    if ["java", "javax", "jdk", "sun", "net", "nms", "nm"].contains(&root.as_str()) {
        return None;
    }
    let key = if parts.len() <= 2 {
        parts.join(".")
    } else if ["com", "org", "io", "me"].contains(&root.as_str()) {
        parts.iter().take(3).copied().collect::<Vec<_>>().join(".")
    } else {
        parts[0].to_owned()
    };
    (key.len() >= 4).then(|| (format!("package:{key}"), key))
}

fn collect_recurring_families(paths: &Value, report: &Report, limit: usize) -> Vec<Value> {
    let mut families: HashMap<String, Value> = HashMap::new();
    let mut seen_occurrences: HashMap<String, BTreeSet<String>> = HashMap::new();
    {
        let mut add = |frame: &Value, context: String, fallback_category: &str| {
            let label = frame
                .get("label")
                .or_else(|| frame.get("terminalLabel"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if label.is_empty()
                || is_generic_frame(label)
                || label.contains("MinecraftServer.runServer")
                || label.contains("MinecraftServer.tickServer")
            {
                return;
            }
            let Some((id, name)) = recurring_family(frame, label) else {
                return;
            };
            let category = frame
                .get("category")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .unwrap_or(fallback_category);
            let percent = frame
                .get("percent")
                .or_else(|| frame.get("maxPercent"))
                .or_else(|| frame.get("terminalPercent"))
                .and_then(Value::as_f64)
                .unwrap_or_default();
            let occurrence = format!("{category}\u{1f}{label}\u{1f}{percent:.6}");
            if !seen_occurrences
                .entry(id.clone())
                .or_default()
                .insert(occurrence)
            {
                return;
            }
            let entry = families.entry(id.clone()).or_insert_with(|| {
            json!({"id":id,"name":name,"occurrences":0,"maxPercent":0.0,"categories":[],"contexts":[],"examples":[],"fromHotspots":false})
        });
            entry["occurrences"] = json!(entry["occurrences"].as_u64().unwrap_or_default() + 1);
            entry["maxPercent"] = json!(entry["maxPercent"]
                .as_f64()
                .unwrap_or_default()
                .max(percent));
            for (field, value) in [("categories", category), ("contexts", context.as_str())] {
                if !value.is_empty()
                    && !entry[field]
                        .as_array()
                        .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(value)))
                {
                    entry[field].as_array_mut().unwrap().push(json!(value));
                }
            }
            if context == "hotspots" {
                entry["fromHotspots"] = json!(true);
            }
            if entry["examples"].as_array().map_or(0, Vec::len) < 6 {
                entry["examples"].as_array_mut().unwrap().push(
                    json!({"label":label,"category":category,"percent":percent,"context":context}),
                );
            }
        };

        for result in paths["categories"].as_array().into_iter().flatten() {
            let category = result["category"].as_str().unwrap_or_default();
            for frame in result["frames"].as_array().into_iter().flatten() {
                add(frame, format!("frames:{category}"), category);
            }
            for chain in result["callChains"].as_array().into_iter().flatten() {
                for frame in chain["path"].as_array().into_iter().flatten() {
                    add(frame, format!("callChain:{category}"), category);
                }
            }
            for path in result["dominantPaths"].as_array().into_iter().flatten() {
                for frame in path["frames"].as_array().into_iter().flatten() {
                    add(frame, format!("dominantPath:{category}"), category);
                }
                for branch in path["branchPoints"].as_array().into_iter().flatten() {
                    for child in branch["children"].as_array().into_iter().flatten() {
                        add(child, format!("branch:{category}"), category);
                    }
                }
            }
        }
        for hotspot in report.summary.top_hotspots.iter().take(limit.max(16) * 3) {
            let frame = serde_json::to_value(hotspot).unwrap_or(Value::Null);
            add(
                &frame,
                "hotspots".into(),
                &classify_hotspot(&hotspot.label, &hotspot.thread),
            );
        }
    }

    let mut result = families
        .into_values()
        .filter_map(|mut entry| {
            let path_count = entry["contexts"].as_array().map_or(0, Vec::len);
            (entry["occurrences"].as_u64().unwrap_or_default() >= 3 && path_count >= 2).then(|| {
                entry["pathCount"] = json!(path_count);
                if let Some(examples) = entry["examples"].as_array_mut() {
                    examples.sort_by(|left, right| {
                        right["percent"]
                            .as_f64()
                            .unwrap_or_default()
                            .total_cmp(&left["percent"].as_f64().unwrap_or_default())
                    });
                }
                entry
            })
        })
        .collect::<Vec<_>>();
    result.sort_by(|left, right| {
        right["occurrences"]
            .as_u64()
            .unwrap_or_default()
            .cmp(&left["occurrences"].as_u64().unwrap_or_default())
            .then_with(|| {
                f64_at(right, "maxPercent")
                    .unwrap_or_default()
                    .total_cmp(&f64_at(left, "maxPercent").unwrap_or_default())
            })
    });
    result.truncate(limit);
    result
}

fn worst_window(windows: &Value) -> Option<&Value> {
    windows
        .get("worstByMaxMspt")
        .or_else(|| windows.get("worstWindows"))
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
}

fn group_by_kind(links: &[Value]) -> Value {
    let mut grouped: Map<String, Value> = Map::new();
    for link in links {
        let kind = link["kind"].as_str().unwrap_or("other").to_owned();
        grouped
            .entry(kind)
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .expect("inserted array")
            .push(link.clone());
    }
    Value::Object(grouped)
}

pub(super) fn evidence_links(report: &Report, limit: usize) -> Value {
    let groups = hotspot_groups(report, limit.max(16));
    let internal_limit = limit.saturating_mul(4).clamp(64, 256);
    let sources = mod_sources(report, internal_limit);
    let chunks = entity_chunks(report, limit.max(24));
    let windows = crate::windows::worst_windows(report, 6);
    let paths = crate::hot_paths::execute(report, "auto", internal_limit);
    let memory = crate::memory_gc::summarize_memory_gc(report);
    let worst = worst_window(&windows);
    let mut links = Vec::new();

    for family in collect_recurring_families(&paths, report, limit) {
        let categories = family["categories"].clone();
        let examples = family["examples"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|example| {
                format!(
                    "{}:{} {:.2}%",
                    example["category"].as_str().unwrap_or("unknown"),
                    example["label"].as_str().unwrap_or("unknown"),
                    example["percent"].as_f64().unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        links.push(json!({
            "kind":"recurring_frame_family",
            "id":family["id"],
            "title":format!("{} appears as a repeated frame family across hot paths",family["name"].as_str().unwrap_or("unknown")),
            "strength":confidence([
                family["occurrences"].as_u64().unwrap_or_default() >= 4,
                family["categories"].as_array().map_or(0,Vec::len) >= 2 || family["pathCount"].as_u64().unwrap_or_default() >= 3,
                family["maxPercent"].as_f64().unwrap_or_default() >= 1.0,
            ]),
            "categories":categories,
            "evidenceSources":evidence_sources(["hot_paths",if family["fromHotspots"].as_bool().unwrap_or(false){"hotspots"}else{""}]),
            "evidence":[
                format!("{}: {} frames across {} hot path contexts, max {:.2}%",family["name"].as_str().unwrap_or("unknown"),family["occurrences"].as_u64().unwrap_or_default(),family["pathCount"].as_u64().unwrap_or_default(),family["maxPercent"].as_f64().unwrap_or_default()),
                format!("examples: {examples}"),
            ],
            "interpretation":"同一帧族反复散落在多个峰/层级时，应作为横向重复模式看待；它可能比任意单个窄块更重要。",
        }));
    }

    let mod_sources_by_id = sources["topSources"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|source| source["sourceId"].as_str().map(|id| (id, source)))
        .collect::<HashMap<_, _>>();
    for source in paths
        .pointer("/attribution/topSources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(limit.saturating_mul(2))
    {
        let source_id = source["sourceId"].as_str().unwrap_or("unknown");
        let source_name = source["sourceName"].as_str().unwrap_or(source_id);
        if source_id == "unknown" || wrapper_source(source_id, source_name) {
            continue;
        }
        let mod_source = mod_sources_by_id.get(source_id).copied();
        let categories = source["categories"].as_array().cloned().unwrap_or_default();
        let matched_entities = source["matchedEntities"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|candidate| candidate["entityId"].as_str().map(str::to_owned))
            .collect::<BTreeSet<_>>();
        let category_evidence = categories
            .iter()
            .filter_map(Value::as_str)
            .filter_map(|category| {
                groups["byCategory"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|group| group["category"].as_str() == Some(category))
                    .map(|group| {
                        format!(
                            "{category} {:.2}%",
                            group["maxPercent"].as_f64().unwrap_or_default()
                        )
                    })
            })
            .collect::<Vec<_>>();
        let mut evidence = vec![format!(
            "hot_paths source {source_name} {:.2}% in {}",
            source["maxPercent"].as_f64().unwrap_or_default(),
            categories
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        )];
        if let Some(mod_source) = mod_source {
            evidence.push(format!(
                "mod_sources max {:.2}% frames: {}",
                mod_source["maxPercent"].as_f64().unwrap_or_default(),
                mod_source["frames"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .take(4)
                    .filter_map(|frame| frame["label"].as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        if !category_evidence.is_empty() {
            evidence.push(format!("hotspot_groups: {}", category_evidence.join("; ")));
        }
        links.push(json!({
            "kind": "source",
            "id": source_id,
            "title": format!("{source_name} appears across hot path evidence"),
            "strength": confidence([
                source["maxPercent"].as_f64().unwrap_or_default() >= 1.0,
                mod_source.is_some(),
                !category_evidence.is_empty(),
                false,
            ]),
            "categories": categories,
            "evidenceSources":evidence_sources(["hot_paths",if mod_source.is_some(){"mod_sources"}else{""},if !category_evidence.is_empty(){"hotspot_groups"}else{""}]),
            "evidence":evidence,
            "matchedEntities": matched_entities,
            "entityLocations": [],
            "interpretation":if mod_source.is_some(){"同一来源同时出现在 hot_paths 终端帧和 mod_sources，优先级高。"}else{"来源来自 hot_paths 终端帧；即使 mod_sources 未单独汇总，也应作为路径候选。"},
        }));
    }

    for candidate in paths
        .pointer("/attribution/entityCandidates")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(limit.saturating_mul(2))
    {
        let entity = candidate["entityId"].as_str().unwrap_or_default();
        if entity.is_empty() {
            continue;
        }
        let entity_chunks = matching_chunks(&chunks, entity);
        let has_locations = !entity_chunks.is_empty();
        links.push(json!({
            "kind":"entity", "id":entity,
            "title":format!("{entity} links terminal entity frames with world distribution"),
            "strength":confidence([candidate["confidence"]=="high",candidate["percent"].as_f64().unwrap_or_default()>=0.5,!entity_chunks.is_empty()]),
            "categories":[candidate["category"].clone()],
            "evidenceSources":evidence_sources(["hot_paths",if entity_chunks.is_empty(){""}else{"entity_chunks"}]),
            "evidence":[
                format!("hot_paths entity candidate via {}:{} {:.2}% ({})",candidate["sourceName"].as_str().or_else(||candidate["sourceId"].as_str()).unwrap_or("unknown"),candidate["label"].as_str().unwrap_or("unknown"),candidate["percent"].as_f64().unwrap_or_default(),candidate["confidence"].as_str().unwrap_or("unknown")),
                if entity_chunks.is_empty(){"entity_chunks did not show this entity type in top dense chunks".into()}else{format!("entity_chunks locations: {}",entity_chunks.len())},
            ],
            "locations":entity_chunks,
            "interpretation":if has_locations{"实体类型既在 CPU 终端帧中出现，也有现场分布线索；可以优先现场复测。"}else{"这是 CPU 类级候选，但当前报告没有给出密集现场位置。"},
        }));
    }

    for group in groups["byCategory"]
        .as_array()
        .into_iter()
        .flatten()
        .take(limit)
    {
        let category = group["category"].as_str().unwrap_or_default();
        if !matches!(
            category,
            "entity_tick"
                | "entity_ai_pathfinding"
                | "chunk_task"
                | "block_entity"
                | "commands"
                | "io"
                | "world_tick"
        ) {
            continue;
        }
        let entity_delta = worst
            .and_then(|value| f64_at(value, "deltas.entitiesFromPrevious"))
            .unwrap_or_default();
        let chunk_delta = worst
            .and_then(|value| f64_at(value, "deltas.chunksFromPrevious"))
            .unwrap_or_default();
        let has_window = match category {
            "chunk_task" => chunk_delta >= 100.0,
            "entity_tick" | "entity_ai_pathfinding" | "block_entity" => entity_delta >= 50.0,
            _ => false,
        };
        links.push(json!({
            "kind":"category_window", "id":category,
            "title":format!("{category} category against worst MSPT windows"),
            "strength":confidence([group["maxPercent"].as_f64().unwrap_or_default() >= 10.0, has_window, worst.and_then(|value| f64_at(value,"msptMax")).unwrap_or_default() >= 200.0]),
            "categories":[category],
            "evidenceSources":["hotspot_groups","worst_windows"],
            "evidence":[
                format!("hotspot_groups {category} max {:.2}%", group["maxPercent"].as_f64().unwrap_or_default()),
                worst.map(|value| format!("worst window {}: max MSPT {:.2}, entity delta {:.0}, chunk delta {:.0}", value["id"].as_str().unwrap_or("unknown"), f64_at(value,"msptMax").unwrap_or_default(),entity_delta,chunk_delta)).unwrap_or_else(|| "no worst window data".into()),
            ],
            "interpretation":"类别热点与聚合窗口形成粗粒度交叉线索；窗口并非逐栈时间戳。",
        }));
    }

    let severe_memory = memory["signals"]
        .as_array()
        .is_some_and(|signals| !signals.is_empty());
    if severe_memory && worst.is_some() {
        links.push(json!({
            "kind":"runtime_window", "id":"gc_memory_vs_spikes",
            "title":"GC/memory signals compared with worst windows",
            "strength":confidence([severe_memory, worst.is_some(), memory.pointer("/heap/usedMaxRatio").and_then(Value::as_f64).is_some_and(|ratio| ratio > 0.85)]),
            "categories":["memory_gc"],
            "evidenceSources":["memory_gc","worst_windows"],
            "evidence":[format!("memory/GC signals: {}", memory["signals"].as_array().map_or(0, Vec::len)), worst.map(|value| format!("worst window max MSPT {:.2}", f64_at(value,"msptMax").unwrap_or_default())).unwrap_or_else(|| "no worst window data".into())],
            "interpretation":"GC 聚合信号与窗口尖峰只能形成待验证联动；精确相关性需要带时间戳的 GC 日志。",
        }));
    }

    links.sort_by(|left, right| {
        confidence_rank(&right["strength"])
            .cmp(&confidence_rank(&left["strength"]))
            .then_with(|| {
                right["evidenceSources"]
                    .as_array()
                    .map_or(0, Vec::len)
                    .cmp(&left["evidenceSources"].as_array().map_or(0, Vec::len))
            })
            .then_with(|| left["kind"].as_str().cmp(&right["kind"].as_str()))
            .then_with(|| left["id"].as_str().cmp(&right["id"].as_str()))
    });
    links.truncate(limit);
    json!({
        "strongestLinks": links,
        "byKind": group_by_kind(&links),
        "selectedCategories": paths["selectedCategories"],
        "notes":[
            "A strong link means the same source/entity/category appears in multiple report views; it is still sampled evidence, not proof of a single instance.",
            "category_window links are coarse because spark time windows are not per-stack timestamps.",
            "runtime_window GC links require external GC logs for exact spike correlation."
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ReportKind, ReportSummary, StackHotspot};

    #[test]
    fn creates_real_cross_evidence_links() {
        let frame = StackHotspot {
            label: "net.minecraft.world.entity.EntityTickList.tick".into(),
            samples: 40.0,
            percent: 20.0,
            thread: "Server thread".into(),
            source: None,
            class_name: Some("net.minecraft.world.entity.EntityTickList".into()),
            method_name: Some("tick".into()),
            method_desc: None,
            line_number: None,
        };
        let report = Report {
            kind: ReportKind::Sampler,
            source: "fixture".into(),
            raw: json!({
                "classSources":{"dev.worker.WorkerEntity":"worker"},
                "metadata":{"sources":{"worker":{"name":"Worker"}},"platformStatistics":{"world":{"totalEntities":60,"entityCounts":{"worker:worker_entity":60},"worlds":[{"name":"world","regions":[{"chunks":[{"x":1,"z":2,"totalEntities":60,"entityCounts":{"worker:worker_entity":60}}]}]}]} }},
                "threads":[{"name":"Server thread","times":[100.0],"childrenRefs":[0],"children":[
                    {"className":"net.minecraft.server.level.ServerLevel","methodName":"tick","times":[100.0],"childrenRefs":[1]},
                    {"className":"net.minecraft.world.entity.EntityTickList","methodName":"tick","times":[50.0],"childrenRefs":[2]},
                    {"className":"dev.worker.WorkerEntity","methodName":"tick","times":[40.0],"childrenRefs":[]}
                ]}],
                "timeWindowStatistics":{"1":{"msptMax":300,"msptMedian":60,"tps":15}}
            }),
            summary: ReportSummary {
                title: "fixture".into(),
                top_hotspots: vec![frame],
                ..Default::default()
            },
        };
        let value = evidence_links(&report, 16);
        assert!(value["strongestLinks"]
            .as_array()
            .is_some_and(|links| !links.is_empty()));
        assert!(value["byKind"]["source"]
            .as_array()
            .is_some_and(|links| !links.is_empty()));
        assert!(value["byKind"]["recurring_frame_family"]
            .as_array()
            .is_none_or(Vec::is_empty));
        let source = value["byKind"]["source"]
            .as_array()
            .and_then(|links| links.iter().find(|link| link["id"] == "worker"))
            .unwrap();
        assert_eq!(source["matchedEntities"], json!(["worker:worker_entity"]));
        assert_eq!(source["entityLocations"], json!([]));
        assert!(value["strongestLinks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|link| link["kind"] == "entity"
                && link["evidenceSources"]
                    .as_array()
                    .is_some_and(|sources| sources.len() == 2)
                && link["locations"]
                    .as_array()
                    .is_some_and(|items| !items.is_empty())));
    }
}
