import type { AnyRecord, ReportDocument } from "./report";
import { executeReportTool } from "./report";
import type { SparkAnalyzerAdapter, AiMessage } from "./adapter";

export type ProviderPreset = {
  id: string;
  name: string;
  baseUrl: string;
  model: string;
  customBaseUrl?: boolean;
};

export type AiConfig = {
  base_url: string;
  api_key: string;
  model: string;
  temperature: number;
};

export type AiModelInfo = {
  id: string;
};

export type AgentTrace = {
  round: number;
  role: "assistant" | "tool" | "system";
  title: string;
  content: string;
};

export type FollowUpMessage = {
  role: "user" | "assistant";
  content: string;
};

export const providerPresets: ProviderPreset[] = [
  { id: "custom", name: "Custom OpenAI-compatible", baseUrl: "", model: "", customBaseUrl: true },
  { id: "openai", name: "OpenAI", baseUrl: "https://api.openai.com/v1", model: "gpt-4.1-mini" },
  { id: "deepseek", name: "DeepSeek", baseUrl: "https://api.deepseek.com/v1", model: "deepseek-chat" },
  { id: "moonshot", name: "Moonshot", baseUrl: "https://api.moonshot.cn/v1", model: "kimi-k2-0711-preview" },
  {
    id: "siliconflow",
    name: "SiliconFlow",
    baseUrl: "https://api.siliconflow.cn/v1",
    model: "Qwen/Qwen3-235B-A22B-Instruct-2507",
  },
  { id: "openrouter", name: "OpenRouter", baseUrl: "https://openrouter.ai/api/v1", model: "openai/gpt-4.1-mini" },
  {
    id: "newapi-happy",
    name: "NewAPI Happy (test)",
    baseUrl: "https://newapi.hello-happy.world/v1",
    model: "deepseek-v4-pro",
  },
];

export async function testConnection(config: AiConfig, adapter: SparkAnalyzerAdapter) {
  return adapter.testAiConnection(config);
}

export async function listModels(config: AiConfig, adapter: SparkAnalyzerAdapter) {
  return adapter.listAiModels(config);
}

export async function askFollowUp(
  report: ReportDocument,
  config: AiConfig,
  traces: AgentTrace[],
  diagnosis: string,
  history: FollowUpMessage[],
  question: string,
  adapter: SparkAnalyzerAdapter,
) {
  const toolContext = traces
    .filter((trace) => trace.role === "tool" || trace.role === "system")
    .slice(-12)
    .map((trace) => `${trace.title}\n${trace.content.slice(0, 5000)}`)
    .join("\n\n---\n\n")
    .slice(0, 32000);

  const messages: AiMessage[] = [
    {
      role: "system",
      content: [
        "你是 Minecraft spark 性能诊断追问助手。",
        "只基于已载入报告、工具结果和既有诊断回答。",
        "如果用户问到当前报告不能证明的对象实例、方块坐标或未采集数据，必须明确说证据不足，并指出需要补采什么。",
        "回答要具体引用已有证据，不要泛泛建议。",
      ].join("\n"),
    },
    {
      role: "user",
      content: [
        "当前报告摘要:",
        JSON.stringify({
          kind: report.kind,
          source: report.source,
          summary: report.summary,
        }, null, 2).slice(0, 10000),
        "\n\n当前诊断结论:",
        diagnosis.slice(0, 12000),
        "\n\n已调用工具证据:",
        toolContext,
      ].join("\n"),
    },
    ...history.slice(-8).map((item) => ({
      role: item.role,
      content: item.content,
    } satisfies AiMessage)),
    {
      role: "user",
      content: question,
    },
  ];

  return adapter.callAiChat({ config, messages });
}

export async function runToolAgent(
  report: ReportDocument,
  config: AiConfig,
  onTrace: (trace: AgentTrace) => void,
  adapter: SparkAnalyzerAdapter,
) {
  const requiredTools = requiredToolsForReport(report);
  const usedTools = new Set<string>(["report_inventory"]);
  const evidenceState = {
    modSourcesResolved: false,
    modSourcesNames: [] as string[],
    hotPathSourcesResolved: false,
    hotPathSourceNames: [] as string[],
    hotPathEntityCandidates: [] as string[],
    entityChunkNames: [] as string[],
    hotPathText: "",
    selectedHotPathCategories: [] as string[],
  };
  const inventory = executeReportTool(report, "report_inventory", {});
  const inventoryText = JSON.stringify(inventory, null, 2);
  onTrace({
    round: 0,
    role: "tool",
    title: "Tool: report_inventory",
    content: inventoryText,
  });

  const messages: AiMessage[] = [
    {
      role: "system",
      content: [
        "你是 Minecraft spark 性能诊断 agent。",
        "你不能直接假设报告内容。你需要按需请求工具结果，再输出 Markdown 诊断。",
        "你的目标不是写泛泛的“可能原因”，而是尽量精确到可验证结论。",
        "最终诊断必须使用这些栏目：# 结论、# 证据链、# 排除项、# 还不能确定的点、# 立刻执行。",
        "如果证据不能唯一定位，不要伪装成确定；写“证据不足以唯一定位”，并说明还需要补采哪种 spark profile。",
        "当你需要数据时，只输出一个 JSON 对象，不要包裹 markdown：",
        '{"tool":"overview","args":{}}',
        "可用工具: report_inventory, overview, environment, hotspots, hotspot_groups, hot_paths, mod_sources, time_windows, worst_windows, entities, entity_chunks, heap, memory_gc, diagnostic_hypotheses, evidence_gaps, raw_field。",
        `最终回答前必须至少查完这些工具: ${requiredTools.join(", ")}。`,
        "diagnostic_hypotheses 是本地规则生成的候选结论；evidence_gaps 会告诉你当前报告不能证明什么。",
        "environment 是报告内 metadata/system statistics/source list，不是本机信息采集。它只能作为平台、版本、Java/JVM、服务器配置、资源上下文，不能单独证明 TPS/MSPT 根因。",
        "证据一致性硬规则：metadata.sources 只能证明报告记录了这些 mod/plugin；只有 mod_sources/hot_paths 把 CPU 帧解析到该来源时，才能把该来源写进性能热点证据。",
        "hot_paths 默认使用 category:auto：先根据 hotspot_groups 自动选择高占比且可下钻的类别，再分别展开具体子帧。最终回答必须引用 hot_paths 的自动下钻结果，而不是停在聚合入口。",
        "hot_paths 会返回 dominantPaths、frames、callChains 和 attribution；最终结论必须先用 dominantPaths 说明火焰图最大子块逐层钻入结果，再用 attribution.topSources / attribution.entityCandidates 做模组/实体归因。",
        "证据优先级硬规则：hot_paths.attribution.topSources 和 hot_paths.callChains 的 terminalSource 是一等性能归因证据。mod_sources 是补充视角；如果 mod_sources 没把某个 terminal source 汇总到 topSources，不能据此否定 hot_paths 已经解析出的终端模组/实体类。",
        "下钻完整性硬规则：hot_paths.attribution.byCategory 必须逐个解释 selectedCategories。不能只写全局 topSources；entity_tick、chunk_task、block_entity/world_tick 等高占用类别要分别列出 dominantPaths 钻入链路和 terminal classes/sources/entities。",
        "证据表达硬规则：hot_paths.attribution.topSources 里的非 wrapper source 必须列为“强候选/优先排查对象”；entityCandidates 必须列成具体实体/生物候选。可以说它们不是唯一锁定根因，但不能写成“不能作为重点怀疑”。",
        "证据一致性硬规则：TPS/MSPT 主因只能优先引用 Server thread 证据。LDLib Async Thread、Netty、Worker 等后台线程可作为并发/同步压力说明，但不能直接当主线程 tick 根因。",
        "证据一致性硬规则：Neruina、Observable、Mixin catch/wrap/bridge 类帧通常是保护/观测/注入包装层。除非其自身下游没有更具体热点，否则必须继续看 callChains 的下游 terminal frame，不能把包装层写成根因。",
        "证据一致性硬规则：hot_paths.selectedCategories 中出现的每个高占比类别，都必须在最终 # 结论 或 # 证据链 中出现；尤其 block_entity 不能被 entity_tick/chunk_task 掩盖。",
        "证据一致性硬规则：如果 mod_sources 对某个来源给出了具体帧，最终回答必须引用这些帧，不能再写该来源“未解析到”。",
        "证据一致性硬规则：如果 mod_sources.resolvedSourceCount > 0 或 notableSources 非空，禁止写“mod_sources 全部 unknown / 无模组来源可解析帧 / 无法解析任何模组来源”。只能写 unknown 占比高、部分帧未解析。",
        "证据一致性硬规则：禁止写“其余帧均为 unknown”。unknown/unattributed 代表未归因框架/runtime/原版帧，不代表报告没有 mod 名；如果解析到 mod source，必须列出主要 mod 名和帧。",
        "证据一致性硬规则：如果只有 entity_chunks 里某个实体命名空间堆积，但 mod_sources/hotspot_groups 没有同来源帧或相关热点，只能写“现场线索/可疑载体”，不能写“直接成因”。",
        "证据一致性硬规则：只有同时存在热点类别（entity_tick/block_entity/chunk_task 等）与 mod_sources 来源帧时，才能把该来源写成强嫌疑；普通 sampler 不能锁定单个方块实体或实体实例。",
        "证据一致性硬规则：entity_chunks 里的具体实体 ID 不能自动等于 CPU 成因。只有 hot_paths 或 mod_sources 中出现同实体类型/类名帧时，才能写该实体类型导致 CPU 热点；否则只能写实体堆积现场线索。",
        "证据一致性硬规则：memory_gc 的聚合统计只能证明 GC 行为异常；没有 GC 日志时间戳和 tick 窗口对齐时，禁止写“GC 加剧/导致 tick 尖峰”，只能写“GC 可能是独立风险或需对齐验证”。",
        "不要把“可能原因”作为最终答案标题；只有工具证据直接支持时才写确定结论。",
      ].join("\n"),
    },
    {
      role: "user",
      content: [
        "开始分析当前 spark 报告。下面是 report_inventory 的结果。",
        inventoryText,
        "不要要求用户手工复制报告数据；你自己决定需要哪些工具结果。",
        `你必须先查完: ${requiredTools.join(", ")}。`,
        "如果需要更多数据，只输出 JSON 工具调用。证据足够后再输出最终 Markdown。",
      ].join("\n"),
    },
  ];

  for (let round = 1; round <= 12; round += 1) {
    const content = await adapter.callAiChat({ config, messages });
    onTrace({ round, role: "assistant", title: "AI", content });

    const toolCall = parseToolCall(content);
    if (!toolCall) {
      const missingTools = requiredTools.filter((tool) => !usedTools.has(tool));
      if (missingTools.length > 0) {
        const forcedTool = missingTools[0];
        const forcedArgs = defaultArgsForTool(forcedTool);
        const forcedResult = executeReportTool(report, forcedTool, forcedArgs);
        const forcedText = JSON.stringify(forcedResult, null, 2);
        usedTools.add(forcedTool);
        updateEvidenceState(evidenceState, forcedTool, forcedResult);
        onTrace({
          round,
          role: "system",
          title: `Premature final blocked`,
          content: `AI 在查完必要工具前尝试收口。强制补查 ${forcedTool}。`,
        });
        onTrace({
          round,
          role: "tool",
          title: `Tool: ${forcedTool}`,
          content: forcedText,
        });
        messages.push({ role: "assistant", content });
        messages.push({
          role: "user",
          content: [
            "你刚才过早输出了最终诊断，当前证据还不完整。",
            `系统已强制补查 TOOL_RESULT ${forcedTool}:`,
            forcedText.slice(0, 18000),
            `还没查完的必要工具: ${requiredTools.filter((tool) => !usedTools.has(tool)).join(", ") || "无"}`,
            "继续。若还缺工具，只输出 JSON 工具调用；查完后再输出最终 Markdown。",
            "最终诊断不要用“可能原因”做主标题，必须给出确定结论或明确证据不足。",
          ].join("\n"),
        });
        continue;
      }

      if (looksContradictoryFinal(content) && round < 10) {
        messages.push({ role: "assistant", content });
        messages.push({
          role: "user",
          content: [
            "你的最终回答出现证据矛盾：一边把某个实体/来源写成直接成因，一边又说 mod_sources 没有解析到该来源，或没有按工具证据区分“强嫌疑”和“现场线索”。",
            "请重新核对已经返回的 mod_sources、entity_chunks、hotspot_groups、diagnostic_hypotheses。",
            "如果 mod_sources 中有对应来源帧，就必须引用这些帧。",
            "如果只有实体命名空间堆积而没有对应来源帧，就不能说它是直接成因，只能说它是实体密集现场线索。",
            "重新输出最终 Markdown，保持 # 结论、# 证据链、# 排除项、# 还不能确定的点、# 立刻执行。",
          ].join("\n"),
        });
        continue;
      }

      if (contradictsResolvedModSources(content, evidenceState) && round < 10) {
        messages.push({ role: "assistant", content });
        messages.push({
          role: "user",
          content: [
            "你的最终回答与工具归因结果冲突。",
            `已解析到非 unknown 来源: ${[...new Set([...evidenceState.hotPathSourceNames, ...evidenceState.modSourcesNames])].join(", ") || "存在，但名称未汇总"}`,
            "禁止写“mod_sources 全部 unknown / 无模组来源可解析帧 / 无法解析任何模组来源”。",
            "如果来源来自 hot_paths.attribution 或 callChains terminalSource，也必须作为性能归因证据引用；不能因为 mod_sources top 汇总没列出就否定它。",
            "你可以写 unknown 占主导、部分顶层 Minecraft/混淆帧无法归因，但必须引用已解析来源帧，并说明其证据强度。",
            "重新输出最终 Markdown，保持 # 结论、# 证据链、# 排除项、# 还不能确定的点、# 立刻执行。",
          ].join("\n"),
        });
        continue;
      }

      if (downplaysHotPathAttribution(content, evidenceState) && round < 10) {
        messages.push({ role: "assistant", content });
        messages.push({
          role: "user",
          content: [
            "你的最终回答弱化了 hot_paths.attribution / callChains 已经解析出的终端来源。",
            `必须作为强候选列出的来源: ${evidenceState.hotPathSourceNames.join(", ") || "存在，但名称未汇总"}`,
            `必须作为具体实体/生物候选列出的对象: ${evidenceState.hotPathEntityCandidates.join(", ") || "无或未匹配"}`,
            "请改写：这些不是“唯一锁定根因”，但它们是当前报告里最具体的性能候选，必须进入 # 结论 或 # 证据链 的优先排查列表。",
            "不要用“mod_sources 未形成一致归因”来否定 hot_paths 的 terminalSource。",
          ].join("\n"),
        });
        continue;
      }

      if (contradictsEntityEvidence(content, evidenceState) && round < 10) {
        messages.push({ role: "assistant", content });
        messages.push({
          role: "user",
          content: [
            "你的最终回答把 entity_chunks 里的实体堆积写成了直接性能成因，但当前 hot_paths/mod_sources 没有同实体类型 CPU 帧。",
            `仅作为现场线索的实体: ${evidenceState.entityChunkNames.join(", ") || "存在，但名称未汇总"}`,
            "请改写：可以写“实体堆积现场线索/需要现场清理复测”，不能写“该实体/该 mod 直接导致 CPU 热点”。",
            "重新输出最终 Markdown，保持 # 结论、# 证据链、# 排除项、# 还不能确定的点、# 立刻执行。",
          ].join("\n"),
        });
        continue;
      }

      if (omitsSelectedHotPathCategory(content, evidenceState) && round < 10) {
        messages.push({ role: "assistant", content });
        messages.push({
          role: "user",
          content: [
            "你的最终回答漏掉了 hot_paths(auto) 自动选中的高占比热点类别。",
            `必须覆盖的类别: ${evidenceState.selectedHotPathCategories.join(", ")}`,
            "请重写并逐项说明这些类别是否有性能压力、对应关键帧是什么、能证明到什么程度。",
            "重新输出最终 Markdown，保持 # 结论、# 证据链、# 排除项、# 还不能确定的点、# 立刻执行。",
          ].join("\n"),
        });
        continue;
      }

      if (contradictsGcCorrelation(content) && round < 10) {
        messages.push({ role: "assistant", content });
        messages.push({
          role: "user",
          content: [
            "你的最终回答把 GC 聚合统计写成了已证实会加剧/导致 tick 尖峰，但当前没有 GC 日志时间戳与 worst_windows 对齐证据。",
            "请改写：GC 只能作为异常风险或待验证项；不能写成已证实的尖峰原因。",
            "重新输出最终 Markdown，保持 # 结论、# 证据链、# 排除项、# 还不能确定的点、# 立刻执行。",
          ].join("\n"),
        });
        continue;
      }

      if (looksLikeWeakFinal(content) && round < 10) {
        messages.push({ role: "assistant", content });
        messages.push({
          role: "user",
          content: [
            "这份回答仍然太像泛化建议，包含过多“可能/风险/建议进一步确认”。",
            "请继续调用最能缩小范围的工具。优先考虑:",
            '- {"tool":"diagnostic_hypotheses","args":{}}',
            '- {"tool":"hot_paths","args":{"category":"auto","limit":64}}',
            '- {"tool":"evidence_gaps","args":{}}',
            '- {"tool":"mod_sources","args":{"limit":24}}',
            '- {"tool":"entity_chunks","args":{"limit":24}}',
            '- {"tool":"entities","args":{}}',
            "如果报告本身无法精确到模组/实体实例，请最终明确写“当前报告无法唯一定位”，不要编造精确结论。",
          ].join("\n"),
        });
        continue;
      }

      return content;
    }

    const toolResult = executeReportTool(report, toolCall.tool, toolCall.args ?? {});
    usedTools.add(toolCall.tool);
    updateEvidenceState(evidenceState, toolCall.tool, toolResult);
    const resultText = JSON.stringify(toolResult, null, 2);
    onTrace({
      round,
      role: "tool",
      title: `Tool: ${toolCall.tool}`,
      content: resultText,
    });

    messages.push({ role: "assistant", content });
    messages.push({
      role: "user",
      content: [
        `TOOL_RESULT ${toolCall.tool}:`,
        resultText.slice(0, 18000),
        `已查工具: ${[...usedTools].join(", ")}`,
        `必要但未查工具: ${requiredTools.filter((tool) => !usedTools.has(tool)).join(", ") || "无"}`,
        "继续。必要工具未查完时只允许输出 JSON 工具调用；查完后如证据足够再输出最终 Markdown。",
        "最终诊断必须区分：确定结论、证据不足、需要补采样。不要用泛泛的“可能原因”。",
      ].join("\n"),
    });
  }

  return "达到最大工具轮数。请缩小问题或增加 max rounds。";
}

function updateEvidenceState(
  evidenceState: {
    modSourcesResolved: boolean;
    modSourcesNames: string[];
    hotPathSourcesResolved: boolean;
    hotPathSourceNames: string[];
    hotPathEntityCandidates: string[];
    entityChunkNames: string[];
    hotPathText: string;
    selectedHotPathCategories: string[];
  },
  tool: string,
  toolResult: unknown,
) {
  if (!toolResult || typeof toolResult !== "object") return;
  const result = toolResult as AnyRecord;
  if (tool === "entity_chunks") {
    const names = [
      ...((Array.isArray(result.topEntityTypes) ? result.topEntityTypes : []) as AnyRecord[]).map((item) => item.name),
      ...((Array.isArray(result.topChunks) ? result.topChunks : []) as AnyRecord[])
        .flatMap((chunk) => Array.isArray(chunk.topEntities) ? chunk.topEntities.map((item: AnyRecord) => item.name) : []),
    ].filter(Boolean).map(String);
    evidenceState.entityChunkNames = [...new Set(names)].slice(0, 32);
    return;
  }
  if (tool === "hot_paths" || tool === "mod_sources") {
    evidenceState.hotPathText = `${evidenceState.hotPathText}\n${JSON.stringify(result).slice(0, 30000)}`.slice(-60000);
    if (tool === "hot_paths" && Array.isArray(result.selectedCategories)) {
      evidenceState.selectedHotPathCategories = [...new Set(result.selectedCategories.map(String))];
    }
    if (tool === "hot_paths") {
      const attributionSources = ((result.attribution?.topSources ?? []) as AnyRecord[])
        .filter((source) => String(source.sourceId ?? "unknown") !== "unknown")
        .filter((source) => !isWrapperSource(source.sourceId, source.sourceName))
        .map((source) => String(source.sourceName ?? source.sourceId ?? ""))
        .filter(Boolean);
      const chainSources = ((result.callChains ?? []) as AnyRecord[])
        .filter((chain) => String(chain.terminalSourceId ?? "unknown") !== "unknown")
        .filter((chain) => !isWrapperSource(chain.terminalSourceId, chain.terminalSourceName))
        .map((chain) => String(chain.terminalSourceName ?? chain.terminalSourceId ?? ""))
        .filter(Boolean);
      const entityCandidates = ((result.attribution?.entityCandidates ?? []) as AnyRecord[])
        .map((candidate) => String(candidate.entityId ?? ""))
        .filter(Boolean);
      evidenceState.hotPathSourceNames = [...new Set([...attributionSources, ...chainSources])].slice(0, 16);
      evidenceState.hotPathEntityCandidates = [...new Set(entityCandidates)].slice(0, 16);
      evidenceState.hotPathSourcesResolved = evidenceState.hotPathSourceNames.length > 0;
    }
  }
  if (tool !== "mod_sources") return;
  const sources = [
    ...((Array.isArray(result.notableSources) ? result.notableSources : []) as AnyRecord[]),
    ...((Array.isArray(result.topSources) ? result.topSources : []) as AnyRecord[]),
  ].filter((source) => String(source.sourceId ?? "unknown") !== "unknown");

  if (Number(result.resolvedSourceCount ?? 0) > 0 || sources.length > 0) {
    evidenceState.modSourcesResolved = true;
    evidenceState.modSourcesNames = [...new Set(
      sources
        .map((source) => String(source.name ?? source.sourceId ?? ""))
        .filter(Boolean),
    )].slice(0, 8);
  }
}

function requiredToolsForReport(report: ReportDocument) {
  if (report.kind === "heap") {
    return ["overview", "environment", "heap", "evidence_gaps", "diagnostic_hypotheses"];
  }
  if (report.kind === "text") {
    return ["overview", "evidence_gaps"];
  }
  return [
    "overview",
    "environment",
    "hotspot_groups",
    "hot_paths",
    "worst_windows",
    "entity_chunks",
    "mod_sources",
    "memory_gc",
    "diagnostic_hypotheses",
    "evidence_gaps",
  ];
}

function defaultArgsForTool(tool: string): AnyRecord {
  if (tool === "hotspots") return { limit: 32 };
  if (tool === "hotspot_groups") return { limit: 24 };
  if (tool === "hot_paths") return { category: "auto", limit: 64 };
  if (tool === "mod_sources") return { limit: 24 };
  if (tool === "time_windows") return { limit: 80 };
  if (tool === "worst_windows") return { limit: 16 };
  if (tool === "entity_chunks") return { limit: 24 };
  if (tool === "heap") return { limit: 40 };
  return {};
}

function looksLikeWeakFinal(content: string) {
  const weakSignals = ["可能原因", "可能", "风险点", "无法排除", "建议进一步", "进一步确认"];
  const weakCount = weakSignals.filter((signal) => content.includes(signal)).length;
  const hasHardConclusion = content.includes("确定结论") || content.includes("当前报告无法唯一定位");
  return weakCount >= 2 && !hasHardConclusion;
}

function looksContradictoryFinal(content: string) {
  const saysDirectCause =
    content.includes("直接成因") ||
    content.includes("直接原因") ||
    content.includes("确定是") ||
    content.includes("就是");
  const deniesSources =
    content.includes("mod_sources 未解析到") ||
    content.includes("没有解析到") ||
    content.includes("未直接归因到") ||
    content.includes("未归因到");
  return saysDirectCause && deniesSources;
}

function contradictsResolvedModSources(
  content: string,
  evidenceState: { modSourcesResolved: boolean; modSourcesNames: string[]; hotPathSourcesResolved: boolean; hotPathSourceNames: string[] },
) {
  if (!evidenceState.modSourcesResolved && !evidenceState.hotPathSourcesResolved) return false;
  const saysAllUnknown =
    content.includes("mod_sources 全部 unknown") ||
    content.includes("全部 unknown") ||
    content.includes("全是 unknown") ||
    content.includes("其余帧均为 unknown") ||
    content.includes("其余帧都是 unknown") ||
    content.includes("所有帧均为 unknown") ||
    content.includes("无模组来源可解析帧") ||
    content.includes("没有模组来源可解析") ||
    content.includes("无法解析任何模组来源") ||
    content.includes("no mod sources") ||
    content.includes("all unknown");
  return saysAllUnknown;
}

function downplaysHotPathAttribution(
  content: string,
  evidenceState: { hotPathSourcesResolved: boolean; hotPathSourceNames: string[]; hotPathEntityCandidates: string[] },
) {
  if (!evidenceState.hotPathSourcesResolved) return false;
  const downplaySignals = [
    "不能把单一模组",
    "不能把这些模组",
    "不构成可排除的唯一成因结论",
    "mod_sources 未对它们形成一致来源归因",
    "mod_sources 没有一致归因",
    "不能作为重点怀疑",
    "不能重点怀疑",
  ];
  const mentionsCandidate = evidenceState.hotPathSourceNames.some((name) => name && content.toLowerCase().includes(name.toLowerCase()));
  return mentionsCandidate && downplaySignals.some((signal) => content.includes(signal));
}

function contradictsEntityEvidence(
  content: string,
  evidenceState: { entityChunkNames: string[]; hotPathText: string },
) {
  const directWords = ["直接成因", "直接原因", "导致", "造成", "元凶", "主因", "罪魁", "确定是"];
  if (!directWords.some((word) => content.includes(word))) return false;
  const evidenceText = evidenceState.hotPathText.toLowerCase();
  for (const name of evidenceState.entityChunkNames) {
    const entityName = String(name);
    if (!entityName.includes(":")) continue;
    const entityId = entityName.split(":").at(-1) ?? entityName;
    const token = entityId.toLowerCase().replace(/[^a-z0-9]/g, "");
    if (token.length < 4) continue;
    if (content.includes(entityName) && !evidenceText.includes(token)) return true;
  }
  return false;
}

function contradictsGcCorrelation(content: string) {
  const mentionsGc = content.includes("GC") || content.includes("G1 Old") || content.includes("Old Generation");
  if (!mentionsGc) return false;
  const causal = ["加剧尖峰", "导致尖峰", "造成尖峰", "解释尖峰", "导致 tick", "造成 tick"];
  return causal.some((word) => content.includes(word));
}

function omitsSelectedHotPathCategory(
  content: string,
  evidenceState: { selectedHotPathCategories: string[] },
) {
  const required = evidenceState.selectedHotPathCategories.filter((category) =>
    ["block_entity", "chunk_task", "entity_tick", "commands", "entity_ai_pathfinding"].includes(category),
  );
  if (!required.length) return false;
  return required.some((category) => !contentMentionsCategory(content, category));
}

function contentMentionsCategory(content: string, category: string) {
  const aliases: Record<string, string[]> = {
    block_entity: ["block_entity", "BlockEntity", "方块实体", "方块实体 tick", "BlockEntityTicker"],
    chunk_task: ["chunk_task", "区块任务", "区块加载", "ChunkMap", "ServerChunkCache"],
    entity_tick: ["entity_tick", "实体 tick", "实体tick", "EntityTickList"],
    commands: ["commands", "命令", "function", "CommandFunction"],
    entity_ai_pathfinding: ["entity_ai_pathfinding", "实体 AI", "寻路", "GoalSelector", "PathNavigation"],
  };
  return (aliases[category] ?? [category]).some((alias) => content.includes(alias));
}

function isWrapperSource(sourceId: unknown, sourceName: unknown) {
  const value = `${String(sourceId ?? "")} ${String(sourceName ?? "")}`.toLowerCase();
  return ["neruina", "observable", "mixin", "minecraft", "unknown"].some((item) => value.includes(item));
}

function parseToolCall(content: string): { tool: string; args?: AnyRecord } | null {
  const trimmed = content.trim();
  const candidates = [
    trimmed,
    trimmed.match(/```(?:json)?\s*([\s\S]*?)```/)?.[1] ?? "",
    trimmed.match(/\{[\s\S]*\}/)?.[0] ?? "",
  ].filter(Boolean);

  for (const candidate of candidates) {
    try {
      const parsed = JSON.parse(candidate) as AnyRecord;
      if (typeof parsed.tool === "string") {
        return { tool: parsed.tool, args: typeof parsed.args === "object" && parsed.args ? parsed.args : {} };
      }
    } catch {
      // try next candidate
    }
  }

  return null;
}
