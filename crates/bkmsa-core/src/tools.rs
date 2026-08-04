use crate::analysis::{
    arr_at, classify_hotspot, compact, f64_at, format_bytes, i64_at, obj_at, path, str_at,
};
use crate::{Report, SparkError, ToolDescription, ToolRequest, ToolResult};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

mod diagnostic_hypotheses;
mod entity_chunks;
mod evidence_links;
mod source_attribution;

use diagnostic_hypotheses::diagnostic_hypotheses;
use entity_chunks::entity_chunks;
use evidence_links::evidence_links;
use source_attribution::mod_sources;

pub fn report_tool_descriptions() -> Vec<ToolDescription> {
    vec![
        ToolDescription {
            name: "overview",
            args: json!({}),
            description: "关键指标、本地阈值发现、GC 摘要",
        },
        ToolDescription {
            name: "environment",
            args: json!({}),
            description: "报告内记录的平台、系统、Java/JVM、服务器配置和来源清单",
        },
        ToolDescription {
            name: "hotspots",
            args: json!({"limit":16}),
            description: "CPU profile 热点帧",
        },
        ToolDescription {
            name: "hotspot_groups",
            args: json!({"limit":20}),
            description: "按类别、包名、线程聚合 CPU 热点",
        },
        ToolDescription {
            name: "hot_paths",
            args: json!({"category":"auto","limit":64}),
            description: "选择热点类别并展开具体类和功能帧",
        },
        ToolDescription {
            name: "mod_sources",
            args: json!({"limit":24}),
            description: "利用来源映射做模组/插件归因",
        },
        ToolDescription {
            name: "time_windows",
            args: json!({"limit":50}),
            description: "spark 时间窗口统计",
        },
        ToolDescription {
            name: "worst_windows",
            args: json!({"limit":12}),
            description: "按 MSPT/TPS 排序的最坏窗口",
        },
        ToolDescription {
            name: "entities",
            args: json!({}),
            description: "实体总量、实体排行、世界摘要",
        },
        ToolDescription {
            name: "entity_chunks",
            args: json!({"limit":24}),
            description: "世界/区块实体热点",
        },
        ToolDescription {
            name: "heap",
            args: json!({"limit":24}),
            description: "heap summary 对象排行",
        },
        ToolDescription {
            name: "memory_gc",
            args: json!({}),
            description: "堆/内存池/GC 聚合统计和异常信号",
        },
        ToolDescription {
            name: "evidence_links",
            args: json!({"limit":16}),
            description: "串联热点、来源、实体和时间窗口证据",
        },
        ToolDescription {
            name: "diagnostic_hypotheses",
            args: json!({}),
            description: "本地规则生成诊断假设、证据、反证和动作",
        },
        ToolDescription {
            name: "evidence_gaps",
            args: json!({}),
            description: "当前报告能/不能证明什么及建议补采",
        },
        ToolDescription {
            name: "raw_field",
            args: json!({"path":"metadata.platformStatistics","maxItems":80}),
            description: "读取指定 raw 字段",
        },
    ]
}

fn limit(args: &Value, default: usize) -> usize {
    args.get("limit")
        .and_then(Value::as_u64)
        .map(|n| n.clamp(1, 1000) as usize)
        .unwrap_or(default)
}
pub fn execute_tool_request(
    report: &Report,
    request: ToolRequest,
) -> Result<ToolResult, SparkError> {
    execute_tool(report, &request.tool, request.args)
}
pub fn execute_tool(report: &Report, tool: &str, args: Value) -> Result<ToolResult, SparkError> {
    if !args.is_object() {
        return Err(SparkError::InvalidArgument(
            "tool arguments must be a JSON object".into(),
        ));
    }
    if tool == "raw_field"
        && args
            .get("path")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
    {
        return Err(SparkError::InvalidArgument(
            "raw_field.path is required".into(),
        ));
    }
    Ok(match tool {
        "report_inventory" => inventory(report),
        "overview" => overview(report),
        "environment" => crate::environment::summarize_environment(report),
        "hotspots" => {
            json!({"limit":limit(&args,16),"hotspots":report.summary.top_hotspots.iter().take(limit(&args,16)).collect::<Vec<_>>() })
        }
        "hotspot_groups" => hotspot_groups(report, limit(&args, 20)),
        "hot_paths" => crate::hot_paths::execute(
            report,
            args.get("category")
                .and_then(Value::as_str)
                .unwrap_or("auto"),
            limit(&args, 24),
        ),
        "mod_sources" => mod_sources(report, limit(&args, 24)),
        "time_windows" => crate::windows::time_windows(report, limit(&args, 50)),
        "worst_windows" => crate::windows::worst_windows(report, limit(&args, 12)),
        "entities" => entities(report),
        "entity_chunks" => entity_chunks(report, limit(&args, 24)),
        "heap" => {
            json!({"topHeap":report.summary.top_heap.iter().take(limit(&args,24)).collect::<Vec<_>>() })
        }
        "memory_gc" => crate::memory_gc::summarize_memory_gc(report),
        "evidence_links" => evidence_links(report, limit(&args, 16)),
        "diagnostic_hypotheses" => diagnostic_hypotheses(report),
        "evidence_gaps" => evidence_gaps(report),
        "raw_field" => raw_field(
            report,
            args.get("path").and_then(Value::as_str).unwrap_or(""),
            args.get("maxItems").and_then(Value::as_u64).unwrap_or(80) as usize,
        ),
        _ => return Err(SparkError::UnknownTool(tool.into())),
    })
}

fn inventory(r: &Report) -> Value {
    json!({"kind":r.kind,"source":r.source,"title":r.summary.title,"availableTools":report_tool_descriptions(),"availableData":{"overview":true,"environment":crate::environment::has_environment(r),"hotspots":!r.summary.top_hotspots.is_empty(),"hotspotGroups":!r.summary.top_hotspots.is_empty(),"modSources":obj_at(&r.raw,"classSources").is_some()||obj_at(&r.raw,"metadata.sources").is_some(),"heap":!r.summary.top_heap.is_empty(),"memoryGc":path(&r.raw,"metadata.platformStatistics.memory").is_some()||path(&r.raw,"metadata.systemStatistics.gc").is_some(),"entities":!r.summary.top_entities.is_empty()||r.summary.entity_count.is_some(),"entityChunks":arr_at(&r.raw,"metadata.platformStatistics.world.worlds").is_some(),"timeWindows":obj_at(&r.raw,"timeWindowStatistics").is_some_and(|m|!m.is_empty()),"worstWindows":obj_at(&r.raw,"timeWindowStatistics").is_some_and(|m|!m.is_empty()),"diagnosticHypotheses":true,"evidenceGaps":true,"rawField":r.kind.as_str()!="text"}})
}
fn overview(r: &Report) -> Value {
    let mut m = Map::new();
    m.insert("tps1m".into(), json!(r.summary.tps1m));
    m.insert("tps5m".into(), json!(r.summary.tps5m));
    m.insert("tps15m".into(), json!(r.summary.tps15m));
    m.insert("msptMedian".into(), json!(r.summary.mspt_median));
    m.insert("msptP95".into(), json!(r.summary.mspt_p95));
    m.insert("msptMax".into(), json!(r.summary.mspt_max));
    m.insert("processCpu1m".into(), json!(r.summary.process_cpu1m));
    m.insert("systemCpu1m".into(), json!(r.summary.system_cpu1m));
    m.insert(
        "heapUsed".into(),
        r.summary
            .heap_used_bytes
            .map(format_bytes)
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    m.insert(
        "heapMax".into(),
        r.summary
            .heap_max_bytes
            .map(format_bytes)
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    m.insert("playerCount".into(), json!(r.summary.player_count));
    m.insert("entityCount".into(), json!(r.summary.entity_count));
    json!({"source":r.source,"kind":r.kind,"title":r.summary.title,"platform":r.summary.platform,"generatedAt":r.summary.generated_at,"durationSeconds":r.summary.duration_seconds,"metrics":compact(m),"findings":r.summary.findings,"gc":r.summary.gc})
}

#[derive(Default)]
struct Group {
    samples: f64,
    max_percent: f64,
    frames: Vec<Value>,
}
fn add_group(map: &mut HashMap<String, Group>, key: String, h: &crate::StackHotspot) {
    let g = map.entry(key).or_default();
    g.samples += h.samples;
    g.max_percent = g.max_percent.max(h.percent);
    if g.frames.len() < 8 {
        g.frames.push(json!(h));
    }
}
fn groups_json(
    mut groups: HashMap<String, Group>,
    key: &str,
    limit: usize,
    category_order: bool,
) -> Vec<Value> {
    let mut values: Vec<_> = groups
        .drain()
        .map(|(name, group)| {
            json!({
                key:name,
                "samples":group.samples,
                "maxPercent":group.max_percent,
                "frames":group.frames.into_iter().take(6).collect::<Vec<_>>()
            })
        })
        .collect();
    values.sort_by(|left, right| {
        if category_order {
            let left_other = left[key].as_str() == Some("other");
            let right_other = right[key].as_str() == Some("other");
            if left_other != right_other {
                return left_other.cmp(&right_other);
            }
        }
        f64_at(right, "samples")
            .unwrap_or_default()
            .total_cmp(&f64_at(left, "samples").unwrap_or_default())
    });
    values.truncate(limit);
    values
}
fn package_key(hotspot: &crate::StackHotspot) -> String {
    let class_name = hotspot.class_name.as_deref().unwrap_or_else(|| {
        let no_line = hotspot
            .label
            .rsplit_once(':')
            .filter(|(_, line)| line.parse::<i64>().is_ok())
            .map_or(hotspot.label.as_str(), |(label, _)| label);
        no_line
            .rsplit_once('.')
            .map_or(no_line, |(class_name, _)| class_name)
    });
    let parts = class_name.split('.').collect::<Vec<_>>();
    if parts.len() <= 2 {
        return class_name.to_owned();
    }
    let length = if matches!(parts[0], "net" | "com" | "org" | "me" | "io") {
        3
    } else {
        2
    };
    parts.into_iter().take(length).collect::<Vec<_>>().join(".")
}
fn hotspot_groups(r: &Report, limit: usize) -> Value {
    let mut cats = HashMap::new();
    let mut packages = HashMap::new();
    let mut threads = HashMap::new();
    for h in &r.summary.top_hotspots {
        add_group(&mut cats, classify_hotspot(&h.label, &h.thread), h);
        add_group(&mut packages, package_key(h), h);
        add_group(
            &mut threads,
            if h.thread.is_empty() {
                "unknown".into()
            } else {
                h.thread.clone()
            },
            h,
        );
    }
    json!({"byCategory":groups_json(cats,"category",limit,true),"byPackage":groups_json(packages,"package",limit,false),"byThread":groups_json(threads,"thread",limit,false),"notes":["samples/percent 来自采样栈帧，并非 exclusive CPU time。","分类是线索，需要结合 raw hotspots 和 worst_windows 确认。"]})
}

fn entities(r: &Report) -> Value {
    let world = path(&r.raw, "metadata.platformStatistics.world");
    let mut top_entity_counts = world
        .and_then(|value| obj_at(value, "entityCounts"))
        .map(|counts| {
            counts
                .iter()
                .filter_map(|(name, value)| {
                    value
                        .as_f64()
                        .map(|value| json!({"name":name,"value":value}))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    top_entity_counts.sort_by(|left, right| {
        right["value"]
            .as_f64()
            .unwrap_or_default()
            .total_cmp(&left["value"].as_f64().unwrap_or_default())
    });
    top_entity_counts.truncate(20);
    let worlds = world
        .and_then(|value| arr_at(value, "worlds"))
        .map(|worlds| {
            worlds
                .iter()
                .take(12)
                .map(|world| {
                    format!(
                        "{}: {} entities",
                        str_at(world, "name").unwrap_or("unknown"),
                        i64_at(world, "totalEntities").unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let world_raw_summary = world.map(|world| {
        json!({
            "totalEntities":i64_at(world,"totalEntities"),
            "topEntityCounts":top_entity_counts,
            "worlds":worlds,
        })
    });
    json!({"totalEntities":r.summary.entity_count,"worlds":r.summary.worlds,"topEntities":r.summary.top_entities,"worldRawSummary":world_raw_summary})
}
fn evidence_gaps(r: &Report) -> Value {
    let mut available_evidence = Vec::new();
    let mut weak_evidence = Vec::new();
    let mut missing_evidence = Vec::new();

    if r.summary.top_hotspots.is_empty() {
        missing_evidence.push("CPU hotspot tree");
    } else {
        available_evidence.push("sampled CPU hotspot tree");
    }
    if obj_at(&r.raw, "timeWindowStatistics").is_some_and(|value| !value.is_empty()) {
        available_evidence.push("time window statistics");
    } else {
        missing_evidence.push("time window statistics");
    }
    if arr_at(&r.raw, "metadata.platformStatistics.world.worlds")
        .is_some_and(|value| !value.is_empty())
    {
        available_evidence.push("world/entity/chunk statistics");
    } else {
        missing_evidence.push("world/entity/chunk statistics");
    }
    let has_source_maps = ["classSources", "methodSources", "lineSources"]
        .iter()
        .any(|path| obj_at(&r.raw, path).is_some_and(|value| !value.is_empty()));
    if has_source_maps {
        available_evidence.push("class/method/line source map");
    } else {
        weak_evidence.push(
            "mod attribution is weak because class_sources/method_sources/line_sources are absent",
        );
    }
    if r.summary.gc.is_empty() {
        missing_evidence.push("GC pause log");
    } else {
        available_evidence.push("GC aggregate statistics");
        weak_evidence.push(
            "GC aggregate statistics are not timestamped; exact spike correlation requires GC logs",
        );
    }
    if r.kind.as_str() == "sampler" {
        weak_evidence.push(
            "ordinary sampler cannot always isolate one tick spike; --only-ticks-over reports are stronger for spikes",
        );
    }

    json!({
        "canProve":[
            "Whether TPS/MSPT were degraded during the captured interval.",
            "Which stack-frame categories dominated sampled server-thread time.",
            "Which entity types/chunks were numerically heavy if world stats are present.",
        ],
        "cannotProveAlone":[
            "The exact entity instance, block entity, or chunk coordinate that caused one spike unless tool data aligns clearly.",
            "GC stop-the-world pauses without GC logs or health GC data.",
            "A mod source when class/method source maps are absent or obfuscated.",
        ],
        "availableEvidence":available_evidence,
        "weakEvidence":weak_evidence,
        "missingEvidence":missing_evidence,
        "recommendedNextCaptures":[
            "/spark profiler --only-ticks-over 50 --timeout 120",
            "/spark healthreport --memory --network --upload",
            "GC log around the spike if heap pressure is suspected",
        ],
    })
}
fn trim(v: &Value, max: usize) -> Value {
    match v {
        Value::Array(a) => Value::Array(a.iter().take(max).map(|x| trim(x, max)).collect()),
        Value::Object(m) => Value::Object(
            m.iter()
                .take(max)
                .map(|(k, v)| (k.clone(), trim(v, max)))
                .collect(),
        ),
        _ => v.clone(),
    }
}
fn raw_field(r: &Report, p: &str, max: usize) -> Value {
    if p.is_empty() {
        return json!({"error":"path is required"});
    }
    path(&r.raw, p).map(|v| trim(v, max)).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parse_text_report, ReportKind, ReportSummary};
    #[test]
    fn exposes_every_planned_tool() {
        let names = report_tool_descriptions()
            .into_iter()
            .map(|t| t.name)
            .collect::<Vec<_>>();
        for n in [
            "overview",
            "environment",
            "hotspots",
            "hotspot_groups",
            "hot_paths",
            "mod_sources",
            "time_windows",
            "worst_windows",
            "entities",
            "entity_chunks",
            "heap",
            "memory_gc",
            "evidence_links",
            "diagnostic_hypotheses",
            "evidence_gaps",
            "raw_field",
        ] {
            assert!(names.contains(&n));
        }
    }
    #[test]
    fn every_tool_executes_on_sparse_report() {
        let r = Report {
            kind: ReportKind::Sampler,
            source: "x".into(),
            raw: json!({}),
            summary: ReportSummary {
                title: "x".into(),
                ..Default::default()
            },
        };
        for t in report_tool_descriptions() {
            let args = if t.name == "raw_field" {
                json!({"path":"metadata"})
            } else {
                json!({})
            };
            assert!(execute_tool(&r, t.name, args).is_ok(), "{}", t.name);
        }
    }
    #[test]
    fn inventory_and_text_work() {
        let r = parse_text_report("hello", "stdin").unwrap();
        assert_eq!(
            execute_tool(&r, "report_inventory", json!({})).unwrap()["kind"],
            "text"
        );
    }

    #[test]
    fn evidence_gaps_preserves_the_legacy_contract() {
        let r = Report {
            kind: ReportKind::Sampler,
            source: "x".into(),
            raw: json!({}),
            summary: ReportSummary {
                title: "x".into(),
                ..Default::default()
            },
        };
        let gaps = execute_tool(&r, "evidence_gaps", json!({})).unwrap();
        assert!(gaps["availableEvidence"].is_array());
        assert!(gaps["weakEvidence"].is_array());
        assert!(gaps["missingEvidence"].is_array());
        assert!(gaps["recommendedNextCaptures"].is_array());
        assert!(gaps["cannotProveAlone"].is_array());
    }
}
