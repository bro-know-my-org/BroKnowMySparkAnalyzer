use bkmsa_core::Report;
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

/// Native report storage keeps large protobuf-derived trees out of the WebView and makes the
/// report id the only stateful value the TypeScript UI needs to retain.
pub struct AnalyzerState {
    next_report_id: AtomicU64,
    next_analysis_id: AtomicU64,
    inner: Mutex<AnalyzerStateInner>,
}

#[derive(Default)]
struct AnalyzerStateInner {
    reports: HashMap<String, Arc<Report>>,
    report_order: VecDeque<String>,
    analysis_runs: HashMap<String, (u64, tokio_util::sync::CancellationToken)>,
}

const MAX_REPORT_SESSIONS: usize = 8;

impl Default for AnalyzerState {
    fn default() -> Self {
        Self {
            next_report_id: AtomicU64::new(1),
            next_analysis_id: AtomicU64::new(1),
            inner: Mutex::new(AnalyzerStateInner::default()),
        }
    }
}

impl Drop for AnalyzerState {
    fn drop(&mut self) {
        let inner = self
            .inner
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (_, token) in inner.analysis_runs.values() {
            token.cancel();
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedReport {
    pub report_id: String,
    pub kind: String,
    pub source: String,
    pub summary: Value,
}

impl AnalyzerState {
    pub fn insert(&self, report: Report) -> Result<LoadedReport, String> {
        let report_id = format!(
            "report-{}",
            self.next_report_id.fetch_add(1, Ordering::Relaxed)
        );
        let loaded = LoadedReport {
            report_id: report_id.clone(),
            kind: report.kind.to_string(),
            source: report.source.clone(),
            summary: serde_json::to_value(&report.summary).map_err(|error| error.to_string())?,
        };
        let evicted_run = {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| "报告会话锁已损坏".to_string())?;
            inner.reports.insert(report_id.clone(), Arc::new(report));
            inner.report_order.push_back(report_id);
            if inner.reports.len() > MAX_REPORT_SESSIONS {
                if let Some(evicted_id) = inner.report_order.pop_front() {
                    inner.reports.remove(&evicted_id);
                    inner.analysis_runs.remove(&evicted_id)
                } else {
                    inner.reports.remove(&loaded.report_id);
                    return Err("报告会话顺序状态不一致".into());
                }
            } else {
                None
            }
        };
        if let Some((_, token)) = evicted_run {
            token.cancel();
        }
        Ok(loaded)
    }

    pub fn get(&self, report_id: &str) -> Result<Arc<Report>, String> {
        self.inner
            .lock()
            .map_err(|_| "报告会话锁已损坏".to_string())?
            .reports
            .get(report_id)
            .cloned()
            .ok_or_else(|| format!("报告会话不存在或已释放: {report_id}"))
    }

    pub fn remove(&self, report_id: &str) -> Result<bool, String> {
        let (removed, run) = {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| "报告会话锁已损坏".to_string())?;
            let removed = inner.reports.remove(report_id).is_some();
            if removed {
                inner.report_order.retain(|id| id != report_id);
            }
            (removed, inner.analysis_runs.remove(report_id))
        };
        if let Some((_, token)) = run {
            token.cancel();
        }
        Ok(removed)
    }

    pub fn begin_analysis(
        &self,
        report_id: &str,
    ) -> Result<(u64, tokio_util::sync::CancellationToken), String> {
        let analysis_id = self.next_analysis_id.fetch_add(1, Ordering::Relaxed);
        let token = tokio_util::sync::CancellationToken::new();
        let previous = {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| "分析任务锁已损坏".to_string())?;
            if !inner.reports.contains_key(report_id) {
                return Err(format!("报告会话不存在或已释放: {report_id}"));
            }
            inner
                .analysis_runs
                .insert(report_id.to_string(), (analysis_id, token.clone()))
        };
        if let Some((_, previous)) = previous {
            previous.cancel();
        }
        Ok((analysis_id, token))
    }

    pub fn finish_analysis(&self, report_id: &str, analysis_id: u64) -> Result<(), String> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "分析任务锁已损坏".to_string())?;
        if inner
            .analysis_runs
            .get(report_id)
            .is_some_and(|(current_id, _)| *current_id == analysis_id)
        {
            inner.analysis_runs.remove(report_id);
        }
        Ok(())
    }

    pub fn cancel_analysis(&self, report_id: &str) -> Result<bool, String> {
        let token = self
            .inner
            .lock()
            .map_err(|_| "分析任务锁已损坏".to_string())?
            .analysis_runs
            .remove(report_id);
        if let Some((_, token)) = token {
            token.cancel();
            return Ok(true);
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bkmsa_core::parse_text_report;

    #[test]
    fn report_sessions_are_opaque_and_releasable() {
        let state = AnalyzerState::default();
        let loaded = state
            .insert(parse_text_report("TPS: 20", "clipboard").unwrap())
            .expect("insert report");
        assert_eq!(loaded.kind, "text");
        assert_eq!(
            state.get(&loaded.report_id).expect("get report").source,
            "clipboard"
        );
        assert!(state.remove(&loaded.report_id).expect("remove report"));
        assert!(state.get(&loaded.report_id).is_err());
    }

    #[test]
    fn finishing_an_old_run_does_not_drop_a_new_run_token() {
        let state = AnalyzerState::default();
        let loaded = state
            .insert(parse_text_report("TPS: 20", "clipboard").unwrap())
            .expect("insert report");
        let (first_id, first) = state.begin_analysis(&loaded.report_id).expect("first run");
        let (_second_id, second) = state.begin_analysis(&loaded.report_id).expect("second run");
        assert!(first.is_cancelled());
        assert!(!second.is_cancelled());

        state
            .finish_analysis(&loaded.report_id, first_id)
            .expect("finish old run");
        assert!(state
            .cancel_analysis(&loaded.report_id)
            .expect("cancel new run"));
        assert!(second.is_cancelled());
    }

    #[test]
    fn released_reports_cannot_start_new_analysis() {
        let state = AnalyzerState::default();
        let loaded = state
            .insert(parse_text_report("TPS: 20", "clipboard").unwrap())
            .expect("insert report");
        assert!(state.remove(&loaded.report_id).expect("release report"));
        assert!(state.begin_analysis(&loaded.report_id).is_err());
    }

    #[test]
    fn evicts_the_oldest_report_and_cancels_its_analysis() {
        let state = AnalyzerState::default();
        let first = state
            .insert(parse_text_report("first", "first").unwrap())
            .expect("insert first");
        let (_, token) = state.begin_analysis(&first.report_id).expect("begin run");
        for index in 1..=MAX_REPORT_SESSIONS {
            state
                .insert(parse_text_report(index.to_string(), index.to_string()).unwrap())
                .expect("insert report");
        }
        assert!(state.get(&first.report_id).is_err());
        assert!(token.is_cancelled());
    }
}
