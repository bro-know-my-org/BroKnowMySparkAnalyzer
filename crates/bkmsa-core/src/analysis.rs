use crate::{Finding, HeapHotspot, NamedValue, ReportKind, ReportSummary, Severity, StackHotspot};
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

pub(crate) fn path<'a>(value: &'a Value, dotted: &str) -> Option<&'a Value> {
    dotted.split('.').try_fold(value, |v, key| v.get(key))
}
pub(crate) fn f64_at(value: &Value, dotted: &str) -> Option<f64> {
    let v = path(value, dotted)?;
    v.as_f64()
        .or_else(|| v.as_str()?.parse().ok())
        .filter(|value| value.is_finite())
}
pub(crate) fn i64_at(value: &Value, dotted: &str) -> Option<i64> {
    let v = path(value, dotted)?;
    v.as_i64()
        .or_else(|| v.as_u64().and_then(|n| i64::try_from(n).ok()))
        .or_else(|| v.as_str()?.parse().ok())
}
pub(crate) fn str_at<'a>(value: &'a Value, dotted: &str) -> Option<&'a str> {
    path(value, dotted)?.as_str()
}
pub(crate) fn obj_at<'a>(value: &'a Value, dotted: &str) -> Option<&'a Map<String, Value>> {
    path(value, dotted)?.as_object()
}
pub(crate) fn arr_at<'a>(value: &'a Value, dotted: &str) -> Option<&'a Vec<Value>> {
    path(value, dotted)?.as_array()
}
pub(crate) fn format_number(value: f64) -> String {
    if value.fract().abs() < 0.005 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    }
}
pub(crate) fn format_bytes(value: u64) -> String {
    let mut n = value as f64;
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut unit = 0;
    while n.abs() >= 1024.0 && unit < units.len() - 1 {
        n /= 1024.0;
        unit += 1;
    }
    format!(
        "{} {}",
        if unit == 0 {
            format!("{n:.0}")
        } else {
            format!("{n:.2}")
        },
        units[unit]
    )
}
pub(crate) fn compact(mut map: Map<String, Value>) -> Value {
    map.retain(|_, v| !v.is_null());
    Value::Object(map)
}
fn timestamp(v: Option<i64>) -> Option<String> {
    DateTime::<Utc>::from_timestamp_millis(v?)
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

pub(crate) fn summarize(kind: ReportKind, raw: &Value, source: &str) -> ReportSummary {
    if kind == ReportKind::Text {
        let text = str_at(raw, "text").unwrap_or_default();
        let mut findings = vec![];
        let lower = text.to_lowercase();
        if lower.contains("can't keep up") || lower.contains("overloaded") {
            findings.push(Finding{severity:Severity::Warning,title:"日志出现 tick 落后提示".into(),detail:"文本里包含 can't keep up/overloaded 类提示，需要结合 spark profile 找主线程热点。".into()});
        }
        findings.push(Finding {
            severity: Severity::Info,
            title: "文本输入已载入".into(),
            detail: "文本输入只能支持弱证据分析；建议拖入 .sparkprofile 获得可追溯结论。".into(),
        });
        return ReportSummary {
            title: format!("文本报告 - {source}"),
            findings,
            ..Default::default()
        };
    }
    let mut windows: Vec<(&String, &Value)> = obj_at(raw, "timeWindowStatistics")
        .map(|m| m.iter().collect())
        .unwrap_or_default();
    windows.sort_by(|(left, _), (right, _)| {
        left.parse::<i64>()
            .ok()
            .cmp(&right.parse::<i64>().ok())
            .then_with(|| left.cmp(right))
    });
    let windows: Vec<&Value> = windows.into_iter().map(|(_, value)| value).collect();
    let latest = windows.last().copied().unwrap_or(&Value::Null);
    let start = i64_at(raw, "metadata.startTime");
    let end = i64_at(raw, "metadata.endTime");
    let pname = str_at(raw, "metadata.platformMetadata.name").unwrap_or("spark");
    let pver = str_at(raw, "metadata.platformMetadata.version").unwrap_or_default();
    let mc = str_at(raw, "metadata.platformMetadata.minecraftVersion").unwrap_or_default();
    let platform = [pname, pver, mc]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let kname = match kind {
        ReportKind::Sampler => "性能采样",
        ReportKind::Health => "健康报告",
        ReportKind::Heap => "堆摘要",
        ReportKind::Text => unreachable!(),
    };
    let heap_used = i64_at(raw, "metadata.platformStatistics.memory.heap.used")
        .and_then(|value| u64::try_from(value).ok());
    let heap_max = i64_at(raw, "metadata.platformStatistics.memory.heap.max")
        .and_then(|value| u64::try_from(value).ok())
        .filter(|value| *value > 0);
    let world = path(raw, "metadata.platformStatistics.world").unwrap_or(&Value::Null);
    let mut top_entities: Vec<NamedValue> = obj_at(world, "entityCounts")
        .map(|m| {
            m.iter()
                .filter_map(|(n, v)| {
                    Some(NamedValue {
                        name: n.chars().take(256).collect(),
                        value: v
                            .as_u64()
                            .or_else(|| v.as_i64().and_then(|value| u64::try_from(value).ok()))?,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    top_entities.sort_by_key(|value| std::cmp::Reverse(value.value));
    top_entities.truncate(12);
    let worlds = arr_at(world, "worlds")
        .into_iter()
        .flatten()
        .filter_map(|w| {
            Some(format!(
                "{}: {} entities",
                str_at(w, "name")?,
                i64_at(w, "totalEntities").unwrap_or_default()
            ))
        })
        .collect();
    let mut top_heap: Vec<HeapHotspot> = arr_at(raw, "entries")
        .into_iter()
        .flatten()
        .filter_map(|e| {
            Some(HeapHotspot {
                type_name: str_at(e, "type")
                    .unwrap_or("unknown")
                    .chars()
                    .take(512)
                    .collect(),
                instances: i64_at(e, "instances").and_then(|value| u64::try_from(value).ok())?,
                bytes: i64_at(e, "size").and_then(|value| u64::try_from(value).ok())?,
            })
        })
        .collect();
    top_heap.sort_by_key(|h| std::cmp::Reverse(h.bytes));
    top_heap.truncate(40);
    let top_hotspots = if kind == ReportKind::Sampler {
        collect_hotspots(raw, 40)
    } else {
        vec![]
    };
    let gc = summarize_gc(raw);
    let mut s = ReportSummary {
        title: format!("{kname} - {pname}"),
        platform: (!platform.is_empty()).then_some(platform),
        generated_at: timestamp(i64_at(raw, "metadata.generatedTime").or(end).or(start)),
        duration_seconds: match (start, end) {
            (Some(a), Some(b)) => b
                .checked_sub(a)
                .filter(|duration| *duration >= 0)
                .map(|duration| (duration as f64 / 1000.0).round()),
            _ => f64_at(latest, "duration"),
        },
        tps1m: f64_at(raw, "metadata.platformStatistics.tps.last1m")
            .or_else(|| f64_at(latest, "tps")),
        tps5m: f64_at(raw, "metadata.platformStatistics.tps.last5m"),
        tps15m: f64_at(raw, "metadata.platformStatistics.tps.last15m"),
        mspt_median: f64_at(raw, "metadata.platformStatistics.mspt.last1m.median")
            .or_else(|| f64_at(latest, "msptMedian")),
        mspt_p95: f64_at(raw, "metadata.platformStatistics.mspt.last1m.percentile95"),
        mspt_max: f64_at(raw, "metadata.platformStatistics.mspt.last1m.max")
            .or_else(|| f64_at(latest, "msptMax")),
        process_cpu1m: f64_at(raw, "metadata.systemStatistics.cpu.processUsage.last1m")
            .or_else(|| f64_at(latest, "cpuProcess")),
        system_cpu1m: f64_at(raw, "metadata.systemStatistics.cpu.systemUsage.last1m")
            .or_else(|| f64_at(latest, "cpuSystem")),
        heap_used_bytes: heap_used,
        heap_max_bytes: heap_max,
        entity_count: i64_at(world, "totalEntities")
            .or_else(|| i64_at(latest, "entities"))
            .and_then(|value| u64::try_from(value).ok()),
        player_count: i64_at(raw, "metadata.platformStatistics.playerCount")
            .or_else(|| i64_at(latest, "players"))
            .and_then(|value| u64::try_from(value).ok()),
        gc,
        worlds,
        top_entities,
        top_heap,
        top_hotspots,
        findings: vec![],
    };
    s.findings = build_findings(kind, &s, raw);
    s
}
fn summarize_gc(raw: &Value) -> Vec<String> {
    [
        ("system", obj_at(raw, "metadata.systemStatistics.gc")),
        ("platform", obj_at(raw, "metadata.platformStatistics.gc")),
    ]
    .into_iter()
    .flat_map(|(source, collectors)| {
        collectors.into_iter().flat_map(move |collectors| {
            collectors.iter().map(move |(n, g)| {
                format!(
                    "{source}/{n}: {} 次, 平均 {}ms, 频率 {}ms",
                    i64_at(g, "total").unwrap_or_default(),
                    f64_at(g, "avgTime").map_or_else(|| "未知".into(), format_number),
                    f64_at(g, "avgFrequency").map_or_else(|| "未知".into(), format_number)
                )
            })
        })
    })
    .collect()
}
fn build_findings(_kind: ReportKind, s: &ReportSummary, raw: &Value) -> Vec<Finding> {
    let mut o = vec![];
    let target = f64_at(raw, "metadata.platformStatistics.mspt.gameMaxIdealMspt")
        .filter(|value| (1.0..=10_000.0).contains(value))
        .unwrap_or(50.0);
    if s.tps1m.is_some_and(|v| v < 18.0) {
        o.push(Finding {
            severity: Severity::Critical,
            title: "TPS 明显低于目标".into(),
            detail: format!(
                "1m TPS 为 {}，优先看主线程热点与 tick 窗口。",
                format_number(s.tps1m.unwrap())
            ),
        });
    }
    if s.mspt_p95.is_some_and(|v| v > target) {
        o.push(Finding {
            severity: Severity::Warning,
            title: "MSPT P95 超过理想 tick 时长".into(),
            detail: format!(
                "P95 MSPT 为 {}ms，目标约 {}ms。",
                format_number(s.mspt_p95.unwrap()),
                target
            ),
        });
    }
    if s.mspt_max.is_some_and(|v| v > target * 2.0) {
        o.push(Finding {
            severity: Severity::Warning,
            title: "存在长 tick 尖峰".into(),
            detail: format!(
                "最大 MSPT 为 {}ms，需要看热点和时间窗口。",
                format_number(s.mspt_max.unwrap())
            ),
        });
    }
    if s.process_cpu1m.is_some_and(|v| v > 85.0) {
        o.push(Finding {
            severity: Severity::Warning,
            title: "Java 进程 CPU 压力高".into(),
            detail: format!(
                "进程 CPU 1m 为 {}%。",
                format_number(s.process_cpu1m.unwrap())
            ),
        });
    }
    if matches!((s.heap_used_bytes,s.heap_max_bytes),(Some(u),Some(m))if m>0&&u as f64/m as f64>0.85)
    {
        o.push(Finding {
            severity: Severity::Warning,
            title: "堆内存接近上限".into(),
            detail: "堆使用率超过 85%。".into(),
        });
    }
    o.extend(crate::memory_gc::finding_signals(raw));
    if let Some(top) = s.top_hotspots.first() {
        o.push(Finding {
            severity: if top.percent > 25.0 {
                Severity::Warning
            } else {
                Severity::Info
            },
            title: "已识别 CPU 热点".into(),
            detail: format!(
                "最高热点 {}，约占 {}%，线程 {}。",
                top.label,
                format_number(top.percent),
                top.thread
            ),
        });
    }
    if o.is_empty() {
        o.push(Finding {
            severity: Severity::Info,
            title: "未触发明显阈值告警".into(),
            detail: "本地规则没有发现直接越线信号，建议由 AI 调工具做模式判断。".into(),
        });
    }
    o
}

pub(crate) fn collect_hotspots(raw: &Value, limit: usize) -> Vec<StackHotspot> {
    let mut all = vec![];
    for thread in arr_at(raw, "threads").into_iter().flatten() {
        let Some(nodes) = arr_at(thread, "children") else {
            continue;
        };
        let mut refs: Vec<usize> = arr_at(thread, "childrenRefs")
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_u64().and_then(|n| usize::try_from(n).ok()))
            .collect();
        if refs.is_empty() {
            let mut used = HashSet::new();
            for n in nodes {
                for r in arr_at(n, "childrenRefs")
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_u64)
                {
                    if let Ok(index) = usize::try_from(r) {
                        used.insert(index);
                    }
                }
            }
            refs = (0..nodes.len()).filter(|i| !used.contains(i)).collect();
        }
        let thread_total = arr_at(thread, "times")
            .into_iter()
            .flatten()
            .filter_map(Value::as_f64)
            .sum::<f64>();
        let roots_total = refs
            .iter()
            .filter_map(|index| nodes.get(*index))
            .map(sum_times)
            .sum::<f64>();
        let total = if thread_total > 0.0 {
            thread_total
        } else {
            roots_total
        };
        let mut visited = HashSet::new();
        for r in refs {
            visit_node(
                nodes,
                r,
                str_at(thread, "name").unwrap_or("unknown"),
                total,
                0,
                &mut all,
                &mut visited,
            );
        }
    }
    let mut unique: HashMap<String, StackHotspot> = HashMap::new();
    for h in all {
        if is_generic(&h.label) {
            continue;
        }
        let key = format!("{}|{}", h.thread, h.label);
        if unique
            .get(&key)
            .map(|x| h.samples > x.samples)
            .unwrap_or(true)
        {
            unique.insert(key, h);
        }
    }
    let mut o: Vec<_> = unique.into_values().collect();
    o.sort_by(|a, b| b.samples.total_cmp(&a.samples));
    o.truncate(limit);
    o
}
fn visit_node(
    nodes: &[Value],
    index: usize,
    thread: &str,
    total: f64,
    depth: usize,
    out: &mut Vec<StackHotspot>,
    visited: &mut HashSet<usize>,
) {
    let Some(n) = nodes.get(index) else { return };
    if depth > 64 || out.len() >= 100_000 || !visited.insert(index) {
        return;
    }
    let class = str_at(n, "className").unwrap_or_default();
    let method = str_at(n, "methodName").unwrap_or_default();
    let line = i64_at(n, "lineNumber").filter(|line| *line > 0);
    let label = format!(
        "{}{}{}{}",
        class,
        if method.is_empty() { "" } else { "." },
        method,
        line.map(|line| format!(":{line}")).unwrap_or_default(),
    );
    let samples = sum_times(n);
    out.push(StackHotspot {
        label,
        samples,
        percent: if total > 0.0 {
            (samples / total * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        },
        thread: thread.into(),
        source: (!class.is_empty()).then(|| class.into()),
        class_name: (!class.is_empty()).then(|| class.into()),
        method_name: (!method.is_empty()).then(|| method.into()),
        method_desc: str_at(n, "methodDesc").map(Into::into),
        line_number: line,
    });
    for r in arr_at(n, "childrenRefs")
        .into_iter()
        .flatten()
        .filter_map(Value::as_u64)
        .filter_map(|value| usize::try_from(value).ok())
    {
        visit_node(nodes, r, thread, total, depth + 1, out, visited)
    }
}
fn sum_times(v: &Value) -> f64 {
    arr_at(v, "times")
        .into_iter()
        .flatten()
        .filter_map(Value::as_f64)
        .sum()
}
fn is_generic(s: &str) -> bool {
    if is_minecraft_loop_frame(s) {
        return true;
    }
    [
        "java.lang.Thread.",
        "net.minecraft.server.MinecraftServer.runServer",
        "net.minecraft.server.MinecraftServer.lambda$spin",
        "net.minecraft.server.MinecraftServer$$Lambda",
        "net.minecraft.server.MinecraftServer.waitUntilNextTick",
        "net.minecraft.server.MinecraftServer.waitForTasks",
        "BlockableEventLoop.managedBlock",
        "MinecraftServer.managedBlock",
        "modernfix$managedBlock",
        "modernfix$waitLongerForTasks",
        "mixinextras$bridge$managedBlock",
        "LockSupport.park",
        "LockSupport.parkNanos",
        "FileWatcher$WatcherThread.run",
        "io.netty.util.internal.ThreadExecutorMap$2.run",
        "io.netty.util.concurrent.SingleThreadEventExecutor$4.run",
        "io.netty.channel.nio.NioEventLoop.run",
        "io.netty.util.concurrent.SingleThreadEventExecutor.runAllTasks",
        "io.netty.util.concurrent.AbstractEventExecutor.safeExecute",
        "io.netty.util.concurrent.AbstractEventExecutor.runTask",
        "$$Lambda.",
        "mixinextras$bridge",
        "libjvm.",
        "libsystem_pthread.",
        "libsystem_kernel.",
        "__psynch_cvwait",
        "jdk.internal.",
        "sun.nio.",
    ]
    .iter()
    .any(|p| s.contains(p))
}

fn is_minecraft_loop_frame(label: &str) -> bool {
    let no_line = label.rsplit_once(':').map_or(label, |(value, _)| value);
    no_line.starts_with("net.minecraft.server.MinecraftServer$$Lambda")
        || no_line.starts_with("net.minecraft.server.level.ServerLevel$$Lambda")
        || [
            "net.minecraft.server.MinecraftServer.m_206580_",
            "net.minecraft.server.MinecraftServer.m_130011_",
            "net.minecraft.server.MinecraftServer.m_5705_",
            "net.minecraft.server.MinecraftServer.m_5703_",
            "net.minecraft.server.dedicated.DedicatedServer.m_5703_",
            "net.minecraft.client.server.IntegratedServer.m_5705_",
            "net.minecraft.server.level.ServerLevel.m_8793_",
            "net.minecraft.world.level.Level.m_46653_",
        ]
        .iter()
        .any(|prefix| no_line.starts_with(prefix))
}

pub(crate) fn is_generic_frame(label: &str) -> bool {
    is_generic(label)
}

pub(crate) fn classify_frame(label: &str) -> &'static str {
    let l = label.to_lowercase();
    if l.contains("blockentity")
        || l.contains("tileentity")
        || l.contains("tickingblockentity")
        || l.contains("catchtickingblockentity")
        || l.contains("redirecttick")
        || l.contains("level.m_46463_")
    {
        "block_entity"
    } else if l.contains("entityticklist")
        || l.contains("guardentitytick")
        || l.contains("catchtickingentities")
        || l.contains("safelytickentities")
        || l.contains("serverlevel.m_184063_")
        || l.contains(".ticknonpassenger")
        || l.contains("onnonpassenger")
        || l.contains(".tickpassenger")
    {
        "entity_tick"
    } else if l.contains("serverchunkcache")
        || l.contains("chunk")
        || l.contains("worldgen")
        || l.contains("generation")
    {
        "chunk_task"
    } else if l.contains("pathnavigation")
        || l.contains("goal")
        || l.contains("brain")
        || l.contains("sensor")
    {
        "entity_ai_pathfinding"
    } else if l.contains("commandfunction")
        || l.contains("commandentry")
        || l.contains(".commands.")
    {
        "commands"
    } else if l.contains("level.tick") || l.contains("serverlevel") || l.contains("tickchildren") {
        "world_tick"
    } else if is_io_frame(&l) {
        "io"
    } else if l.contains("gc") || l.contains("g1") || l.contains("shenandoah") || l.contains("zgc")
    {
        "gc"
    } else {
        "other"
    }
}

pub(crate) fn is_io_frame(lower_label: &str) -> bool {
    lower_label.contains("filesystem")
        || lower_label.contains("java.io.")
        || lower_label.contains("java.nio.")
        || lower_label.contains("sun.nio.")
        || lower_label.contains("fileinputstream")
        || lower_label.contains("fileoutputstream")
        || lower_label.contains("files.new")
        || lower_label.contains("filechannel")
}

pub(crate) fn is_server_thread_name(thread: &str) -> bool {
    thread.eq_ignore_ascii_case("server thread")
}

pub(crate) fn is_server_thread_category(category: &str) -> bool {
    matches!(
        category,
        "entity_tick"
            | "entity_ai_pathfinding"
            | "chunk_task"
            | "block_entity"
            | "commands"
            | "world_tick"
    )
}

pub(crate) fn classify_hotspot(label: &str, thread: &str) -> String {
    let category = classify_frame(label);
    if is_server_thread_category(category) && !is_server_thread_name(thread) {
        return match category {
            "block_entity" => "background_block_entity_sync".into(),
            "chunk_task" => "background_chunk_task".into(),
            _ => format!("background_{category}"),
        };
    }
    category.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn summary_findings_include_memory_and_gc_signals() {
        let raw = json!({"metadata":{"platformStatistics":{"memory":{"heap":{"used":90,"committed":100,"max":100}},"gc":{"Old GC":{"total":2,"avgTime":250.0,"avgFrequency":1500.0}}}}});
        let summary = summarize(ReportKind::Health, &raw, "fixture");
        assert!(summary
            .findings
            .iter()
            .any(|finding| finding.title.contains("长暂停")));
        assert!(summary
            .findings
            .iter()
            .any(|finding| finding.title.contains("堆使用率")));
        assert!(summary.gc.iter().any(|line| line.contains("1500ms")));
    }

    #[test]
    fn hotspot_percent_uses_full_thread_total_and_keeps_line_number() {
        let raw = json!({"threads":[{"name":"Server thread","times":[200.0],"childrenRefs":[0],"children":[
            {"className":"example.Root","methodName":"work","lineNumber":1,"times":[100.0],"childrenRefs":[1]},
            {"className":"example.Worker","methodName":"tick","lineNumber":42,"times":[50.0],"childrenRefs":[]}
        ]}]});
        let hotspots = collect_hotspots(&raw, 40);
        let worker = hotspots
            .iter()
            .find(|hotspot| hotspot.label == "example.Worker.tick:42")
            .unwrap();
        assert_eq!(worker.percent, 25.0);
        assert_eq!(worker.line_number, Some(42));
    }

    #[test]
    fn precise_categories_preserve_server_thread_boundary() {
        let label = "net.minecraft.world.level.block.entity.TickingBlockEntity.tick";
        assert_eq!(classify_hotspot(label, "Server thread"), "block_entity");
        assert_eq!(
            classify_hotspot(label, "Worker-1"),
            "background_block_entity_sync"
        );
        assert_eq!(
            classify_frame("net.minecraft.server.level.ServerChunkCache.tick"),
            "chunk_task"
        );
        assert_eq!(classify_frame("sun.nio.ch.FileDispatcherImpl.read0"), "io");
        assert_eq!(classify_frame("example.Profiler.tick"), "other");
    }
}
