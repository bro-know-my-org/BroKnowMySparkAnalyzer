use crate::analysis::{arr_at, f64_at, format_number, i64_at, obj_at, path, str_at};
use crate::{Finding, Report, Severity};
use serde_json::{json, Value};
use std::collections::HashMap;

fn ratio(used: f64, total: f64) -> Option<f64> {
    if !used.is_finite() || !total.is_finite() || used < 0.0 || total <= 0.0 || used > total {
        None
    } else {
        Some(used / total)
    }
}

fn format_bytes(value: i64) -> String {
    if value < 0 {
        return "-".into();
    }
    let units = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut scaled = value as f64;
    let mut unit = 0;
    while scaled >= 1024.0 && unit < units.len() - 1 {
        scaled /= 1024.0;
        unit += 1;
    }
    format!("{} {}", format_number(scaled), units[unit])
}

fn usage_summary(usage: Option<&Value>) -> Value {
    let used = usage.and_then(|v| i64_at(v, "used"));
    let committed = usage.and_then(|v| i64_at(v, "committed"));
    let max = usage.and_then(|v| i64_at(v, "max"));
    let effective_max = max.filter(|value| *value > 0);
    json!({
        "available":usage.is_some() && used.is_some(),
        "used":used,"committed":committed,"max":max,"effectiveMax":effective_max,
        "usedFormatted":used.map(format_bytes),"committedFormatted":committed.map(format_bytes),
        "maxFormatted":effective_max.map(format_bytes).unwrap_or_else(|| "-".to_owned()),
        "usedCommittedRatio":used.zip(committed).and_then(|(used,total)|ratio(used as f64,total as f64)),
        "usedMaxRatio":used.zip(effective_max).and_then(|(used,total)|ratio(used as f64,total as f64)),
    })
}

fn signal(category: &str, severity: &str, title: String, detail: String) -> Value {
    json!({"category":category,"severity":severity,"title":title,"detail":detail})
}

fn usage_ratio(summary: &Value) -> Option<(f64, &'static str, &str)> {
    f64_at(summary, "usedMaxRatio").map(|ratio| {
        (
            ratio,
            "max",
            summary["maxFormatted"].as_str().unwrap_or("-"),
        )
    })
}

fn heap_signals(heap: Option<&Value>) -> Vec<Value> {
    let summary = usage_summary(heap);
    let Some(used_max) = f64_at(&summary, "usedMaxRatio") else {
        return Vec::new();
    };
    let detail = || {
        format!(
            "heap used/max {}% ({} / {})",
            format_number(used_max * 100.0),
            summary["usedFormatted"].as_str().unwrap_or("-"),
            summary["maxFormatted"].as_str().unwrap_or("-")
        )
    };
    if used_max >= 0.9 {
        vec![signal(
            "memory",
            "critical",
            "堆使用率接近上限".into(),
            detail(),
        )]
    } else if used_max >= 0.75 {
        vec![signal("memory", "warning", "堆使用率偏高".into(), detail())]
    } else {
        vec![]
    }
}

fn non_heap_signals(non_heap: Option<&Value>) -> Vec<Value> {
    let summary = usage_summary(non_heap);
    let Some((ratio, denominator, total)) = usage_ratio(&summary) else {
        return Vec::new();
    };
    if ratio >= 0.9 {
        vec![signal(
            "memory",
            if ratio >= 0.97 { "critical" } else { "warning" },
            "非堆内存接近上限".into(),
            format!(
                "non-heap used/{denominator} {}% ({} / {})",
                format_number(ratio * 100.0),
                summary["usedFormatted"].as_str().unwrap_or("-"),
                total
            ),
        )]
    } else {
        Vec::new()
    }
}

fn pool_signals(name: &str, usage: Option<&Value>, collection: Option<&Value>) -> Vec<Value> {
    let current = usage_summary(usage);
    let collected = usage_summary(collection);
    let lower = name.to_lowercase();
    let old = lower.contains("old") || lower.contains("tenured");
    let mut signals = vec![];
    let current_ratio = usage_ratio(&current);
    if old && current_ratio.is_some_and(|(ratio, _, _)| ratio >= 0.8) {
        let (ratio, denominator, total) = current_ratio.expect("checked above");
        signals.push(signal(
            "memory",
            if ratio >= 0.92 { "critical" } else { "warning" },
            format!("{name} 使用率偏高"),
            format!(
                "used/{denominator} {}% ({} / {})",
                format_number(ratio * 100.0),
                current["usedFormatted"].as_str().unwrap_or("-"),
                total
            ),
        ));
    }
    let collection_ratio = usage_ratio(&collected);
    if old && collection_ratio.is_some_and(|(ratio, _, _)| ratio >= 0.85) {
        let (ratio, denominator, _) = collection_ratio.expect("checked above");
        signals.push(signal(
            "memory",
            "warning",
            format!("{name} collection usage 偏高"),
            format!(
                "collection used/{denominator} {}%",
                format_number(ratio * 100.0)
            ),
        ));
    }
    signals
}

fn gc_signals(name: &str, gc: &Value) -> Vec<Value> {
    let total = i64_at(gc, "total").unwrap_or_default();
    let avg_time = f64_at(gc, "avgTime");
    let avg_frequency = f64_at(gc, "avgFrequency");
    let lower = name.to_lowercase();
    let old_or_full = lower.contains("old") || lower.contains("full");
    let mut signals = vec![];
    if old_or_full && total > 0 && avg_time.is_some_and(|value| value >= 200.0) {
        let avg_time = avg_time.expect("checked above");
        signals.push(signal("gc","critical",format!("{name} 发生长暂停"),format!("old/full collector avg {}ms; this can cause visible tick spikes even when heap is not full",format_number(avg_time))));
    } else if total > 0 && avg_time.is_some_and(|value| value >= 200.0) {
        let avg_time = avg_time.expect("checked above");
        signals.push(signal(
            "gc",
            "critical",
            format!("{name} 平均暂停极高"),
            format!(
                "avg {}ms across {total} collections",
                format_number(avg_time)
            ),
        ));
    } else if total > 0
        && avg_time.is_some_and(|value| value >= 100.0 || (old_or_full && value >= 50.0))
    {
        let avg_time = avg_time.expect("checked above");
        signals.push(signal(
            "gc",
            "warning",
            format!("{name} 平均暂停偏高"),
            format!(
                "avg {}ms across {total} collections",
                format_number(avg_time)
            ),
        ));
    }
    if total > 0 && avg_frequency.is_some_and(|value| value > 0.0 && value <= 2000.0) {
        let avg_frequency = avg_frequency.expect("checked above");
        signals.push(signal(
            "gc",
            "warning",
            format!("{name} 触发频率很高"),
            format!(
                "average interval {}s",
                format_number(avg_frequency / 1000.0)
            ),
        ));
    }
    signals
}

fn interpretation(signals: &[Value], available: bool) -> &'static str {
    if !available {
        return "报告未提供可用的 GC/内存池聚合数据，不能据此判断内存或 GC 是否健康。";
    }
    let critical_gc = signals
        .iter()
        .any(|v| v["category"] == "gc" && v["severity"] == "critical");
    let memory = signals.iter().any(|v| v["category"] == "memory");
    if critical_gc {
        "GC 聚合数据存在严重暂停信号；这不是 OOM 结论，而是 STW/GC 行为可能制造卡顿尖峰。"
    } else if memory {
        "内存池存在压力信号；需要结合 GC 日志或 only-ticks-over 报告确认是否影响 tick。"
    } else if signals.iter().any(|v| v["category"] == "gc") {
        "GC 聚合数据存在警告信号；需要带时间戳的 GC 日志确认是否与 tick 尖峰相关。"
    } else {
        "未从聚合 GC/内存池数据中发现明显异常；仍可用 GC 日志排除单次停顿。"
    }
}

pub(crate) fn summarize_memory_gc(report: &Report) -> Value {
    summarize_memory_gc_raw(&report.raw)
}

fn summarize_memory_gc_raw(raw: &Value) -> Value {
    let heap = path(raw, "metadata.platformStatistics.memory.heap");
    let non_heap = path(raw, "metadata.platformStatistics.memory.nonHeap");
    let pools=arr_at(raw,"metadata.platformStatistics.memory.pools").into_iter().flatten().filter_map(|pool| {
        let usage=path(pool,"usage");
        let collection=path(pool,"collectionUsage");
        if usage.and_then(|value| i64_at(value,"used")).is_none() && collection.and_then(|value| i64_at(value,"used")).is_none() {
            return None;
        }
        let name=str_at(pool,"name").unwrap_or_default();
        Some(json!({"name":name,"usage":usage_summary(usage),"collectionUsage":usage_summary(collection),"signals":pool_signals(name,usage,collection)}))
    }).collect::<Vec<_>>();
    let gc_collectors = [
        ("system", obj_at(raw, "metadata.systemStatistics.gc")),
        (
            "platform",
            obj_at(raw, "metadata.platformStatistics.gc"),
        ),
    ]
    .into_iter()
    .flat_map(|(source, collectors)| {
        collectors.into_iter().flat_map(move |collectors| {
            collectors.iter().filter_map(move |(name, gc)| {
                let avg_frequency_ms = f64_at(gc, "avgFrequency");
                let total = i64_at(gc,"total");
                let avg_time = f64_at(gc,"avgTime");
                (total.is_some() || avg_time.is_some() || avg_frequency_ms.is_some()).then(|| json!({"source":source,"name":name,"total":total.unwrap_or_default(),"avgTimeMs":avg_time,"avgFrequencyMs":avg_frequency_ms,"avgFrequencySeconds":avg_frequency_ms.map(|value| value/1000.0),"signals":gc_signals(name,gc)}))
            })
        })
    })
    .collect::<Vec<_>>();
    let mut merged_collectors = HashMap::new();
    for mut collector in gc_collectors {
        let key = (
            collector["name"]
                .as_str()
                .unwrap_or_default()
                .to_ascii_lowercase(),
            collector["total"].as_i64().unwrap_or_default(),
            collector["avgTimeMs"].as_f64().map(f64::to_bits),
            collector["avgFrequencyMs"].as_f64().map(f64::to_bits),
        );
        let source = collector["source"].clone();
        collector["sources"] = json!([source]);
        let existing = merged_collectors.entry(key).or_insert(collector);
        if !existing["sources"]
            .as_array()
            .is_some_and(|sources| sources.contains(&source))
        {
            existing["sources"].as_array_mut().unwrap().push(source);
        }
    }
    let mut gc_collectors = merged_collectors.into_values().collect::<Vec<_>>();
    gc_collectors.sort_by(|a, b| {
        f64_at(b, "avgTimeMs")
            .unwrap_or_default()
            .total_cmp(&f64_at(a, "avgTimeMs").unwrap_or_default())
            .then_with(|| {
                f64_at(b, "avgTimeMs")
                    .is_some()
                    .cmp(&f64_at(a, "avgTimeMs").is_some())
            })
            .then_with(|| {
                a["name"]
                    .as_str()
                    .unwrap_or_default()
                    .cmp(b["name"].as_str().unwrap_or_default())
            })
            .then_with(|| {
                a["total"]
                    .as_i64()
                    .unwrap_or_default()
                    .cmp(&b["total"].as_i64().unwrap_or_default())
            })
            .then_with(|| {
                f64_at(a, "avgFrequencyMs")
                    .unwrap_or_default()
                    .total_cmp(&f64_at(b, "avgFrequencyMs").unwrap_or_default())
            })
            .then_with(|| {
                f64_at(b, "avgFrequencyMs")
                    .is_some()
                    .cmp(&f64_at(a, "avgFrequencyMs").is_some())
            })
    });
    let mut signals = heap_signals(heap);
    signals.extend(non_heap_signals(non_heap));
    signals.extend(
        pools
            .iter()
            .flat_map(|pool| pool["signals"].as_array().into_iter().flatten().cloned()),
    );
    signals.extend(
        gc_collectors
            .iter()
            .flat_map(|gc| gc["signals"].as_array().into_iter().flatten().cloned()),
    );
    let available = usage_summary(heap)["available"].as_bool().unwrap_or(false)
        || usage_summary(non_heap)["available"]
            .as_bool()
            .unwrap_or(false)
        || !pools.is_empty()
        || !gc_collectors.is_empty();
    json!({"available":available,"heap":usage_summary(heap),"nonHeap":usage_summary(non_heap),"pools":pools,"gcCollectors":gc_collectors,"signals":signals,"interpretation":interpretation(&signals,available)})
}

pub(crate) fn finding_signals(raw: &Value) -> Vec<Finding> {
    let result = summarize_memory_gc_raw(raw);
    result["signals"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            Some(Finding {
                severity: match item["severity"].as_str()? {
                    "critical" => Severity::Critical,
                    "warning" => Severity::Warning,
                    _ => Severity::Info,
                },
                title: item["title"].as_str()?.into(),
                detail: item["detail"].as_str()?.into(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ReportKind, ReportSummary};
    fn report(raw: Value) -> Report {
        Report {
            kind: ReportKind::Health,
            source: "x".into(),
            raw,
            summary: ReportSummary::default(),
        }
    }
    #[test]
    fn restores_memory_gc_shape_units_and_signals() {
        let r = report(
            json!({"metadata":{"platformStatistics":{"memory":{"heap":{"used":90,"committed":100,"max":100},"nonHeap":{"used":20,"committed":40,"max":0},"pools":[{"name":"G1 Old Gen","usage":{"used":93,"committed":100,"max":100},"collectionUsage":{"used":90,"committed":100,"max":100}}]},"gc":{"G1 Old Generation":{"total":4,"avgTime":250.0,"avgFrequency":1500.0}}}}}),
        );
        let result = summarize_memory_gc(&r);
        assert_eq!(result["heap"]["usedMaxRatio"], 0.9);
        assert!(result["nonHeap"]["usedMaxRatio"].is_null());
        assert_eq!(result["nonHeap"]["usedCommittedRatio"], 0.5);
        assert_eq!(result["gcCollectors"][0]["avgFrequencyMs"], 1500.0);
        assert_eq!(result["gcCollectors"][0]["avgFrequencySeconds"], 1.5);
        assert!(result["signals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["title"].as_str().unwrap().contains("长暂停")));
        assert!(finding_signals(&r.raw)
            .iter()
            .any(|v| v.title.contains("长暂停")));
    }

    #[test]
    fn committed_usage_without_a_hard_max_does_not_emit_pressure() {
        let r = report(json!({"metadata":{"platformStatistics":{"memory":{
            "heap":{"used":10,"committed":100,"max":100},
            "nonHeap":{"used":95,"committed":100,"max":0},
            "pools":[{"name":"G1 Old Gen","usage":{"used":93,"committed":100,"max":-1},"collectionUsage":{"used":85,"committed":100,"max":-1}}]
        }}}}));
        let result = summarize_memory_gc(&r);
        let signals = result["signals"].as_array().unwrap();
        assert!(signals.is_empty());
        assert_eq!(result["nonHeap"]["usedCommittedRatio"], 0.95);
        assert_eq!(result["pools"][0]["usage"]["usedCommittedRatio"], 0.93);
        assert!(result["nonHeap"]["usedMaxRatio"].is_null());
    }
}
