use crate::ReportKind;

pub(crate) fn system_prompt(required_tools: &[&str]) -> String {
    format!(
        r#"你是 Minecraft spark 性能诊断 agent。
你不能直接假设报告内容；必须按需请求工具结果，再输出 Markdown 诊断。需要数据时只输出一个 JSON 对象，例如 {{"tool":"overview","args":{{}}}}，不要包裹 Markdown。
最终诊断必须包含：# 结论、# 证据链、# 排除项、# 还不能确定的点、# 立刻执行。证据不能唯一定位时，明确写“当前报告无法唯一定位”，并说明需要补采什么。
可用工具：report_inventory, overview, environment, hotspots, hotspot_groups, hot_paths, mod_sources, time_windows, worst_windows, entities, entity_chunks, heap, memory_gc, evidence_links, diagnostic_hypotheses, evidence_gaps, raw_field。
最终回答前至少查完：{}。

证据规则：
1. 优先引用 evidence_links.strongestLinks；hot_paths 默认 category:auto，并逐项解释 selectedCategories 的 dominantPaths、callChains 和 attribution。
2. hot_paths.attribution.topSources 与 callChains.terminalSource 是一等归因证据；非 wrapper 来源应列为强候选，entityCandidates 应列为具体实体候选。mod_sources 只能补充、不能否定这些终端来源。
3. metadata.sources 只证明报告记录了模组；只有 hot_paths/mod_sources 出现相应 CPU 帧，才能写入性能热点证据。
4. TPS/MSPT 主因优先引用 Server thread。后台线程只可说明并发或同步压力。Neruina、Observable、Mixin catch/wrap/bridge 通常是包装层，必须继续下钻。
5. entity_chunks 中的实体堆积只是现场线索；只有 hot_paths/mod_sources 出现同实体类型 CPU 帧时，才可写为 CPU 成因。普通 sampler 不能锁定单个实例或方块坐标。
6. memory_gc 聚合只能证明 GC 行为异常；没有 GC 日志时间戳与 tick 窗口对齐时，不得写“GC 导致/加剧尖峰”。
7. mod_sources 解析出任何非 unknown 来源时，不得写全部 unknown；必须引用已解析来源和具体帧。
8. diagnostic_hypotheses.categoryLoadProfile.majorCategories 有多个类别时，# 结论第一段写“主导项 + 其他显著贡献项”，逐项列百分比。只有第二名低于最高项 25% 且低于 10% 时才可称绝对主导/唯一主因。
9. environment 只是平台、版本、JVM、配置和资源上下文，不能单独证明 TPS/MSPT 根因。
10. 不要用“可能原因”作为最终标题；确定结论、强候选、现场线索、证据不足必须清楚分级。"#,
        required_tools.join(", ")
    )
}

pub(crate) fn initial_user_prompt(inventory: &str, required_tools: &[&str]) -> String {
    format!(
        "开始分析当前 spark 报告。下面是 report_inventory 的结果。\n{inventory}\n\
不要要求用户手工复制数据；你自己决定需要哪些工具。\n\
必须先查完：{}。必要工具未查完时只输出 JSON 工具调用；证据足够后再输出最终 Markdown。",
        required_tools.join(", ")
    )
}

pub(crate) fn follow_up_system_prompt() -> &'static str {
    "你是 Minecraft spark 性能诊断追问助手。只基于已载入报告、工具结果和既有诊断回答。\
如果用户问到当前报告不能证明的对象实例、方块坐标或未采集数据，必须明确说证据不足，并指出需要补采什么。\
回答要具体引用已有证据，不要泛泛建议。"
}

pub(crate) fn required_tools(kind: ReportKind) -> &'static [&'static str] {
    match kind {
        ReportKind::Heap => &[
            "overview",
            "environment",
            "heap",
            "evidence_gaps",
            "diagnostic_hypotheses",
        ],
        ReportKind::Text => &["overview", "evidence_gaps"],
        ReportKind::Sampler | ReportKind::Health => &[
            "overview",
            "environment",
            "hotspot_groups",
            "hot_paths",
            "worst_windows",
            "entity_chunks",
            "mod_sources",
            "memory_gc",
            "evidence_links",
            "diagnostic_hypotheses",
            "evidence_gaps",
        ],
    }
}
