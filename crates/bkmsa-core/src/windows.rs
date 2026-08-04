use crate::analysis::{f64_at, obj_at};
use crate::Report;
use serde_json::{json, Map, Value};

fn sorted_rows(report: &Report) -> Vec<Value> {
    let mut rows = obj_at(&report.raw, "timeWindowStatistics")
        .into_iter()
        .flat_map(Map::iter)
        .filter_map(|(id, value)| {
            let mut object = value.as_object().cloned()?;
            object.insert("id".into(), Value::String(id.clone()));
            Some(Value::Object(object))
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        let left_id = left["id"].as_str().and_then(|v| v.parse::<i64>().ok());
        let right_id = right["id"].as_str().and_then(|v| v.parse::<i64>().ok());
        left_id
            .cmp(&right_id)
            .then_with(|| left["id"].as_str().cmp(&right["id"].as_str()))
    });
    rows
}

fn delta(current: &Value, previous: Option<&Value>, key: &str) -> Option<f64> {
    Some(f64_at(current, key)? - f64_at(previous?, key)?)
}

fn enriched_rows(report: &Report) -> Vec<Value> {
    let rows = sorted_rows(report);
    rows.iter()
        .enumerate()
        .map(|(index, window)| {
            let previous = index.checked_sub(1).and_then(|i| rows.get(i));
            let next = rows.get(index + 1);
            let mut object = window.as_object().cloned().unwrap_or_default();
            object.insert(
                "score".into(),
                json!(f64_at(window, "msptMax")
                    .or_else(|| f64_at(window, "msptMedian"))
                    .unwrap_or_default()),
            );
            let mut deltas = Map::new();
            for (output, key) in [
                ("entitiesFromPrevious", "entities"),
                ("chunksFromPrevious", "chunks"),
                ("playersFromPrevious", "players"),
                ("tpsFromPrevious", "tps"),
            ] {
                if let Some(value) = delta(window, previous, key) {
                    deltas.insert(output.into(), json!(value));
                }
            }
            object.insert("deltas".into(), Value::Object(deltas));
            if let Some(next) = next {
                let mut next_window = Map::new();
                for key in [
                    "id",
                    "tps",
                    "msptMedian",
                    "msptMax",
                    "entities",
                    "chunks",
                    "players",
                ] {
                    if let Some(value) = next.get(key) {
                        next_window.insert(key.into(), value.clone());
                    }
                }
                object.insert("nextWindow".into(), Value::Object(next_window));
            }
            Value::Object(object)
        })
        .collect()
}

pub(crate) fn time_windows(report: &Report, limit: usize) -> Value {
    json!({"windows":sorted_rows(report).into_iter().take(limit).collect::<Vec<_>>()})
}

pub(crate) fn worst_windows(report: &Report, limit: usize) -> Value {
    let enriched = enriched_rows(report);
    let mut max = enriched
        .iter()
        .filter(|row| f64_at(row, "msptMax").is_some())
        .cloned()
        .collect::<Vec<_>>();
    max.sort_by(|a, b| {
        f64_at(b, "msptMax")
            .unwrap_or_default()
            .total_cmp(&f64_at(a, "msptMax").unwrap_or_default())
    });
    let mut median = enriched
        .iter()
        .filter(|row| f64_at(row, "msptMedian").is_some())
        .cloned()
        .collect::<Vec<_>>();
    median.sort_by(|a, b| {
        f64_at(b, "msptMedian")
            .unwrap_or_default()
            .total_cmp(&f64_at(a, "msptMedian").unwrap_or_default())
    });
    let mut low_tps = enriched
        .into_iter()
        .filter(|row| f64_at(row, "tps").is_some())
        .collect::<Vec<_>>();
    low_tps.sort_by(|a, b| {
        f64_at(a, "tps")
            .unwrap_or(20.0)
            .total_cmp(&f64_at(b, "tps").unwrap_or(20.0))
    });
    json!({"worstByMaxMspt":max.into_iter().take(limit).collect::<Vec<_>>(),"worstByMedianMspt":median.into_iter().take(limit).collect::<Vec<_>>(),"lowTpsWindows":low_tps.into_iter().take(limit).collect::<Vec<_>>()})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ReportKind, ReportSummary};
    #[test]
    fn sorts_ids_before_computing_deltas_and_builds_three_rankings() {
        let report = Report {
            kind: ReportKind::Health,
            source: "x".into(),
            summary: ReportSummary::default(),
            raw: json!({"timeWindowStatistics":{"10":{"tps":18.0,"msptMedian":40.0,"msptMax":100.0,"entities":130},"2":{"tps":20.0,"msptMedian":50.0,"msptMax":80.0,"entities":100},"3":{"tps":15.0,"msptMedian":30.0,"msptMax":120.0,"entities":110}}}),
        };
        let chronological = time_windows(&report, 10);
        assert_eq!(chronological["windows"][0]["id"], "2");
        let result = worst_windows(&report, 10);
        assert!(result["worstByMedianMspt"][0]["deltas"]
            .as_object()
            .unwrap()
            .is_empty());
        assert_eq!(result["worstByMaxMspt"][0]["id"], "3");
        assert_eq!(result["worstByMedianMspt"][0]["id"], "2");
        assert_eq!(result["lowTpsWindows"][0]["id"], "3");
        assert_eq!(
            result["worstByMaxMspt"][0]["deltas"]["entitiesFromPrevious"],
            10.0
        );
        assert_eq!(result["worstByMaxMspt"][0]["nextWindow"]["id"], "10");
    }
}
