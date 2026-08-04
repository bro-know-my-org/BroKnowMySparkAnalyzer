use crate::analysis::{f64_at, format_number, i64_at, obj_at, path, str_at};
use crate::{Report, ReportKind};
use serde_json::{json, Map, Value};

fn compact(entries: impl IntoIterator<Item = (&'static str, Option<Value>)>) -> Value {
    Value::Object(
        entries
            .into_iter()
            .filter_map(|(key, value)| value.filter(|v| !v.is_null()).map(|v| (key.into(), v)))
            .collect(),
    )
}

fn copied(value: &Value, dotted: &str) -> Option<Value> {
    match path(value, dotted)? {
        Value::String(value) if !value.is_empty() => {
            Some(Value::String(value.chars().take(512).collect()))
        }
        Value::Number(value) => Some(Value::Number(value.clone())),
        Value::Bool(value) => Some(Value::Bool(*value)),
        _ => None,
    }
}

fn compact_value(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.retain(|_, v| !v.is_null() && v.as_str() != Some("-"));
    }
    value
}

fn ratio(used: i64, total: i64) -> f64 {
    if used < 0 || total <= 0 {
        0.0
    } else {
        (used as f64 / total as f64).clamp(0.0, 1.0)
    }
}

fn format_bytes(value: i64) -> String {
    if value <= 0 {
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

fn memory_pool(pool: Option<&Value>) -> Value {
    let used = pool.and_then(|v| i64_at(v, "used"));
    let total = pool.and_then(|v| i64_at(v, "total"));
    compact_value(json!({
        "used": used,
        "total": total,
        "usedFormatted": used.map(format_bytes),
        "totalFormatted": total.map(format_bytes),
        "usedRatio": used.zip(total).map(|(used,total)| ratio(used, total)),
    }))
}

fn format_duration(millis: i64) -> Option<String> {
    if millis <= 0 {
        return None;
    }
    let seconds = (millis as f64 / 1000.0).round() as i64;
    if seconds == 0 {
        return None;
    }
    let parts = [
        (seconds / 86_400, "d"),
        ((seconds % 86_400) / 3_600, "h"),
        ((seconds % 3_600) / 60, "m"),
        (seconds % 60, "s"),
    ];
    Some(
        parts
            .into_iter()
            .filter(|(n, _)| *n > 0)
            .map(|(n, unit)| format!("{n}{unit}"))
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn vm_args(value: Option<&str>) -> Option<Value> {
    let original = value?;
    let value = original.chars().take(16 * 1024).collect::<String>();
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let prefixes = [
        "-Xms",
        "-Xmx",
        "-XX:+Use",
        "-XX:Max",
        "-XX:G1",
        "-XX:+AlwaysPreTouch",
        "-XX:+DisableExplicitGC",
        "-javaagent",
    ];
    let mut count = 0usize;
    let mut important = Vec::new();
    for arg in value.split_whitespace().take(4_096) {
        count += 1;
        if important.len() < 32 && prefixes.iter().any(|prefix| arg.starts_with(prefix)) {
            important.push(if arg.starts_with("-javaagent") {
                "-javaagent=<redacted>".to_owned()
            } else {
                arg.chars().take(160).collect()
            });
        }
    }
    Some(
        json!({"count": count, "important": important, "inputTruncated": original.chars().nth(16 * 1024).is_some(),"argumentsTruncated":value.split_whitespace().nth(4_096).is_some()}),
    )
}

fn sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "password",
        "passwd",
        "secret",
        "token",
        "api_key",
        "apikey",
        "authorization",
        "credential",
        "connection_string",
        "private_key",
    ]
    .iter()
    .any(|needle| key.contains(needle))
}

fn allowed_environment_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace('_', "-");
    [
        "view-distance",
        "simulation-distance",
        "max-players",
        "online-mode",
        "difficulty",
        "gamemode",
        "hardcore",
        "pvp",
        "allow-flight",
        "white-list",
        "enforce-whitelist",
        "spawn-protection",
        "entity-broadcast-range-percentage",
        "implementation-version",
        "server-version",
    ]
    .contains(&normalized.as_str())
}

fn scalar_text(value: &Value, scan_limit: usize) -> String {
    match value {
        Value::String(s) => s.chars().take(scan_limit).collect(),
        Value::Array(values) => format!("[{} items]", values.len()),
        Value::Object(values) => format!("{{{} keys}}", values.len()),
        other => other.to_string(),
    }
}

fn top_key_values(
    value: Option<&Map<String, Value>>,
    limit: usize,
    value_limit: usize,
) -> Vec<Value> {
    let mut entries = value
        .into_iter()
        .flat_map(Map::iter)
        .filter(|(key, _)| allowed_environment_key(key))
        .collect::<Vec<_>>();
    entries.sort_by_key(|(key, _)| *key);
    entries
        .into_iter()
        .take(limit)
        .map(|(key, value)| {
            let key: String = key.chars().take(160).collect();
            if sensitive_key(&key) {
                return json!({"key":key,"value":"<redacted>","redacted":true});
            }
            let normalized = scalar_text(value, value_limit.saturating_mul(4).max(value_limit + 1))
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let length = normalized.chars().count();
            let truncated = length > value_limit;
            let shown = if truncated {
                format!(
                    "{}...",
                    normalized.chars().take(value_limit).collect::<String>()
                )
            } else {
                normalized
            };
            json!({"key":key,"value":shown,"truncated":truncated,"length":length})
        })
        .collect()
}

fn gc_summary(gc: Option<&Map<String, Value>>) -> Vec<String> {
    gc.into_iter()
        .flat_map(Map::iter)
        .take(32)
        .map(|(name, item)| {
            let name: String = name.chars().take(160).collect();
            format!(
                "{name}: total {}, avg {}ms, freq {}",
                i64_at(item, "total").unwrap_or_default(),
                format_number(f64_at(item, "avgTime").unwrap_or_default()),
                format_number(f64_at(item, "avgFrequency").unwrap_or_default())
            )
        })
        .collect()
}

fn platform_type(raw: &Value) -> Option<Value> {
    match path(raw, "metadata.platformMetadata.type")? {
        Value::String(value) if !value.is_empty() => Some(Value::String(value.clone())),
        Value::Number(value) => Some(Value::String(
            match value.as_i64()? {
                0 => "SERVER",
                1 => "CLIENT",
                2 => "PROXY",
                3 => "APPLICATION",
                _ => return None,
            }
            .into(),
        )),
        _ => None,
    }
}

pub(crate) fn summarize_environment(report: &Report) -> Value {
    if report.kind == ReportKind::Text || !has_environment(report) {
        return json!({"available":false,"note":"文本输入没有 spark protobuf metadata，无法读取报告内运行环境。"});
    }
    let raw = &report.raw;
    let sources = obj_at(raw, "metadata.sources");
    let source_count = sources.map_or(0, Map::len);
    let builtin_count = sources
        .into_iter()
        .flat_map(Map::values)
        .filter(|source| path(source, "builtin").and_then(Value::as_bool) == Some(true))
        .count();
    let external_count = sources
        .into_iter()
        .flat_map(Map::values)
        .filter(|source| path(source, "builtin").and_then(Value::as_bool) == Some(false))
        .count();
    let mut source_entries = sources
        .into_iter()
        .flat_map(Map::iter)
        .map(|(id, source)| {
            let id: String = id.chars().take(160).collect();
            compact_value(json!({
                "id": id,
                "name": str_at(source,"name").unwrap_or(&id).chars().take(200).collect::<String>(),
                "version": str_at(source,"version").map(|value| Value::String(value.chars().take(120).collect())),
                "author": str_at(source,"author").map(|value| Value::String(value.chars().take(160).collect())),
                "builtin": path(source,"builtin").and_then(Value::as_bool),
            }))
        })
        .collect::<Vec<_>>();
    source_entries.sort_by(|a, b| {
        a["builtin"]
            .as_bool()
            .cmp(&b["builtin"].as_bool())
            .then_with(|| a["name"].as_str().cmp(&b["name"].as_str()))
    });
    let physical = path(raw, "metadata.systemStatistics.memory.physical");
    let swap = path(raw, "metadata.systemStatistics.memory.swap");
    let disk_used = i64_at(raw, "metadata.systemStatistics.disk.used");
    let disk_total = i64_at(raw, "metadata.systemStatistics.disk.total");
    let uptime = i64_at(raw, "metadata.systemStatistics.uptime");
    json!({
        "available": true,
        "source": "spark report metadata",
        "platform": compact([("type",platform_type(raw)),("name",copied(raw,"metadata.platformMetadata.name")),("version",copied(raw,"metadata.platformMetadata.version")),("minecraftVersion",copied(raw,"metadata.platformMetadata.minecraftVersion")),("sparkVersion",copied(raw,"metadata.platformMetadata.sparkVersion")),("brand",copied(raw,"metadata.platformMetadata.brand"))]),
        "os": compact([("name",copied(raw,"metadata.systemStatistics.os.name")),("version",copied(raw,"metadata.systemStatistics.os.version")),("arch",copied(raw,"metadata.systemStatistics.os.arch"))]),
        "java": compact([("vendor",copied(raw,"metadata.systemStatistics.java.vendor")),("version",copied(raw,"metadata.systemStatistics.java.version")),("vendorVersion",copied(raw,"metadata.systemStatistics.java.vendorVersion")),("vmArgs",vm_args(str_at(raw,"metadata.systemStatistics.java.vmArgs")))]),
        "jvm": compact([("name",copied(raw,"metadata.systemStatistics.jvm.name")),("vendor",copied(raw,"metadata.systemStatistics.jvm.vendor")),("version",copied(raw,"metadata.systemStatistics.jvm.version"))]),
        "cpu": compact([("modelName",copied(raw,"metadata.systemStatistics.cpu.modelName")),("threads",copied(raw,"metadata.systemStatistics.cpu.threads")),("processUsage1m",copied(raw,"metadata.systemStatistics.cpu.processUsage.last1m")),("processUsage15m",copied(raw,"metadata.systemStatistics.cpu.processUsage.last15m")),("systemUsage1m",copied(raw,"metadata.systemStatistics.cpu.systemUsage.last1m")),("systemUsage15m",copied(raw,"metadata.systemStatistics.cpu.systemUsage.last15m"))]),
        "physicalMemory": memory_pool(physical),
        "swapMemory": memory_pool(swap),
        "disk": compact_value(json!({"used":disk_used,"total":disk_total,"usedFormatted":disk_used.map(format_bytes),"totalFormatted":disk_total.map(format_bytes),"usedRatio":disk_used.zip(disk_total).map(|(used,total)|ratio(used,total))})),
        "uptime": compact([("millis",uptime.map(|value|json!(value))),("formatted",uptime.and_then(format_duration).map(Value::String))]),
        "networkInterfaceCount": obj_at(raw,"metadata.systemStatistics.net").map_or(0,Map::len),
        "gcCollectors": gc_summary(obj_at(raw,"metadata.systemStatistics.gc")),
        "serverConfigurations": top_key_values(obj_at(raw,"metadata.serverConfigurations"),48,360),
        "extraPlatformMetadata": top_key_values(obj_at(raw,"metadata.extraPlatformMetadata"),48,360),
        "sources": {"count":source_count,"builtinCount":builtin_count,"externalCount":external_count,"truncated":source_count>80,"top":source_entries.into_iter().take(80).collect::<Vec<_>>()},
        "interpretation": [
            "这些字段来自 spark 报告 metadata，只能作为运行环境、版本、配置和资源上下文。",
            "它不能单独证明 TPS/MSPT 根因；根因仍需结合 hotspots/hot_paths/mod_sources/time windows/GC 等证据。"
        ]
    })
}

pub(crate) fn has_environment(report: &Report) -> bool {
    [
        "metadata.platformMetadata",
        "metadata.systemStatistics",
        "metadata.serverConfigurations",
        "metadata.extraPlatformMetadata",
        "metadata.sources",
    ]
    .into_iter()
    .filter_map(|key| path(&report.raw, key))
    .any(|value| match value {
        Value::Null => false,
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
        Value::Bool(_) | Value::Number(_) => true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ReportSummary;

    fn report(raw: Value) -> Report {
        Report {
            kind: ReportKind::Sampler,
            source: "fixture".into(),
            raw,
            summary: ReportSummary::default(),
        }
    }

    #[test]
    fn normalizes_environment_and_keeps_evidence_boundary() {
        let result = summarize_environment(&report(
            json!({"metadata":{"platformMetadata":{"type":0,"name":"Paper","version":"1.2"},"systemStatistics":{"cpu":{"threads":8,"processUsage":{"last1m":42.0}},"memory":{"physical":{"used":50,"total":100}},"disk":{"used":25,"total":100},"uptime":61000,"net":{"eth0":{}},"java":{"vmArgs":"-Xmx4G -Dfoo=bar"}},"serverConfigurations":{"view-distance":"10","api-token":"secret"},"sources":{"builtin":{"name":"Minecraft","builtin":true},"mod":{"name":"Alpha","builtin":false}}}}),
        ));
        assert_eq!(result["available"], true);
        assert_eq!(result["platform"]["type"], "SERVER");
        assert_eq!(result["physicalMemory"]["usedRatio"], 0.5);
        assert_eq!(result["serverConfigurations"][0]["key"], "view-distance");
        assert_eq!(result["serverConfigurations"].as_array().unwrap().len(), 1);
        assert_eq!(result["sources"]["top"][0]["id"], "mod");
        assert!(result["sources"]["top"][0].get("version").is_none());
        assert_eq!(result["interpretation"].as_array().unwrap().len(), 2);
    }
}
