use super::evidence_links::{confidence_rank, evidence_links};
use super::{entity_chunks, evidence_gaps, hotspot_groups, mod_sources};
use crate::Report;
use serde_json::{json, Value};

fn actionable(category: &str) -> bool {
    matches!(
        category,
        "entity_tick"
            | "entity_ai_pathfinding"
            | "chunk_task"
            | "block_entity"
            | "commands"
            | "io"
            | "world_tick"
    )
}

fn category_load_profile(groups: &Value) -> Value {
    let mut categories: Vec<_> = groups["byCategory"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|group| group["category"].as_str().is_some_and(actionable))
        .cloned()
        .collect();
    categories.sort_by(|left, right| {
        right["maxPercent"]
            .as_f64()
            .unwrap_or_default()
            .total_cmp(&left["maxPercent"].as_f64().unwrap_or_default())
    });
    let dominant = categories.first().cloned().unwrap_or(Value::Null);
    let dominant_percent = dominant["maxPercent"].as_f64().unwrap_or_default();
    let major: Vec<_> = categories
        .iter()
        .filter(|group| {
            let percent = group["maxPercent"].as_f64().unwrap_or_default();
            percent >= 10.0 || (dominant_percent > 0.0 && percent >= dominant_percent * 0.25)
        })
        .take(8)
        .cloned()
        .collect();
    let secondary: Vec<_> = categories
        .iter()
        .filter(|group| {
            let percent = group["maxPercent"].as_f64().unwrap_or_default();
            percent >= 3.0
                && !major
                    .iter()
                    .any(|item| item["category"] == group["category"])
        })
        .take(8)
        .cloned()
        .collect();
    json!({
        "dominant":dominant,
        "majorCategories":major,
        "secondaryCategories":secondary,
        "rule":"major = maxPercent >= 10% or at least 25% of dominant category; secondary = remaining actionable categories >= 3%."
    })
}

fn hypothesis_rank(value: &Value) -> i32 {
    confidence_rank(&value["confidence"])
}

fn confidence(checks: impl IntoIterator<Item = bool>) -> &'static str {
    match checks.into_iter().filter(|value| *value).count() {
        3.. => "high",
        2 => "medium",
        _ => "low",
    }
}

fn category<'a>(groups: &'a Value, name: &str) -> Option<&'a Value> {
    groups["byCategory"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|group| group["category"].as_str() == Some(name))
}

fn wrapper_source(source_id: &str, source_name: &str) -> bool {
    let value = format!("{source_id} {source_name}").to_ascii_lowercase();
    let tokens = value
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    [
        "neruina",
        "observable",
        "mixin",
        "wrapper",
        "bridge",
        "hook",
    ]
    .iter()
    .any(|token| tokens.contains(token))
}

fn source_has_server_frame(source: &Value) -> bool {
    source["frames"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|frame| {
            frame["thread"]
                .as_str()
                .is_some_and(crate::analysis::is_server_thread_name)
        })
}

fn chunk_evidence(chunk: &Value) -> String {
    format!(
        "{} ({}, {}): {} entities [{}]",
        chunk["world"].as_str().unwrap_or("unknown"),
        chunk["x"],
        chunk["z"],
        chunk["totalEntities"],
        chunk["topEntities"]
            .as_array()
            .into_iter()
            .flatten()
            .take(5)
            .map(|entity| format!(
                "{}={}",
                entity["name"].as_str().unwrap_or("unknown"),
                entity["value"]
            ))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn matching_source_chunks<'a>(source: &Value, chunks: &'a Value) -> Vec<&'a Value> {
    let id = source["sourceId"]
        .as_str()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let name = source["name"]
        .as_str()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let name_tokens = name
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
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
                    let entity = entity["name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_ascii_lowercase();
                    let namespace = entity.split(':').next().unwrap_or_default();
                    (!id.is_empty() && namespace == id)
                        || (!name.is_empty() && name_tokens.contains(&namespace))
                })
        })
        .collect()
}

fn source_entity_frames(source: &Value, chunks: &[&Value]) -> Vec<String> {
    let tokens = chunks
        .iter()
        .flat_map(|chunk| chunk["topEntities"].as_array().into_iter().flatten())
        .filter_map(|entity| entity["name"].as_str())
        .map(|entity| {
            entity
                .rsplit(':')
                .next()
                .unwrap_or(entity)
                .replace('_', "")
                .to_ascii_lowercase()
        })
        .filter(|token| token.len() >= 4)
        .collect::<Vec<_>>();
    source["frames"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|frame| {
            frame["thread"]
                .as_str()
                .is_some_and(crate::analysis::is_server_thread_name)
        })
        .filter_map(|frame| frame["label"].as_str())
        .filter(|label| {
            let label = label.replace(['.', '_'], "").to_ascii_lowercase();
            tokens.iter().any(|token| label.contains(token))
        })
        .take(6)
        .map(str::to_owned)
        .collect()
}

pub(super) fn diagnostic_hypotheses(report: &Report) -> Value {
    let groups = hotspot_groups(report, 12);
    let profile = category_load_profile(&groups);
    let sources = mod_sources(report, 200);
    let chunks = entity_chunks(report, 12);
    let windows = crate::windows::worst_windows(report, 6);
    let paths = crate::hot_paths::execute(report, "auto", 64);
    let gaps = evidence_gaps(report);
    let links = evidence_links(report, 12);
    let memory = crate::memory_gc::summarize_memory_gc(report);
    let mut hypotheses = Vec::new();

    if profile["majorCategories"]
        .as_array()
        .is_some_and(|items| !items.is_empty())
    {
        let majors = profile["majorCategories"].as_array().unwrap();
        hypotheses.push(json!({
            "id":"server_thread_category_load_profile",
            "confidence":if majors.len() >= 2 {"high"} else {"medium"},
            "conclusion":if majors.len() >= 2 {"主线程负载由多个显著热点类别共同构成，结论不能只写最高一类"} else {"主线程负载有一个明显最高热点类别，但仍需检查次级类别"},
            "evidence":[
                format!("dominant: {} {:.2}%", profile["dominant"]["category"].as_str().unwrap_or("none"), profile["dominant"]["maxPercent"].as_f64().unwrap_or_default()),
                format!("major categories: {}", majors.iter().map(|item| format!("{} {:.2}%", item["category"].as_str().unwrap_or("unknown"), item["maxPercent"].as_f64().unwrap_or_default())).collect::<Vec<_>>().join(", ")),
            ],
            "limitations":["类别占比来自 sampled inclusive frames；它表示主线程热点分布，不等于精确独占 CPU。","最高类别可以称为主导项，但不能覆盖其他超过显著阈值的类别。"],
            "nextActions":["最终结论按 dominant / major / secondary 分层列出。","对每个 major category 引用 hot_paths 终端帧或说明未解析到终端来源。"]
        }));
    }

    let entity_group = category(&groups, "entity_tick");
    let block_entity_group = category(&groups, "block_entity");
    let chunk_group = category(&groups, "chunk_task");
    let hot_sources = paths
        .pointer("/attribution/topSources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|source| {
            let id = source["sourceId"].as_str().unwrap_or("unknown");
            let name = source["sourceName"].as_str().unwrap_or(id);
            id != "unknown" && !wrapper_source(id, name)
        })
        .take(12)
        .collect::<Vec<_>>();
    let category_evidence = paths
        .pointer("/attribution/byCategory")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|entry| {
            let source_text = entry["topSources"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|source| {
                    let id = source["sourceId"].as_str().unwrap_or("unknown");
                    !wrapper_source(id, source["sourceName"].as_str().unwrap_or(id))
                })
                .take(6)
                .map(|source| {
                    format!(
                        "{} {:.2}%",
                        source["sourceName"]
                            .as_str()
                            .or_else(|| source["sourceId"].as_str())
                            .unwrap_or("unknown"),
                        source["maxPercent"].as_f64().unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join(" | ");
            let entity_text = entry["entityCandidates"]
                .as_array()
                .into_iter()
                .flatten()
                .take(8)
                .map(|candidate| {
                    format!(
                        "{} {:.2}%",
                        candidate["entityId"].as_str().unwrap_or("unknown"),
                        candidate["percent"].as_f64().unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            let dominant = entry["dominantPaths"]
                .as_array()
                .into_iter()
                .flatten()
                .take(3)
                .map(|path| {
                    path["frames"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|frame| {
                            format!(
                                "{}:{} {:.2}%",
                                frame["sourceName"]
                                    .as_str()
                                    .or_else(|| frame["sourceId"].as_str())
                                    .unwrap_or("unknown"),
                                frame["label"].as_str().unwrap_or("unknown"),
                                frame["percent"].as_f64().unwrap_or_default()
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(" -> ")
                })
                .collect::<Vec<_>>()
                .join(" || ");
            format!(
                "{}: terminalSources: {}{}{}",
                entry["category"].as_str().unwrap_or("unknown"),
                if source_text.is_empty() {
                    "无非 wrapper 模组来源"
                } else {
                    &source_text
                },
                if entity_text.is_empty() {
                    String::new()
                } else {
                    format!(" entityCandidates: {entity_text}")
                },
                if dominant.is_empty() {
                    String::new()
                } else {
                    format!(" dominantPaths: {dominant}")
                }
            )
        })
        .collect::<Vec<_>>();
    if !hot_sources.is_empty() {
        let mut evidence = vec![format!(
            "selectedCategories: {}",
            paths["selectedCategories"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        )];
        evidence.extend(category_evidence);
        evidence.extend(hot_sources.iter().take(8).map(|source| {
            format!(
                "global: {} max {:.2}% categories {} frames: {}",
                source["sourceName"]
                    .as_str()
                    .or_else(|| source["sourceId"].as_str())
                    .unwrap_or("unknown"),
                source["maxPercent"].as_f64().unwrap_or_default(),
                source["categories"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", "),
                source["terminalFrames"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .take(3)
                    .filter_map(|frame| frame["label"].as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        }));
        hypotheses.push(json!({
            "id":"hot_path_terminal_sources",
            "confidence":confidence([hot_sources.iter().any(|source|source["maxPercent"].as_f64().unwrap_or_default()>=1.0),entity_group.is_some()||block_entity_group.is_some()||chunk_group.is_some(),hot_sources.iter().any(|source|source["terminalFrames"].as_array().is_some_and(|frames|!frames.is_empty()))]),
            "conclusion":"hot_paths 已按高占用类别下钻到具体终端模组/类；每个 selected category 都必须独立看",
            "evidence":evidence,
            "limitations":["这些来源来自 hot_paths terminal frames，是性能路径候选；不要求 mod_sources 再次汇总到同一来源才成立。","全局排序不能替代逐类别下钻；entity_tick、chunk_task、block_entity 等高占用类别必须分别解释。","普通 sampler 仍不能证明单个实例或单个坐标；但这些模组/类应优先检查和复测。"],
            "nextActions":["按 hot_paths terminal source 优先做 A/B 复测或配置隔离。","对 entity_tick 终端实体类，结合 entity_chunks 和 only-ticks-over 捕获确认具体场景。"]
        }));
    }

    let entity_candidates = paths
        .pointer("/attribution/entityCandidates")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|candidate| candidate["entityId"].as_str().is_some())
        .take(16)
        .collect::<Vec<_>>();
    if !entity_candidates.is_empty() {
        hypotheses.push(json!({
            "id":"hot_path_entity_candidates",
            "confidence":confidence([entity_candidates.iter().any(|candidate|candidate["confidence"]=="high"),entity_candidates.iter().any(|candidate|candidate["percent"].as_f64().unwrap_or_default()>=0.5),entity_group.is_some()]),
            "conclusion":"hot_paths 已把部分实体 tick 终端帧匹配到具体实体/生物候选",
            "evidence":entity_candidates.iter().take(8).map(|candidate|format!("{} via {}:{} {:.2}% ({}, {})",candidate["entityId"].as_str().unwrap_or("unknown"),candidate["sourceName"].as_str().or_else(||candidate["sourceId"].as_str()).unwrap_or("unknown"),candidate["label"].as_str().unwrap_or("unknown"),candidate["percent"].as_f64().unwrap_or_default(),candidate["confidence"].as_str().unwrap_or("unknown"),candidate["reason"].as_str().unwrap_or("no reason"))).collect::<Vec<_>>(),
            "limitations":["这是实体类型/类级别归因，不是单个实体 UUID。","如果 entity_chunks 中没有对应密集现场，也仍可能是少量高耗实体逻辑；需要 only-ticks-over 复测。"],
            "nextActions":["优先在服务器里定位这些实体类型出现的位置，减少/隔离后重采 profile。","若无法复现，采 /spark profiler --only-ticks-over 50 --timeout 120。"]
        }));
    }

    let source_candidates = sources[if sources["notableSources"]
        .as_array()
        .is_some_and(|items| !items.is_empty())
    {
        "notableSources"
    } else {
        "topSources"
    }]
    .as_array()
    .into_iter()
    .flatten()
    .filter(|source| {
        source["sourceId"].as_str() != Some("unknown") && source_has_server_frame(source)
    })
    .take(32)
    .collect::<Vec<_>>();
    for source in source_candidates {
        let server_frames = source["frames"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|frame| {
                frame["thread"]
                    .as_str()
                    .is_some_and(crate::analysis::is_server_thread_name)
            })
            .collect::<Vec<_>>();
        let server_max_percent = server_frames
            .iter()
            .filter_map(|frame| frame["percent"].as_f64())
            .fold(0.0, f64::max);
        if server_max_percent < 5.0 {
            continue;
        }
        let matched_chunks = matching_source_chunks(source, &chunks);
        let entity_frames = source_entity_frames(source, &matched_chunks);
        hypotheses.push(json!({
            "id":format!("mod_source_hotspot:{}",source["sourceId"].as_str().unwrap_or("unknown")),
            "confidence":confidence([true,block_entity_group.is_some()||entity_group.is_some(),!entity_frames.is_empty()]),
            "conclusion":format!("{} 有可引用的采样热点；实体分布若仅同命名空间，只能作为现场线索",source["name"].as_str().or_else(||source["sourceId"].as_str()).unwrap_or("unknown")),
            "evidence":[format!("mod_sources server-thread max {:.2}%: {} frames: {}",server_max_percent,source["name"].as_str().or_else(||source["sourceId"].as_str()).unwrap_or("unknown"),server_frames.iter().take(6).filter_map(|frame|frame["label"].as_str()).collect::<Vec<_>>().join("; ")),if entity_frames.is_empty(){"未看到能绑定到同实体类型的 CPU 帧".into()}else{format!("同实体类型 CPU 帧: {}",entity_frames.join("; "))},if matched_chunks.is_empty(){"未看到同命名空间实体区块".into()}else{format!("同命名空间实体区块（现场线索，不是 CPU 证据）: {}",matched_chunks.iter().take(3).map(|chunk|chunk_evidence(chunk)).collect::<Vec<_>>().join(" | "))}],
            "limitations":["当前报告能证明这些来源帧参与采样热点；普通 sampler 仍不能锁定单个方块实体或实体实例。","entity_chunks 只能证明某区块/实体类型堆积；除非 CPU 帧出现同实体类型，否则不能写成直接成因。"],
            "nextActions":["优先检查已采样到的具体热点类/方法对应系统并做 A/B 复测。","若要锁定具体机器，站在该区块附近捕获 only-ticks-over。"]
        }));
    }

    if block_entity_group
        .is_some_and(|group| group["maxPercent"].as_f64().unwrap_or_default() >= 10.0)
    {
        let block_paths = crate::hot_paths::execute(report, "block_entity", 12);
        hypotheses.push(json!({
            "id":"block_entity_hot_path",
            "confidence":confidence([block_entity_group.is_some_and(|group|group["maxPercent"].as_f64().unwrap_or_default()>=25.0),block_paths["frames"].as_array().is_some_and(|frames|!frames.is_empty()),block_paths["frames"].as_array().into_iter().flatten().any(|frame|frame["sourceId"].as_str().is_some_and(|id|id!="unknown"))]),
            "conclusion":"方块实体 tick 是主线程热点路径，必须进入结论",
            "evidence":[format!("block_entity 类热点最高约 {:.2}%",block_entity_group.and_then(|group|group["maxPercent"].as_f64()).unwrap_or_default()),format!("hot_paths(block_entity): {}",block_paths["frames"].as_array().into_iter().flatten().take(8).map(|frame|format!("{}:{} {:.2}%",frame["sourceName"].as_str().or_else(||frame["sourceId"].as_str()).unwrap_or("unknown"),frame["label"].as_str().unwrap_or("unknown"),frame["maxPercent"].as_f64().unwrap_or_default())).collect::<Vec<_>>().join("; "))],
            "limitations":["这能证明方块实体 tick 路径有采样热点，但普通 sampler 不能直接给出具体方块坐标。"],
            "nextActions":["优先检查 hot_paths 中出现的方块实体类型和机器链路。","在疑似机器区附近采 only-ticks-over。"]
        }));
    }

    if let Some(dense) = chunks["topChunks"]
        .as_array()
        .and_then(|items| items.first())
        .filter(|chunk| chunk["totalEntities"].as_i64().unwrap_or_default() >= 50)
    {
        hypotheses.push(json!({
            "id":"high_density_entity_chunk",
            "confidence":confidence([dense["totalEntities"].as_i64().unwrap_or_default()>=80,entity_group.is_some(),dense["riskSignals"].as_array().is_some_and(|signals|!signals.is_empty())]),
            "conclusion":"存在明确的实体密集区块，必须作为首批现场检查点",
            "evidence":[format!("Top chunk: {}",chunk_evidence(dense)),format!("Risk signals: {}",dense["riskSignals"].as_array().into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")),format!("全局实体总数 {:?}",report.summary.entity_count)],
            "limitations":["实体密度能定位现场，但不能单独证明 CPU 独占耗时；需要同实体类型 CPU 帧或 only-ticks-over 对齐。"],
            "nextActions":["定位到该 chunk，清理异常堆积实体后以相同负载复测。"]
        }));
    }

    if let Some(group) = entity_group {
        hypotheses.push(json!({
            "id":"entity_tick_load",
            "confidence":confidence([group["maxPercent"].as_f64().unwrap_or_default()>15.0,chunks["topEntityTypes"].as_array().is_some_and(|items|!items.is_empty())]),
            "conclusion":"主线程实体 tick 是重要负载来源",
            "evidence":[format!("entity_tick 类热点最高约 {:.2}%",group["maxPercent"].as_f64().unwrap_or_default()),format!("实体总量 {:?}",report.summary.entity_count),format!("Top entity types: {}",chunks["topEntityTypes"].as_array().into_iter().flatten().take(6).map(|item|format!("{}={}",item["name"].as_str().unwrap_or("unknown"),item["value"])).collect::<Vec<_>>().join(", "))],
            "limitations":["spark profile cannot identify a single entity instance unless per-chunk/entity context aligns with hotspot frames."],
            "nextActions":["Use entity_chunks to inspect top chunks in-game.","Capture only-ticks-over near suspected chunks."]
        }));
    }

    let worst = windows["worstByMaxMspt"]
        .as_array()
        .and_then(|items| items.first());
    if chunk_group.is_some()
        || worst
            .and_then(|window| window.pointer("/deltas/chunksFromPrevious"))
            .and_then(Value::as_f64)
            .unwrap_or_default()
            > 500.0
    {
        hypotheses.push(json!({
            "id":"chunk_task_or_generation_spike",
            "confidence":confidence([chunk_group.is_some(),worst.and_then(|window|window["msptMax"].as_f64()).unwrap_or_default()>200.0,worst.and_then(|window|window.pointer("/deltas/chunksFromPrevious")).and_then(Value::as_f64).unwrap_or_default()>300.0]),
            "conclusion":"卡顿尖峰与 chunk 任务/加载/生成相关",
            "evidence":[chunk_group.map(|group|format!("chunk_task 类热点最高约 {:.2}%",group["maxPercent"].as_f64().unwrap_or_default())).unwrap_or_else(||"没有明确 chunk_task 热点，但窗口 chunk 数变化需要关注".into()),worst.map(|window|format!("最坏窗口 {}: max MSPT {:.2}, chunks delta {}",window["id"].as_str().unwrap_or("unknown"),window["msptMax"].as_f64().unwrap_or_default(),window.pointer("/deltas/chunksFromPrevious").unwrap_or(&Value::Null))).unwrap_or_else(||"无窗口数据".into())],
            "limitations":["time window is coarse; it cannot bind a specific chunk task to an exact stack sample."],
            "nextActions":["Lower view-distance/simulation-distance or pregen suspected areas, then compare worst_windows.","Capture only-ticks-over during exploration/worldgen."]
        }));
    }
    if let (Some(c2me), Some(group)) = (
        sources["topSources"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|source| {
                source["sourceId"]
                    .as_str()
                    .is_some_and(|id| id.to_ascii_lowercase().contains("c2me"))
            }),
        chunk_group,
    ) {
        hypotheses.push(json!({
            "id":"c2me_chunk_io_path",
            "confidence":confidence([c2me["maxPercent"].as_f64().unwrap_or_default()>3.0,group["maxPercent"].as_f64().unwrap_or_default()>8.0,worst.and_then(|window|window["msptMax"].as_f64()).unwrap_or_default()>200.0]),
            "conclusion":"尖峰与 C2ME/chunk IO 主线程任务路径相关，但当前报告不能唯一锁定触发区块",
            "evidence":[format!("mod_sources: {} max {:.2}%",c2me["sourceId"].as_str().unwrap_or("c2me"),c2me["maxPercent"].as_f64().unwrap_or_default()),format!("chunk_task 类热点最高约 {:.2}%",group["maxPercent"].as_f64().unwrap_or_default())],
            "limitations":["普通 sampler 只显示采样期间有 chunk IO/任务等待，不能把单次尖峰绑定到具体 chunk。"],
            "nextActions":["复现移动/加载区域时捕获 only-ticks-over。","同时记录玩家坐标和移动路线并和窗口对齐。"]
        }));
    }

    let gc_signals = memory["signals"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|signal| signal["category"] == "gc" && signal["severity"] != "info")
        .collect::<Vec<_>>();
    let memory_signals = memory["signals"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|signal| signal["category"] == "memory" && signal["severity"] != "info")
        .collect::<Vec<_>>();
    if !gc_signals.is_empty() || !memory_signals.is_empty() {
        hypotheses.push(json!({
            "id":"memory_gc_pressure",
            "confidence":confidence([gc_signals.iter().any(|signal|signal["severity"]=="critical"),!gc_signals.is_empty(),!memory_signals.is_empty()]),
            "conclusion":if gc_signals.is_empty(){"内存池压力存在异常信号，但需要结合 GC/时间窗口判断影响"}else{"GC 暂停/频率存在异常信号，可能解释无明显 CPU 热点时的卡顿尖峰"},
            "evidence":memory["signals"],
            "limitations":["spark 聚合 GC 统计能证明 GC 行为异常，但不能把某一次 tick 尖峰精确绑定到某一次 STW 暂停。"],
            "nextActions":["用带时间戳 GC 日志和 worst_windows 对齐。","若 Old/Full GC 平均暂停高，优先检查堆配置与对象 churn。"]
        }));
    }
    let heap_ratio = match (
        report.summary.heap_used_bytes,
        report.summary.heap_max_bytes,
    ) {
        (Some(used), Some(max)) if max > 0 => used as f64 / max as f64,
        _ => 0.0,
    };
    if gc_signals.is_empty() && (heap_ratio > 0.75 || report.summary.gc.is_empty()) {
        hypotheses.push(json!({
            "id":"gc_pause_possible_but_unproven",
            "confidence":if heap_ratio>0.85{"medium"}else{"low"},
            "conclusion":"GC/内存停顿不能由当前报告证明",
            "evidence":[format!("heap usage {:?} / {:?}",report.summary.heap_used_bytes,report.summary.heap_max_bytes),format!("GC data present: {}",!report.summary.gc.is_empty())],
            "limitations":["Current spark profile does not include per-pause GC log correlation."],
            "nextActions":["Collect spark health report with GC section or JVM GC log around the spike."]
        }));
    }

    if hypotheses.is_empty() {
        hypotheses.push(json!({
            "id":"no_strong_local_signal", "confidence":"low",
            "conclusion":"当前报告没有形成强本地诊断",
            "evidence":report.summary.findings,
            "limitations":["报告可能缺少时间窗口、来源映射或深层调用路径。"],
            "nextActions":["按 evidenceGaps 补采，并重新运行 hot_paths。"]
        }));
    }
    hypotheses.sort_by_key(|hypothesis| std::cmp::Reverse(hypothesis_rank(hypothesis)));

    json!({
        "hypotheses":hypotheses,
        "strongest":hypotheses.first().cloned().unwrap_or(Value::Null),
        "categoryLoadProfile":profile,
        "evidenceLinks":links["strongestLinks"],
        "evidenceGaps":gaps,
        "selectedCategories":paths["selectedCategories"],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ReportKind, ReportSummary, StackHotspot};

    #[test]
    fn exposes_diagnostic_contract_and_major_categories() {
        let frames = vec![
            StackHotspot {
                label: "net.minecraft.world.entity.EntityTickList.tick".into(),
                samples: 30.0,
                percent: 30.0,
                thread: "Server thread".into(),
                source: None,
                class_name: None,
                method_name: None,
                method_desc: None,
                line_number: None,
            },
            StackHotspot {
                label: "net.minecraft.server.level.ServerChunkCache.tick".into(),
                samples: 15.0,
                percent: 15.0,
                thread: "Server thread".into(),
                source: None,
                class_name: None,
                method_name: None,
                method_desc: None,
                line_number: None,
            },
        ];
        let report = Report {
            kind: ReportKind::Sampler,
            source: "fixture".into(),
            raw: json!({}),
            summary: ReportSummary {
                title: "fixture".into(),
                top_hotspots: frames,
                ..Default::default()
            },
        };
        let value = diagnostic_hypotheses(&report);
        assert!(value["strongest"].is_object());
        assert_eq!(
            value["categoryLoadProfile"]["majorCategories"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert!(value["evidenceLinks"].is_array());
        assert!(value["evidenceGaps"].is_object());
    }

    #[test]
    fn restores_hot_path_entity_block_chunk_and_gc_hypotheses() {
        let hotspot = |label: &str, class_name: &str, samples: f64, percent: f64| StackHotspot {
            label: label.into(),
            samples,
            percent,
            thread: "Server thread".into(),
            source: None,
            class_name: Some(class_name.into()),
            method_name: Some("tick".into()),
            method_desc: Some("()V".into()),
            line_number: None,
        };
        let report = Report {
            kind: ReportKind::Sampler,
            source: "rich-fixture".into(),
            raw: json!({
                "classSources":{
                    "dev.worker.WorkerEntity":"worker",
                    "com.simibubi.create.MechanicalPress":"create",
                    "com.ishland.c2me.ChunkTask":"c2me"
                },
                "metadata":{
                    "sources":{
                        "worker":{"name":"Worker"},
                        "create":{"name":"Create"},
                        "c2me":{"name":"C2ME"}
                    },
                    "platformStatistics":{"world":{
                        "totalEntities":80,
                        "entityCounts":{"worker:worker_entity":80},
                        "worlds":[{"name":"world","regions":[{"chunks":[{
                            "x":1,"z":2,"totalEntities":80,
                            "entityCounts":{"worker:worker_entity":80}
                        }]}]}]
                    }}
                },
                "threads":[{"name":"Server thread","times":[100.0],"childrenRefs":[0],"children":[
                    {"className":"net.minecraft.server.level.ServerLevel","methodName":"tick","times":[100.0],"childrenRefs":[1,3,5]},
                    {"className":"net.minecraft.world.entity.EntityTickList","methodName":"tick","times":[30.0],"childrenRefs":[2]},
                    {"className":"dev.worker.WorkerEntity","methodName":"tick","methodDesc":"()V","times":[25.0],"childrenRefs":[]},
                    {"className":"net.minecraft.world.level.block.entity.TickingBlockEntity","methodName":"tick","times":[30.0],"childrenRefs":[4]},
                    {"className":"com.simibubi.create.MechanicalPress","methodName":"tick","methodDesc":"()V","times":[25.0],"childrenRefs":[]},
                    {"className":"net.minecraft.server.level.ChunkTaskPriorityQueueSorter","methodName":"tick","times":[30.0],"childrenRefs":[6]},
                    {"className":"com.ishland.c2me.ChunkTask","methodName":"tick","methodDesc":"()V","times":[25.0],"childrenRefs":[]}
                ]}],
                "timeWindowStatistics":{
                    "1":{"msptMax":80.0,"chunks":100.0},
                    "2":{"msptMax":300.0,"chunks":600.0}
                }
            }),
            summary: ReportSummary {
                title: "rich-fixture".into(),
                entity_count: Some(80),
                top_hotspots: vec![
                    hotspot(
                        "net.minecraft.world.entity.EntityTickList.tick",
                        "net.minecraft.world.entity.EntityTickList",
                        30.0,
                        30.0,
                    ),
                    hotspot(
                        "net.minecraft.world.level.block.entity.TickingBlockEntity.tick",
                        "net.minecraft.world.level.block.entity.TickingBlockEntity",
                        30.0,
                        30.0,
                    ),
                    hotspot(
                        "com.ishland.c2me.ChunkTask.tick",
                        "com.ishland.c2me.ChunkTask",
                        25.0,
                        25.0,
                    ),
                ],
                ..Default::default()
            },
        };
        let value = diagnostic_hypotheses(&report);
        let ids = value["hypotheses"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|hypothesis| hypothesis["id"].as_str())
            .collect::<Vec<_>>();
        for expected in [
            "hot_path_terminal_sources",
            "hot_path_entity_candidates",
            "block_entity_hot_path",
            "high_density_entity_chunk",
            "entity_tick_load",
            "chunk_task_or_generation_spike",
            "c2me_chunk_io_path",
            "gc_pause_possible_but_unproven",
        ] {
            assert!(ids.contains(&expected), "missing {expected}: {ids:?}");
        }
    }
}
