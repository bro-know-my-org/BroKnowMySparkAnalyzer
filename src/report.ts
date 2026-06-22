import protobuf from "protobufjs";

export type ReportKind = "sampler" | "health" | "heap" | "text";
export type Severity = "critical" | "warning" | "info";
export type AnyRecord = Record<string, any>;

export type Finding = {
  severity: Severity;
  title: string;
  detail: string;
};

export type NamedValue = {
  name: string;
  value: number;
};

export type HeapHotspot = {
  type: string;
  instances: number;
  bytes: number;
};

export type StackHotspot = {
  label: string;
  samples: number;
  percent: number;
  thread: string;
  source?: string;
  className?: string;
  methodName?: string;
  methodDesc?: string;
  lineNumber?: number;
};

type SparkWindow = AnyRecord & {
  id: string;
  tps?: number;
  msptMedian?: number;
  msptMax?: number;
  entities?: number;
  chunks?: number;
  players?: number;
};

export type ReportSummary = {
  title: string;
  platform?: string;
  generatedAt?: string;
  durationSeconds?: number;
  tps1m?: number;
  tps5m?: number;
  tps15m?: number;
  msptMedian?: number;
  msptP95?: number;
  msptMax?: number;
  processCpu1m?: number;
  systemCpu1m?: number;
  heapUsedBytes?: number;
  heapMaxBytes?: number;
  entityCount?: number;
  playerCount?: number;
  gc?: string[];
  worlds?: string[];
  topEntities?: NamedValue[];
  topHeap?: HeapHotspot[];
  topHotspots?: StackHotspot[];
  findings: Finding[];
};

export type ReportDocument = {
  kind: ReportKind;
  source: string;
  raw: AnyRecord;
  summary: ReportSummary;
};

let protobufRoot: protobuf.Root | null = null;
const HOT_PATH_MAX_DEPTH = 64;
const HOT_PATH_ANCHOR_LIMIT = 24;
const HOT_PATH_CALL_CHAIN_LIMIT = 32;
const HOT_PATH_BRANCH_WIDTH = 8;
const HOT_PATH_BEAM_WIDTH = 48;

export async function parseReportBytes(
  bytes: Uint8Array,
  source: string,
  hint = "",
): Promise<ReportDocument> {
  const root = await loadProtobufRoot();
  const candidates = reportCandidatesForHint(hint);
  const errors: string[] = [];

  for (const candidate of candidates) {
    try {
      const type = root.lookupType(candidate.typeName);
      const decoded = type.decode(bytes);
      const raw = type.toObject(decoded, {
        longs: Number,
        enums: String,
        bytes: String,
        defaults: false,
        arrays: true,
        objects: true,
      }) as AnyRecord;
      return {
        kind: candidate.kind,
        source,
        raw,
        summary: summarizeReport(candidate.kind, raw, source),
      };
    } catch (error) {
      errors.push(`${candidate.typeName}: ${String(error)}`);
    }
  }

  throw new Error(`无法按 spark protobuf 解码。${errors.slice(0, 2).join(" | ")}`);
}

export function parseTextReport(text: string, source = "pasted text"): ReportDocument {
  return {
    kind: "text",
    source,
    raw: { text },
    summary: summarizeText(text, source),
  };
}

export function executeReportTool(
  report: ReportDocument,
  tool: string,
  args: AnyRecord = {},
) {
  switch (tool) {
    case "report_inventory":
      return {
        kind: report.kind,
        source: report.source,
        title: report.summary.title,
        availableTools: reportToolDescriptions(),
        availableData: {
          overview: true,
          environment: hasReportEnvironment(report.raw),
          hotspots: Boolean(report.summary.topHotspots?.length),
          hotspotGroups: Boolean(report.summary.topHotspots?.length),
          modSources: hasSourceMaps(report.raw),
          heap: Boolean(report.summary.topHeap?.length),
          memoryGc: Boolean(report.raw.metadata?.platformStatistics?.memory || report.raw.metadata?.systemStatistics?.gc),
          entities: Boolean(report.summary.topEntities?.length || report.summary.entityCount),
          entityChunks: Boolean(report.raw.metadata?.platformStatistics?.world?.worlds?.length),
          timeWindows: Boolean(Object.keys(report.raw.timeWindowStatistics ?? {}).length),
          worstWindows: Boolean(Object.keys(report.raw.timeWindowStatistics ?? {}).length),
          diagnosticHypotheses: true,
          evidenceGaps: true,
          rawField: report.kind !== "text",
        },
      };
    case "environment":
      return summarizeEnvironment(report);
    case "overview":
      return {
        source: report.source,
        kind: report.kind,
        title: report.summary.title,
        platform: report.summary.platform,
        generatedAt: report.summary.generatedAt,
        durationSeconds: report.summary.durationSeconds,
        metrics: compactObject({
          tps1m: report.summary.tps1m,
          tps5m: report.summary.tps5m,
          tps15m: report.summary.tps15m,
          msptMedian: report.summary.msptMedian,
          msptP95: report.summary.msptP95,
          msptMax: report.summary.msptMax,
          processCpu1m: report.summary.processCpu1m,
          systemCpu1m: report.summary.systemCpu1m,
          heapUsed: formatBytes(report.summary.heapUsedBytes),
          heapMax: formatBytes(report.summary.heapMaxBytes),
          playerCount: report.summary.playerCount,
          entityCount: report.summary.entityCount,
        }),
        findings: report.summary.findings,
        gc: report.summary.gc,
      };
    case "hotspots":
      return {
        limit: numberArg(args.limit, 16),
        hotspots: (report.summary.topHotspots ?? []).slice(0, numberArg(args.limit, 16)),
      };
    case "hotspot_groups":
      return summarizeHotspotGroups(report, numberArg(args.limit, 20));
    case "hot_paths":
      return summarizeHotPaths(report, String(args.category ?? "auto"), numberArg(args.limit, 24));
    case "mod_sources":
      return summarizeModSources(report, numberArg(args.limit, 24));
    case "time_windows":
      return {
        windows: Object.entries(report.raw.timeWindowStatistics ?? {})
          .map(([id, value]) => ({ id, ...(value as AnyRecord) }))
          .slice(0, numberArg(args.limit, 50)),
      };
    case "worst_windows":
      return summarizeWorstWindows(report, numberArg(args.limit, 12));
    case "entities":
      return {
        totalEntities: report.summary.entityCount,
        worlds: report.summary.worlds,
        topEntities: report.summary.topEntities,
        worldRawSummary: summarizeWorldRaw(report.raw.metadata?.platformStatistics?.world),
      };
    case "entity_chunks":
      return summarizeEntityChunks(report, numberArg(args.limit, 24));
    case "heap":
      return {
        topHeap: (report.summary.topHeap ?? []).slice(0, numberArg(args.limit, 24)),
      };
    case "memory_gc":
      return summarizeMemoryGc(report);
    case "diagnostic_hypotheses":
      return buildDiagnosticHypotheses(report);
    case "evidence_gaps":
      return buildEvidenceGaps(report);
    case "raw_field":
      return readRawField(report.raw, String(args.path ?? ""), numberArg(args.maxItems, 80));
    default:
      throw new Error(`未知报告工具: ${tool}`);
  }
}

export function reportToolDescriptions() {
  return [
    { name: "overview", args: {}, description: "关键指标、本地阈值发现、GC 摘要" },
    { name: "environment", args: {}, description: "报告内记录的平台、系统、Java/JVM、服务器配置和来源清单" },
    { name: "hotspots", args: { limit: 16 }, description: "CPU profile 热点帧" },
    { name: "hotspot_groups", args: { limit: 20 }, description: "按类别、包名、线程聚合 CPU 热点，降低框架帧噪声" },
    { name: "hot_paths", args: { category: "auto", limit: 24 }, description: "自动选择高占比热点类别并向下展开子路径，定位具体类和功能帧" },
    { name: "mod_sources", args: { limit: 24 }, description: "利用 class_sources/method_sources/metadata.sources 做模组/来源归因" },
    { name: "time_windows", args: { limit: 50 }, description: "spark 时间窗口统计" },
    { name: "worst_windows", args: { limit: 12 }, description: "按 MSPT max/median/TPS 排序的最坏窗口和前后变化" },
    { name: "entities", args: {}, description: "实体总量、实体排行、世界摘要" },
    { name: "entity_chunks", args: { limit: 24 }, description: "按实体数量排序的世界/区块热点和区块内实体类型" },
    { name: "heap", args: { limit: 24 }, description: "heap summary 对象排行" },
    { name: "memory_gc", args: {}, description: "堆/内存池/GC 聚合统计和异常信号" },
    { name: "diagnostic_hypotheses", args: {}, description: "本地规则生成的诊断假设、证据、反证和下一步动作" },
    { name: "evidence_gaps", args: {}, description: "当前报告能/不能证明什么，以及需要补采的报告类型" },
    { name: "raw_field", args: { path: "metadata.platformStatistics", maxItems: 80 }, description: "读取指定 raw 字段" },
  ];
}

async function loadProtobufRoot() {
  if (protobufRoot) {
    return protobufRoot;
  }
  protobufRoot = await protobuf.load(["/proto/spark_sampler.proto", "/proto/spark_heap.proto"]);
  return protobufRoot;
}

function reportCandidatesForHint(hint = "") {
  const lower = hint.toLowerCase();
  const all = [
    { kind: "sampler" as ReportKind, typeName: "spark.SamplerData" },
    { kind: "health" as ReportKind, typeName: "spark.HealthData" },
    { kind: "heap" as ReportKind, typeName: "spark.HeapData" },
  ];
  if (lower.includes("heap") || lower.includes("sparkheap")) return [all[2], all[0], all[1]];
  if (lower.includes("health")) return [all[1], all[0], all[2]];
  return all;
}

function summarizeReport(kind: ReportKind, raw: AnyRecord, source: string): ReportSummary {
  if (kind === "text") {
    return summarizeText(raw.text ?? "", source);
  }

  const metadata = raw.metadata ?? {};
  const platformStats = metadata.platformStatistics ?? {};
  const systemStats = metadata.systemStatistics ?? {};
  const platformMeta = metadata.platformMetadata ?? {};
  const windows = Object.values(raw.timeWindowStatistics ?? {}) as AnyRecord[];
  const latestWindow = windows.at(-1) ?? {};
  const mspt = platformStats.mspt ?? {};
  const tps = platformStats.tps ?? {};
  const heapUsage = platformStats.memory?.heap ?? {};
  const world = platformStats.world ?? {};
  const topEntities = sortNamedValues(world.entityCounts).slice(0, 12);
  const summary: ReportSummary = {
    title: reportTitle(kind, platformMeta),
    platform: formatPlatform(platformMeta),
    generatedAt: formatTimestamp(metadata.generatedTime ?? metadata.endTime ?? metadata.startTime),
    durationSeconds:
      metadata.startTime && metadata.endTime
        ? Math.round((Number(metadata.endTime) - Number(metadata.startTime)) / 1000)
        : latestWindow.duration,
    tps1m: tps.last1m ?? latestWindow.tps,
    tps5m: tps.last5m,
    tps15m: tps.last15m,
    msptMedian: mspt.last1m?.median ?? latestWindow.msptMedian,
    msptP95: mspt.last1m?.percentile95,
    msptMax: mspt.last1m?.max ?? latestWindow.msptMax,
    processCpu1m: systemStats.cpu?.processUsage?.last1m ?? latestWindow.cpuProcess,
    systemCpu1m: systemStats.cpu?.systemUsage?.last1m ?? latestWindow.cpuSystem,
    heapUsedBytes: heapUsage.used,
    heapMaxBytes: heapUsage.max || heapUsage.committed,
    entityCount: world.totalEntities ?? latestWindow.entities,
    playerCount: platformStats.playerCount ?? latestWindow.players,
    gc: summarizeGc(platformStats.gc ?? systemStats.gc),
    worlds: summarizeWorlds(world.worlds),
    topEntities,
    findings: [],
  };

  if (kind === "sampler") summary.topHotspots = collectHotspots(raw).slice(0, 40);
  if (kind === "heap") {
    summary.topHeap = ((raw.entries ?? []) as AnyRecord[])
      .map((entry) => ({
        type: entry.type ?? "unknown",
        instances: Number(entry.instances ?? 0),
        bytes: Number(entry.size ?? 0),
      }))
      .sort((left, right) => right.bytes - left.bytes)
      .slice(0, 40);
  }
  summary.findings = buildFindings(kind, summary, raw);
  return summary;
}

function summarizeText(text: string, source: string): ReportSummary {
  const lowered = text.toLowerCase();
  const findings: Finding[] = [];
  if (lowered.includes("can't keep up") || lowered.includes("overloaded")) {
    findings.push({
      severity: "warning",
      title: "日志出现 tick 落后提示",
      detail: "文本里包含 can't keep up/overloaded 类提示，需要结合 spark profile 找主线程热点。",
    });
  }
  findings.push({
    severity: "info",
    title: "文本输入已载入",
    detail: "文本输入只能支持弱证据分析；建议拖入 .sparkprofile 获得可追溯结论。",
  });
  return { title: `文本报告 - ${source}`, findings };
}

function buildFindings(kind: ReportKind, summary: ReportSummary, raw: AnyRecord) {
  const findings: Finding[] = [];
  const targetMspt = raw.metadata?.platformStatistics?.mspt?.gameMaxIdealMspt ?? 50;
  if (summary.tps1m !== undefined && summary.tps1m < 18) {
    findings.push({
      severity: "critical",
      title: "TPS 明显低于目标",
      detail: `1m TPS 为 ${formatNumber(summary.tps1m)}，优先看主线程热点与 tick 窗口。`,
    });
  }
  if (summary.msptP95 !== undefined && summary.msptP95 > targetMspt) {
    findings.push({
      severity: "warning",
      title: "MSPT P95 超过理想 tick 时长",
      detail: `P95 MSPT 为 ${formatNumber(summary.msptP95)}ms，目标约 ${targetMspt}ms。`,
    });
  }
  if (summary.msptMax !== undefined && summary.msptMax > targetMspt * 2) {
    findings.push({
      severity: "warning",
      title: "存在长 tick 尖峰",
      detail: `最大 MSPT 为 ${formatNumber(summary.msptMax)}ms，需要看热点和时间窗口。`,
    });
  }
  if (summary.processCpu1m !== undefined && summary.processCpu1m > 85) {
    findings.push({
      severity: "warning",
      title: "Java 进程 CPU 压力高",
      detail: `进程 CPU 1m 为 ${formatPercent(summary.processCpu1m)}。`,
    });
  }
  if (summary.heapUsedBytes && summary.heapMaxBytes && summary.heapUsedBytes / summary.heapMaxBytes > 0.85) {
    findings.push({
      severity: "warning",
      title: "堆内存接近上限",
      detail: `堆使用率约 ${formatPercent((summary.heapUsedBytes / summary.heapMaxBytes) * 100)}。`,
    });
  }
  const memoryGc = summarizeMemoryGc({ kind, source: "", raw, summary });
  for (const signal of memoryGc.signals ?? []) {
    findings.push({
      severity: signal.severity,
      title: signal.title,
      detail: signal.detail,
    });
  }
  if (kind === "sampler" && summary.topHotspots?.length) {
    const top = summary.topHotspots[0];
    findings.push({
      severity: top.percent > 25 ? "warning" : "info",
      title: "已识别 CPU 热点",
      detail: `最高热点 ${top.label}，约占 ${formatNumber(top.percent)}%，线程 ${top.thread}。`,
    });
  }
  if (findings.length === 0) {
    findings.push({
      severity: "info",
      title: "未触发明显阈值告警",
      detail: "本地规则没有发现直接越线信号，建议由 AI 调工具做模式判断。",
    });
  }
  return findings;
}

function collectHotspots(raw: AnyRecord) {
  const hotspots: StackHotspot[] = [];
  for (const thread of raw.threads ?? []) {
    const nodes = thread.children ?? [];
    const rootRefs = Array.isArray(thread.childrenRefs) && thread.childrenRefs.length > 0
      ? thread.childrenRefs
      : rootNodeRefs(nodes);
    const threadSamples = maxThreadSamples(thread, nodes, rootRefs);
    for (const rootRef of rootRefs) {
      const node = nodes[Number(rootRef)];
      if (node) visitStackNode(node, nodes, thread.name ?? "unknown", threadSamples, hotspots, 0);
    }
  }

  const byLabel = new Map<string, StackHotspot>();
  for (const hotspot of hotspots) {
    if (isGenericFrame(hotspot.label)) continue;
    const key = `${hotspot.thread}|${hotspot.label}`;
    const existing = byLabel.get(key);
    if (!existing || hotspot.samples > existing.samples) byLabel.set(key, hotspot);
  }
  return [...byLabel.values()].sort((left, right) => right.samples - left.samples);
}

function visitStackNode(
  node: AnyRecord,
  siblings: AnyRecord[],
  thread: string,
  threadSamples: number,
  hotspots: StackHotspot[],
  depth: number,
) {
  const samples = sumTimes(node.times);
  hotspots.push({
    label: formatStackLabel(node),
    samples,
    percent: threadSamples > 0 ? (samples / threadSamples) * 100 : 0,
    thread,
    source: node.className,
    className: node.className,
    methodName: node.methodName,
    methodDesc: node.methodDesc,
    lineNumber: node.lineNumber,
  });
  if (depth > 40) return;
  for (const childRef of node.childrenRefs ?? []) {
    const child = siblings[Number(childRef)];
    if (child) visitStackNode(child, siblings, thread, threadSamples, hotspots, depth + 1);
  }
}

function rootNodeRefs(nodes: AnyRecord[]) {
  const referenced = new Set<number>();
  nodes.forEach((node) => {
    for (const childRef of node.childrenRefs ?? []) referenced.add(Number(childRef));
  });
  const roots = nodes.map((_, index) => index).filter((index) => !referenced.has(index));
  return roots.length ? roots : nodes.map((_, index) => index);
}

function isGenericFrame(label: string) {
  if (isMinecraftLoopFrame(label)) return true;
  return [
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
  ].some((pattern) => label.includes(pattern));
}

function isMinecraftLoopFrame(label: string) {
  return [
    /^net\.minecraft\.server\.MinecraftServer\$\$Lambda/,
    /^net\.minecraft\.server\.level\.ServerLevel\$\$Lambda/,
    /^net\.minecraft\.server\.MinecraftServer\.m_(206580|130011|5705|5703)_/,
    /^net\.minecraft\.server\.dedicated\.DedicatedServer\.m_5703_/,
    /^net\.minecraft\.client\.server\.IntegratedServer\.m_5705_/,
    /^net\.minecraft\.server\.level\.ServerLevel\.m_8793_/,
    /^net\.minecraft\.world\.level\.Level\.m_46653_/,
  ].some((pattern) => pattern.test(label));
}

function readRawField(raw: AnyRecord, path: string, maxItems: number) {
  if (!path) return { error: "path is required" };
  const value = path.split(".").reduce<unknown>((current, part) => {
    if (current && typeof current === "object") return (current as AnyRecord)[part];
    return undefined;
  }, raw);
  return trimForAi(value, maxItems);
}

function summarizeHotspotGroups(report: ReportDocument, limit: number) {
  const hotspots = report.summary.topHotspots ?? [];
  const byCategory = new Map<string, { category: string; samples: number; maxPercent: number; frames: StackHotspot[] }>();
  const byPackage = new Map<string, { package: string; samples: number; maxPercent: number; frames: StackHotspot[] }>();
  const byThread = new Map<string, { thread: string; samples: number; maxPercent: number; frames: StackHotspot[] }>();

  for (const hotspot of hotspots) {
    addGroupedHotspot(byCategory, classifyHotspot(hotspot), "category", hotspot);
    addGroupedHotspot(byPackage, packageKey(hotspot.label), "package", hotspot);
    addGroupedHotspot(byThread, hotspot.thread || "unknown", "thread", hotspot);
  }

  return {
    byCategory: sortCategoryGroups(byCategory).slice(0, limit),
    byPackage: sortGroups(byPackage).slice(0, limit),
    byThread: sortGroups(byThread).slice(0, limit),
    notes: [
      "samples/percent come from sampled stack frames, not exclusive CPU time.",
      "Use categories as leads; confirm with raw hotspots and worst_windows.",
    ],
  };
}

function summarizeHotPaths(report: ReportDocument, category: string, limit: number): AnyRecord {
  if (category === "auto") {
    const groups = summarizeHotspotGroups(report, 32);
    const actionableGroups = (groups.byCategory as AnyRecord[])
      .filter((group) => isActionableHotPathCategory(group.category))
      .sort((left, right) => Number(right.maxPercent ?? 0) - Number(left.maxPercent ?? 0));
    const categories = (actionableGroups.some((group) => Number(group.maxPercent ?? 0) >= 3)
      ? actionableGroups.filter((group) => Number(group.maxPercent ?? 0) >= 3)
      : actionableGroups.slice(0, 1))
      .slice(0, 6)
      .map((group) => group.category);
    const uniqueCategories = [...new Set(categories)];
    const categoryResults: AnyRecord[] = uniqueCategories.map((item) => summarizeHotPaths(report, item, limit));
    return {
      category: "auto",
      selectedCategories: uniqueCategories,
      callChains: categoryResults.flatMap((result: AnyRecord) =>
        ((result.callChains ?? []) as AnyRecord[]).map((chain) => ({ ...chain, category: result.category })),
      ).sort((left: AnyRecord, right: AnyRecord) => Number(right.terminalPercent ?? 0) - Number(left.terminalPercent ?? 0)).slice(0, limit),
      categories: categoryResults,
      frames: categoryResults.flatMap((result: AnyRecord) =>
        ((result.frames ?? []) as AnyRecord[]).map((frame) => ({ ...frame, category: result.category })),
      ).sort((left: AnyRecord, right: AnyRecord) => Number(right.maxPercent ?? 0) - Number(left.maxPercent ?? 0)).slice(0, limit),
      attribution: buildHotPathAttribution(report, categoryResults, limit),
      selectionRule: "Drill actionable hotspot categories with maxPercent >= 3%, sorted by category maxPercent.",
      skippedCategories: (groups.byCategory as AnyRecord[])
        .filter((group) => !uniqueCategories.includes(group.category))
        .slice(0, 8)
        .map((group) => ({
          category: group.category,
          maxPercent: group.maxPercent,
          reason: isActionableHotPathCategory(group.category) ? "below selection limit" : "aggregate or unsupported category",
        })),
    };
  }

  const anchors = collectHotPathAnchors(report.raw, category);
  const classSources = report.raw.classSources ?? {};
  const methodSources = report.raw.methodSources ?? {};
  const lineSources = report.raw.lineSources ?? {};
  const sourceMetadata = report.raw.metadata?.sources ?? {};
  const grouped = new Map<string, {
    label: string;
    className: string;
    methodName: string;
    sourceId: string;
    sourceName: string;
    sourceVersion?: string;
    samples: number;
    maxPercent: number;
    role: string;
  }>();

  for (const anchor of anchors) {
    for (const item of anchor.descendants) {
      const node = item.node;
      const label = formatStackLabel(node);
      if (shouldSkipHotPathFrame(label, category)) continue;
      const className = node.className ?? classNameFromLabel(label);
      const methodName = node.methodName ?? methodNameFromLabel(label);
      const sourceId = resolveSourceId(
        { className, methodName, methodDesc: node.methodDesc, lineNumber: node.lineNumber },
        classSources,
        methodSources,
        lineSources,
      );
      const metadata = sourceMetadata[sourceId] ?? {};
      const key = `${label}|${sourceId}`;
      const samples = sumTimes(node.times);
      const percent = anchor.threadSamples > 0 ? (samples / anchor.threadSamples) * 100 : 0;
      const entry = grouped.get(key) ?? {
        label,
        className,
        methodName,
        sourceId,
        sourceName: metadata.name ?? sourceId,
        sourceVersion: metadata.version,
        samples: 0,
        maxPercent: 0,
        role: hotPathFrameRole(label, className, methodName),
      };
      entry.samples = Math.max(entry.samples, samples);
      entry.maxPercent = Math.max(entry.maxPercent, percent);
      grouped.set(key, entry);
    }
  }

  const frames = [...grouped.values()]
    .sort((left, right) => right.maxPercent - left.maxPercent || right.samples - left.samples)
    .slice(0, limit);
  const callChains = summarizeHotPathCallChains(
    anchors,
    category,
    classSources,
    methodSources,
    lineSources,
    sourceMetadata,
    Math.min(limit, HOT_PATH_CALL_CHAIN_LIMIT),
  );
  const dominantPaths = summarizeDominantFlamePaths(
    anchors,
    classSources,
    methodSources,
    lineSources,
    sourceMetadata,
    Math.min(limit, 24),
  );

  return {
    category,
    anchors: anchors.slice(0, 8).map((anchor) => ({
      thread: anchor.thread,
      label: formatStackLabel(anchor.node),
      samples: sumTimes(anchor.node.times),
      percent: anchor.threadSamples > 0 ? (sumTimes(anchor.node.times) / anchor.threadSamples) * 100 : 0,
    })),
    dominantPaths,
    callChains,
    frames,
    interpretation: hotPathInterpretation(category, frames),
    limitations: [
      "Frames are sampled inclusive stack data, not exact exclusive CPU time.",
      "A sampler can identify entity/block classes and methods on hot paths, but it cannot identify a single entity instance UUID or exact block position unless the report includes matching per-instance context.",
    ],
  };
}

function summarizeModSources(report: ReportDocument, limit: number) {
  const sources = report.raw.metadata?.sources ?? {};
  const classSources = report.raw.classSources ?? {};
  const methodSources = report.raw.methodSources ?? {};
  const lineSources = report.raw.lineSources ?? {};
  type SourceGroup = {
    sourceId: string;
    name: string;
    version?: string;
    samples: number;
    maxPercent: number;
    frames: StackHotspot[];
  };
  const bySource = new Map<string, SourceGroup>();

  for (const hotspot of report.summary.topHotspots ?? []) {
    const className = hotspot.className ?? classNameFromLabel(hotspot.label);
    const methodName = hotspot.methodName ?? methodNameFromLabel(hotspot.label);
    const sourceId = resolveSourceId(
      { className, methodName, methodDesc: hotspot.methodDesc, lineNumber: hotspot.lineNumber },
      classSources,
      methodSources,
      lineSources,
    );
    const metadata = sources[sourceId] ?? {};
    const entry = bySource.get(sourceId) ?? {
      sourceId,
      name: metadata.name ?? sourceId,
      version: metadata.version,
      samples: 0,
      maxPercent: 0,
      frames: [] as StackHotspot[],
    };
    entry.samples += hotspot.samples;
    entry.maxPercent = Math.max(entry.maxPercent, hotspot.percent);
    if (entry.frames.length < 8) entry.frames.push(hotspot);
    bySource.set(sourceId, entry);
  }

  const allSources = [...bySource.values()].sort((left, right) => right.samples - left.samples);
  const unresolvedFrameBucket = allSources.find((source) => source.sourceId === "unknown");
  const resolvedSources = allSources.filter((source) => source.sourceId !== "unknown");
  const notableSources = allSources
    .filter((source) =>
      source.sourceId !== "unknown" &&
      source.maxPercent >= 3,
    )
    .sort((left, right) => right.maxPercent - left.maxPercent || right.samples - left.samples)
    .slice(0, limit);

  return {
    sourceMapAvailable: hasSourceMaps(report.raw),
    sourceCount: Object.keys(sources).length,
    resolvedSourceCount: resolvedSources.length,
    unresolvedHotspotCount: (report.summary.topHotspots ?? [])
      .filter((hotspot) => {
        const className = hotspot.className ?? classNameFromLabel(hotspot.label);
        const methodName = hotspot.methodName ?? methodNameFromLabel(hotspot.label);
        return resolveSourceId(
          { className, methodName, methodDesc: hotspot.methodDesc, lineNumber: hotspot.lineNumber },
          classSources,
          methodSources,
          lineSources,
        ) === "unknown";
      })
      .length,
    verdict: resolvedSources.length
      ? `source map resolved ${resolvedSources.length} non-unknown source groups; do not report mod_sources as all unknown.`
      : "no non-unknown source groups were resolved from current hotspot frames.",
    topSources: resolvedSources.slice(0, limit),
    unresolvedFrameBucket: unresolvedFrameBucket
      ? {
          ...unresolvedFrameBucket,
          name: "unattributed framework/runtime frames",
          note: "These are usually Minecraft/Forge/framework/runtime frames without a mod source mapping. They are not a mod source.",
        }
      : null,
    notableSources,
    unresolvedHotspots: (report.summary.topHotspots ?? [])
      .filter((hotspot) => {
        const className = hotspot.className ?? classNameFromLabel(hotspot.label);
        const methodName = hotspot.methodName ?? methodNameFromLabel(hotspot.label);
        return resolveSourceId(
          { className, methodName, methodDesc: hotspot.methodDesc, lineNumber: hotspot.lineNumber },
          classSources,
          methodSources,
          lineSources,
        ) === "unknown";
      })
      .slice(0, 12),
  };
}

function summarizeWorstWindows(report: ReportDocument, limit: number) {
  const windows = sortedWindows(report.raw.timeWindowStatistics ?? {});
  const enriched: Array<
    SparkWindow & {
      score: number;
      deltas: {
        entitiesFromPrevious?: number;
        chunksFromPrevious?: number;
        playersFromPrevious?: number;
        tpsFromPrevious?: number;
      };
      nextWindow?: Pick<SparkWindow, "id" | "tps" | "msptMedian" | "msptMax" | "entities" | "chunks">;
    }
  > = windows.map((window, index) => {
    const previous = windows[index - 1];
    const next = windows[index + 1];
    return {
      ...window,
      score: window.msptMax ?? window.msptMedian ?? 0,
      deltas: {
        entitiesFromPrevious: delta(window.entities, previous?.entities),
        chunksFromPrevious: delta(window.chunks, previous?.chunks),
        playersFromPrevious: delta(window.players, previous?.players),
        tpsFromPrevious: delta(window.tps, previous?.tps),
      },
      nextWindow: next
        ? {
            id: next.id,
            tps: next.tps,
            msptMedian: next.msptMedian,
            msptMax: next.msptMax,
            entities: next.entities,
            chunks: next.chunks,
          }
        : undefined,
    };
  });

  return {
    worstByMaxMspt: [...enriched]
      .sort((left, right) => Number(right.msptMax ?? 0) - Number(left.msptMax ?? 0))
      .slice(0, limit),
    worstByMedianMspt: [...enriched]
      .sort((left, right) => Number(right.msptMedian ?? 0) - Number(left.msptMedian ?? 0))
      .slice(0, limit),
    lowTpsWindows: [...enriched]
      .sort((left, right) => Number(left.tps ?? 20) - Number(right.tps ?? 20))
      .slice(0, limit),
  };
}

function summarizeEntityChunks(report: ReportDocument, limit: number) {
  const world = report.raw.metadata?.platformStatistics?.world;
  const chunks: AnyRecord[] = [];
  for (const worldEntry of world?.worlds ?? []) {
    for (const region of worldEntry.regions ?? []) {
      for (const chunk of region.chunks ?? []) {
        chunks.push({
          world: worldEntry.name,
          x: chunk.x,
          z: chunk.z,
          totalEntities: chunk.totalEntities ?? 0,
          topEntities: sortNamedValues(chunk.entityCounts).slice(0, 12),
          riskSignals: entityRiskSignals(chunk.entityCounts),
        });
      }
    }
  }

  return {
    totalEntities: world?.totalEntities,
    topEntityTypes: sortNamedValues(world?.entityCounts).slice(0, 20),
    topChunks: chunks
      .sort((left, right) => Number(right.totalEntities) - Number(left.totalEntities))
      .slice(0, limit),
    note: chunks.length
      ? "High entity chunks identify where to inspect in-game; they do not prove CPU cost without matching hotspot frames."
      : "Report does not include per-chunk entity data.",
  };
}

function summarizeMemoryGc(report: ReportDocument) {
  const platformStats = report.raw.metadata?.platformStatistics ?? {};
  const systemStats = report.raw.metadata?.systemStatistics ?? {};
  const memory = platformStats.memory ?? {};
  const heap = memory.heap ?? {};
  const nonHeap = memory.nonHeap ?? {};
  const pools = (memory.pools ?? []).map((pool: AnyRecord) => {
    const usage = pool.usage ?? {};
    const collectionUsage = pool.collectionUsage ?? {};
    return {
      name: pool.name,
      usage: memoryUsageSummary(usage),
      collectionUsage: memoryUsageSummary(collectionUsage),
      signals: memoryPoolSignals(pool.name, usage, collectionUsage),
    };
  });
  const gcCollectors = Object.entries({ ...(systemStats.gc ?? {}), ...(platformStats.gc ?? {}) })
    .map(([name, value]) => {
      const gc = value as AnyRecord;
      const avgTimeMs = Number(gc.avgTime ?? 0);
      const avgFrequencyMs = Number(gc.avgFrequency ?? 0);
      return {
        name,
        total: Number(gc.total ?? 0),
        avgTimeMs,
        avgFrequencyMs,
        avgFrequencySeconds: Number.isFinite(avgFrequencyMs) ? avgFrequencyMs / 1000 : undefined,
        signals: gcSignals(name, gc),
      };
    })
    .sort((left, right) => Number(right.avgTimeMs ?? 0) - Number(left.avgTimeMs ?? 0));
  const signals = [
    ...heapSignals(heap),
    ...pools.flatMap((pool: AnyRecord) => pool.signals),
    ...gcCollectors.flatMap((collector) => collector.signals),
  ];

  return {
    heap: memoryUsageSummary(heap),
    nonHeap: memoryUsageSummary(nonHeap),
    pools,
    gcCollectors,
    signals,
    interpretation: memoryGcInterpretation(signals),
  };
}

function summarizeEnvironment(report: ReportDocument) {
  if (report.kind === "text") {
    return {
      available: false,
      note: "文本输入没有 spark protobuf metadata，无法读取报告内运行环境。",
    };
  }

  const metadata = report.raw.metadata ?? {};
  const platform = metadata.platformMetadata ?? {};
  const system = metadata.systemStatistics ?? {};
  const cpu = system.cpu ?? {};
  const memory = system.memory ?? {};
  const os = system.os ?? {};
  const java = system.java ?? {};
  const jvm = system.jvm ?? {};
  const disk = system.disk ?? {};
  const serverConfigurations = metadata.serverConfigurations ?? {};
  const extraPlatformMetadata = metadata.extraPlatformMetadata ?? {};
  const sources = metadata.sources ?? {};
  const sourceEntries = Object.entries(sources)
    .map(([id, value]) => {
      const source = value as AnyRecord;
      return {
        id,
        name: source.name ?? id,
        version: source.version,
        author: source.author,
        builtin: Boolean(source.builtin),
      };
    })
    .sort((left, right) => {
      if (left.builtin !== right.builtin) return left.builtin ? 1 : -1;
      return String(left.name).localeCompare(String(right.name));
    });

  return {
    available: true,
    source: "spark report metadata",
    platform: compactObject({
      type: platform.type,
      name: platform.name,
      version: platform.version,
      minecraftVersion: platform.minecraftVersion,
      sparkVersion: platform.sparkVersion,
      brand: platform.brand,
    }),
    os: compactObject({
      name: os.name,
      version: os.version,
      arch: os.arch,
    }),
    java: compactObject({
      vendor: java.vendor,
      version: java.version,
      vendorVersion: java.vendorVersion,
      vmArgs: summarizeVmArgs(java.vmArgs),
    }),
    jvm: compactObject({
      name: jvm.name,
      vendor: jvm.vendor,
      version: jvm.version,
    }),
    cpu: compactObject({
      modelName: cpu.modelName,
      threads: cpu.threads,
      processUsage1m: cpu.processUsage?.last1m,
      processUsage15m: cpu.processUsage?.last15m,
      systemUsage1m: cpu.systemUsage?.last1m,
      systemUsage15m: cpu.systemUsage?.last15m,
    }),
    physicalMemory: memoryPoolEnvironment(memory.physical),
    swapMemory: memoryPoolEnvironment(memory.swap),
    disk: compactObject({
      used: disk.used,
      total: disk.total,
      usedFormatted: formatBytes(disk.used),
      totalFormatted: formatBytes(disk.total),
      usedRatio: ratio(Number(disk.used ?? 0), Number(disk.total ?? 0)),
    }),
    uptime: compactObject({
      millis: system.uptime,
      formatted: formatDuration(system.uptime),
    }),
    networkInterfaceCount: Object.keys(system.net ?? {}).length,
    gcCollectors: summarizeGc(system.gc),
    serverConfigurations: topKeyValues(serverConfigurations, 48, 360),
    extraPlatformMetadata: topKeyValues(extraPlatformMetadata, 48, 360),
    sources: {
      count: sourceEntries.length,
      builtinCount: sourceEntries.filter((source) => source.builtin).length,
      externalCount: sourceEntries.filter((source) => !source.builtin).length,
      top: sourceEntries.slice(0, 80),
    },
    interpretation: [
      "这些字段来自 spark 报告 metadata，只能作为运行环境、版本、配置和资源上下文。",
      "它不能单独证明 TPS/MSPT 根因；根因仍需结合 hotspots/hot_paths/mod_sources/time windows/GC 等证据。",
    ],
  };
}

function buildDiagnosticHypotheses(report: ReportDocument) {
  const groups = summarizeHotspotGroups(report, 12);
  const sources = summarizeModSources(report, 200);
  const chunks = summarizeEntityChunks(report, 12);
  const windows = summarizeWorstWindows(report, 6);
  const gaps = buildEvidenceGaps(report);
  const hotPaths = summarizeHotPaths(report, "auto", 64);
  const hotPathAttribution = hotPaths.attribution as AnyRecord | undefined;
  const hypotheses: AnyRecord[] = [];
  const categoryMap = new Map((groups.byCategory as AnyRecord[]).map((entry) => [entry.category, entry]));
  const denseChunk = (chunks.topChunks as AnyRecord[])[0];
  const blockEntityGroup = categoryMap.get("block_entity");
  const entityGroup = categoryMap.get("entity_tick");
  const namespaceStats = summarizeEntityNamespaces(chunks);
  const sourceCandidates = ((sources.notableSources?.length ? sources.notableSources : sources.topSources) as AnyRecord[])
    .filter((source) => source.sourceId !== "unknown")
    .filter(sourceHasServerThreadFrame)
    .slice(0, 32);

  const hotPathSourceCandidates = ((hotPathAttribution?.topSources ?? []) as AnyRecord[])
    .filter((source) => source.sourceId !== "unknown")
    .filter((source) => !isWrapperSource(source.sourceId, source.sourceName))
    .slice(0, 12);
  const hotPathCategoryEvidence = ((hotPathAttribution?.byCategory ?? []) as AnyRecord[])
    .map((entry) => {
      const sourcesByCategory = ((entry.topSources ?? []) as AnyRecord[])
        .filter((source) => !isWrapperSource(source.sourceId, source.sourceName))
        .slice(0, 6);
      const entitiesByCategory = ((entry.entityCandidates ?? []) as AnyRecord[]).slice(0, 8);
      const sourceText = sourcesByCategory.length
        ? sourcesByCategory.map((source) =>
          `${source.sourceName ?? source.sourceId} ${formatPercent(source.maxPercent)} [${((source.terminalFrames ?? []) as AnyRecord[]).slice(0, 2).map((frame) => frame.label).join("; ")}]`,
        ).join(" | ")
        : "无非 wrapper 模组来源";
      const entityText = entitiesByCategory.length
        ? ` entityCandidates: ${entitiesByCategory.map((candidate) => `${candidate.entityId} ${formatPercent(candidate.percent)}`).join(", ")}`
        : "";
      const dominantText = ((entry.dominantPaths ?? []) as AnyRecord[]).slice(0, 3).map((path) =>
        `${((path.frames ?? []) as AnyRecord[]).map((frame) => `${frame.sourceName ?? frame.sourceId}:${frame.label} ${formatPercent(frame.percent)}`).join(" -> ")}`,
      ).join(" || ");
      return `${entry.category}: terminalSources: ${sourceText}${entityText}${dominantText ? ` dominantPaths: ${dominantText}` : ""}`;
    });
  if (hotPathSourceCandidates.length || hotPathCategoryEvidence.length) {
    hypotheses.push({
      id: "hot_path_terminal_sources",
      confidence: confidenceFromEvidence([
        hotPathSourceCandidates.some((source) => Number(source.maxPercent ?? 0) >= 1) || hotPathCategoryEvidence.length > 0,
        Boolean(entityGroup) || Boolean(blockEntityGroup) || Boolean(categoryMap.get("chunk_task")),
        hotPathSourceCandidates.some((source) => Array.isArray(source.terminalFrames) && source.terminalFrames.length > 0),
      ]),
      conclusion: "hot_paths 已按高占用类别下钻到具体终端模组/类；每个 selected category 都必须独立看",
      evidence: [
        `selectedCategories: ${((hotPaths.selectedCategories ?? []) as string[]).join(", ")}`,
        ...hotPathCategoryEvidence,
        ...hotPathSourceCandidates.slice(0, 8).map((source) =>
          `global: ${source.sourceName ?? source.sourceId} max ${formatPercent(source.maxPercent)} categories ${(source.categories ?? []).join(", ")} frames: ${((source.terminalFrames ?? []) as AnyRecord[]).slice(0, 3).map((frame) => `${frame.label} ${formatPercent(frame.percent)}`).join("; ")}`,
        ),
      ],
      limitations: [
        "这些来源来自 hot_paths terminal frames，是性能路径候选；不要求 mod_sources 再次汇总到同一来源才成立。",
        "全局排序不能替代逐类别下钻；entity_tick、chunk_task、block_entity 等高占用类别必须分别解释。",
        "普通 sampler 仍不能证明单个实例或单个坐标；但这些模组/类应优先检查和复测。",
      ],
      nextActions: [
        "按 hot_paths terminal source 优先做 A/B 复测或配置隔离，而不是先怀疑 Neruina/Observable 包装层。",
        "对 entity_tick 终端实体类，结合 entity_chunks 和 only-ticks-over 捕获确认具体场景。",
      ],
    });
  }

  const entityCandidates = ((hotPathAttribution?.entityCandidates ?? []) as AnyRecord[])
    .filter((candidate) => candidate.entityId)
    .slice(0, 16);
  if (entityCandidates.length) {
    hypotheses.push({
      id: "hot_path_entity_candidates",
      confidence: confidenceFromEvidence([
        entityCandidates.some((candidate) => candidate.confidence === "high"),
        entityCandidates.some((candidate) => Number(candidate.percent ?? 0) >= 0.5),
        Boolean(entityGroup),
      ]),
      conclusion: "hot_paths 已把部分实体 tick 终端帧匹配到具体实体/生物候选",
      evidence: entityCandidates.slice(0, 8).map((candidate) =>
        `${candidate.entityId} via ${candidate.sourceName ?? candidate.sourceId}:${candidate.label} ${formatPercent(candidate.percent)} (${candidate.confidence}, ${candidate.reason})`,
      ),
      limitations: [
        "这是实体类型/类级别归因，不是单个实体 UUID。",
        "如果 entity_chunks 中没有对应密集现场，也仍可能是少量高耗实体逻辑；需要 only-ticks-over 复测。",
      ],
      nextActions: [
        "优先在服务器里定位这些实体类型出现的位置，减少/隔离后重采 profile。",
        "若无法复现，采 /spark profiler --only-ticks-over 50 --timeout 120 捕捉尖峰级实体调用链。",
      ],
    });
  }

  for (const source of sourceCandidates) {
    const namespace = bestNamespaceMatch(source, namespaceStats);
    const matchedChunks = namespace ? namespaceStats.get(namespace)?.chunks ?? [] : [];
    const hasSourceHotspot = Number(source.maxPercent ?? 0) >= 5;
    const hasEntityNamespace = matchedChunks.length > 0;
    const entityFrameMatches = matchingEntityFramesForSource(source, matchedChunks);
    const hasEntityCpuEvidence = entityFrameMatches.length > 0;
    if (!hasSourceHotspot) continue;

    hypotheses.push({
      id: `mod_source_hotspot:${source.sourceId}`,
      confidence: confidenceFromEvidence([
        hasSourceHotspot,
        Boolean(blockEntityGroup) || Boolean(entityGroup),
        hasEntityCpuEvidence,
      ]),
      conclusion: `${source.name ?? source.sourceId} 有可引用的采样热点；实体分布若仅同命名空间，只能作为现场线索`,
      evidence: [
        `mod_sources: ${source.name ?? source.sourceId} ${source.version ?? ""} max ${formatPercent(source.maxPercent)}, frames: ${frameLabels(source.frames).join("; ")}`,
        blockEntityGroup ? `block_entity 类热点最高约 ${formatPercent(blockEntityGroup.maxPercent)}` : "未看到 block_entity 类聚合热点",
        entityGroup ? `entity_tick 类热点最高约 ${formatPercent(entityGroup.maxPercent)}` : "未看到 entity_tick 类聚合热点",
        hasEntityCpuEvidence
          ? `同实体类型 CPU 帧: ${entityFrameMatches.slice(0, 4).join("; ")}`
          : "未看到能绑定到同实体类型的 CPU 帧",
        hasEntityNamespace
          ? `同命名空间实体区块（现场线索，不是 CPU 证据）: ${matchedChunks.slice(0, 3).map(formatChunkEvidence).join(" | ")}`
          : "未看到同命名空间实体区块",
      ],
      limitations: [
        "当前报告能证明这些来源帧参与采样热点；普通 sampler 仍不能锁定单个方块实体或实体实例。",
        "entity_chunks 只能证明某区块/实体类型堆积；除非 hot_paths/mod_sources 里出现同实体类型帧，否则不能写该实体类型是直接 CPU 成因。",
      ],
      nextActions: [
        "优先检查已采样到的具体热点类/方法对应的机器或系统，清理实体密集区块后也要复测对比。",
        "若要锁定到具体机器，站在该区块附近捕获 /spark profiler --only-ticks-over 50 --timeout 120。",
      ],
    });
  }

  if (blockEntityGroup && Number(blockEntityGroup.maxPercent ?? 0) >= 10) {
    const blockPaths = summarizeHotPaths(report, "block_entity", 12);
    const frames = (blockPaths.frames ?? []) as AnyRecord[];
    const callChains = (blockPaths.callChains ?? []) as AnyRecord[];
    const resolvedFrames = frames.filter((frame) => frame.sourceId !== "unknown");
    const blockEntitySourceFrames = ((sources.notableSources ?? sources.topSources ?? []) as AnyRecord[])
      .flatMap((source) => ((source.frames ?? []) as StackHotspot[]).map((frame) => ({ source, frame })))
      .filter(({ frame }) => classifyFrame(frame.label) === "block_entity" && isServerThreadName(frame.thread))
      .slice(0, 8);
    hypotheses.push({
      id: "block_entity_hot_path",
      confidence: confidenceFromEvidence([
        Number(blockEntityGroup.maxPercent ?? 0) >= 25,
        frames.length > 0,
        resolvedFrames.length > 0,
      ]),
      conclusion: "方块实体 tick 是主线程热点路径，必须进入结论",
      evidence: [
        `block_entity 类热点最高约 ${formatPercent(blockEntityGroup.maxPercent)}`,
        `hot_paths(block_entity): ${frames.slice(0, 8).map((frame) => `${frame.sourceName ?? frame.sourceId}:${frame.label} ${formatPercent(frame.maxPercent)}`).join("; ")}`,
        callChains.length
          ? `hot_paths(block_entity) 调用链: ${callChains.slice(0, 4).map((chain) => `${((chain.path ?? []) as AnyRecord[]).map((entry) => `${entry.sourceName}:${entry.label}`).join(" -> ")} (${formatPercent(chain.terminalPercent)})`).join(" | ")}`
          : "hot_paths(block_entity) 未返回调用链",
        blockEntitySourceFrames.length
          ? `mod_sources 中的方块实体帧: ${blockEntitySourceFrames.map(({ source, frame }) => `${source.name ?? source.sourceId}:${frame.label} ${formatPercent(frame.percent)}`).join("; ")}`
          : "mod_sources 未列出方块实体来源帧",
        resolvedFrames.length
          ? `解析到来源帧: ${resolvedFrames.slice(0, 6).map((frame) => `${frame.sourceName}:${frame.label}`).join("; ")}`
          : "block_entity 子路径未解析到具体模组来源帧",
      ],
      limitations: [
        "这能证明方块实体 tick 路径有采样热点，但普通 sampler 不能直接给出具体方块坐标。",
        "需要在热点类对应机器附近捕获 only-ticks-over 或结合现场坐标来定位具体机器组。",
      ],
      nextActions: [
        "优先检查 hot_paths 中出现的方块实体类型和机器链路。",
        "在疑似机器区附近采 /spark profiler --only-ticks-over 50 --timeout 120 来锁定具体 tick 尖峰。",
      ],
    });
  }

  if (denseChunk && Number(denseChunk.totalEntities ?? 0) >= 50) {
    hypotheses.push({
      id: "high_density_entity_chunk",
      confidence: confidenceFromEvidence([
        Number(denseChunk.totalEntities ?? 0) >= 80,
        Boolean(categoryMap.get("entity_tick")),
        Boolean((denseChunk.riskSignals as string[] | undefined)?.length),
      ]),
      conclusion: "存在明确的实体密集区块，必须作为首批现场检查点",
      evidence: [
        `Top chunk: ${formatChunkEvidence(denseChunk)}`,
        `Risk signals: ${(denseChunk.riskSignals ?? []).join(", ") || "none"}`,
        `全局实体总数 ${report.summary.entityCount ?? "unknown"}`,
      ],
      limitations: [
        "实体密度能定位现场，但不能单独证明 CPU 独占耗时；需要 hot_paths/mod_sources 出现同实体类型帧，或 only-ticks-over 对齐。",
      ],
      nextActions: [
        "传送/定位到该 chunk，先清理异常堆积实体，再采一次相同 profile 对比 TPS/MSPT。",
      ],
    });
  }

  if (entityGroup) {
    hypotheses.push({
      id: "entity_tick_load",
      confidence: confidenceFromEvidence([entityGroup.maxPercent > 15, Boolean(chunks.topEntityTypes?.length)]),
      conclusion: "主线程实体 tick 是重要负载来源",
      evidence: [
        `entity_tick 类热点最高约 ${formatPercent(entityGroup.maxPercent)}`,
        `实体总量 ${report.summary.entityCount ?? "unknown"}`,
        `Top entity types: ${(chunks.topEntityTypes ?? []).slice(0, 6).map((item: NamedValue) => `${item.name}=${item.value}`).join(", ")}`,
      ],
      limitations: [
        "spark profile cannot identify a single entity instance unless per-chunk/entity context aligns with hotspot frames.",
      ],
      nextActions: [
        "Use entity_chunks to inspect top chunks in-game.",
        "Capture /spark profiler --only-ticks-over 50 while standing near suspected chunks.",
      ],
    });
  }

  const chunkGroup = categoryMap.get("chunk_task");
  const worst = (windows.worstByMaxMspt as AnyRecord[])[0];
  if (chunkGroup || Number(worst?.deltas?.chunksFromPrevious ?? 0) > 500) {
    hypotheses.push({
      id: "chunk_task_or_generation_spike",
      confidence: confidenceFromEvidence([Boolean(chunkGroup), Number(worst?.msptMax ?? 0) > 200, Number(worst?.deltas?.chunksFromPrevious ?? 0) > 300]),
      conclusion: "卡顿尖峰与 chunk 任务/加载/生成相关",
      evidence: [
        chunkGroup ? `chunk_task 类热点最高约 ${formatPercent(chunkGroup.maxPercent)}` : "没有明确 chunk_task 热点，但窗口 chunk 数变化需要关注",
        worst ? `最坏窗口 ${worst.id}: max MSPT ${formatNumber(worst.msptMax)}, chunks delta ${worst.deltas?.chunksFromPrevious ?? "-"}` : "无窗口数据",
      ],
      limitations: [
        "time window is coarse; it cannot bind a specific chunk task to an exact stack sample.",
      ],
      nextActions: [
        "Lower view-distance/simulation-distance or pregen suspected areas, then compare worst_windows.",
        "Capture only-ticks-over report during exploration/worldgen.",
      ],
    });
  }

  const c2meSource = (sources.topSources as AnyRecord[]).find((entry) => String(entry.sourceId).includes("c2me"));
  if (c2meSource && chunkGroup) {
    hypotheses.push({
      id: "c2me_chunk_io_path",
      confidence: confidenceFromEvidence([
        Number(c2meSource.maxPercent ?? 0) > 3,
        Number(chunkGroup.maxPercent ?? 0) > 8,
        Number(worst?.msptMax ?? 0) > 200,
      ]),
      conclusion: "尖峰与 C2ME/chunk IO 主线程任务路径相关，但当前报告不能唯一锁定触发区块",
      evidence: [
        `mod_sources: ${c2meSource.sourceId} ${c2meSource.version ?? ""} max ${formatPercent(c2meSource.maxPercent)}`,
        `chunk_task 类热点最高约 ${formatPercent(chunkGroup.maxPercent)}`,
        worst ? `最坏窗口 ${worst.id}: max MSPT ${formatNumber(worst.msptMax)}, chunks ${worst.chunks ?? "-"}` : "无窗口数据",
      ],
      limitations: [
        "普通 sampler 只显示采样期间有 chunk IO/任务等待，不能把 4s 级单次尖峰绑定到具体 chunk。",
      ],
      nextActions: [
        "在复现移动/加载区域时捕获 /spark profiler --only-ticks-over 50 --timeout 120。",
        "同时记录玩家坐标和移动路线，和 worst_windows 时间段对齐。",
      ],
    });
  }

  const memoryGc = summarizeMemoryGc(report);
  const severeGcSignals = (memoryGc.signals ?? []).filter((signal: AnyRecord) => signal.category === "gc" && signal.severity !== "info");
  const severeMemorySignals = (memoryGc.signals ?? []).filter((signal: AnyRecord) => signal.category === "memory" && signal.severity !== "info");
  if (severeGcSignals.length || severeMemorySignals.length) {
    hypotheses.push({
      id: "memory_gc_pressure",
      confidence: confidenceFromEvidence([
        severeGcSignals.some((signal: AnyRecord) => signal.severity === "critical"),
        severeGcSignals.length > 0,
        severeMemorySignals.length > 0,
      ]),
      conclusion: severeGcSignals.length
        ? "GC 暂停/频率存在异常信号，可能解释无明显 CPU 热点时的卡顿尖峰"
        : "内存池压力存在异常信号，但需要结合 GC/时间窗口判断影响",
      evidence: [
        `heap ${memoryGc.heap.usedFormatted} / ${memoryGc.heap.maxFormatted} (${formatPercent(memoryGc.heap.usedMaxRatio * 100)})`,
        ...(memoryGc.gcCollectors as AnyRecord[]).slice(0, 4).map((gc) =>
          `${gc.name}: total ${gc.total}, avg ${formatNumber(gc.avgTimeMs)}ms, every ${formatNumber(gc.avgFrequencySeconds)}s`,
        ),
        ...(memoryGc.signals ?? []).slice(0, 4).map((signal: AnyRecord) => `${signal.title}: ${signal.detail}`),
      ],
      limitations: [
        "spark 聚合 GC 统计能证明 GC 行为异常，但不能把某一次 tick 尖峰精确绑定到某一次 STW 暂停。",
      ],
      nextActions: [
        "用 JVM -Xlog:gc*:file=gc.log:time 采集 GC 日志，并和 spark worst_windows 的时间段对齐。",
        "如果 Old/Full GC 平均暂停高，优先检查堆配置、对象 churn、区块/实体加载导致的分配峰值。",
      ],
    });
  }

  const gcOrMemory = report.summary.heapUsedBytes && report.summary.heapMaxBytes
    ? report.summary.heapUsedBytes / report.summary.heapMaxBytes
    : 0;
  if (!severeGcSignals.length && (gcOrMemory > 0.75 || gaps.missingEvidence.includes("GC pause log"))) {
    hypotheses.push({
      id: "gc_pause_possible_but_unproven",
      confidence: gcOrMemory > 0.85 ? "medium" : "low",
      conclusion: "GC/内存停顿不能由当前报告证明",
      evidence: [
        `heap usage ${formatBytes(report.summary.heapUsedBytes)} / ${formatBytes(report.summary.heapMaxBytes)}`,
        `GC data present: ${report.summary.gc?.length ? "yes" : "no"}`,
      ],
      limitations: [
        "Current spark profile does not include per-pause GC log correlation.",
      ],
      nextActions: [
        "Collect spark health report with GC section or JVM -Xlog:gc* log around the spike.",
      ],
    });
  }

  const sortedHypotheses = hypotheses.sort((left, right) => confidenceRank(right.confidence) - confidenceRank(left.confidence));
  return {
    hypotheses: sortedHypotheses,
    strongest: sortedHypotheses[0] ?? null,
    evidenceGaps: gaps,
  };
}

function buildEvidenceGaps(report: ReportDocument) {
  const missingEvidence: string[] = [];
  const weakEvidence: string[] = [];
  const availableEvidence: string[] = [];

  if (report.summary.topHotspots?.length) availableEvidence.push("sampled CPU hotspot tree");
  else missingEvidence.push("CPU hotspot tree");

  if (Object.keys(report.raw.timeWindowStatistics ?? {}).length) availableEvidence.push("time window statistics");
  else missingEvidence.push("time window statistics");

  if (report.raw.metadata?.platformStatistics?.world?.worlds?.length) availableEvidence.push("world/entity/chunk statistics");
  else missingEvidence.push("world/entity/chunk statistics");

  if (hasSourceMaps(report.raw)) {
    availableEvidence.push("class/method/line source map");
  } else {
    weakEvidence.push("mod attribution is weak because class_sources/method_sources/line_sources are absent");
  }

  if (report.summary.gc?.length) {
    availableEvidence.push("GC aggregate statistics");
    weakEvidence.push("GC aggregate statistics are not timestamped; exact spike correlation requires GC logs");
  } else {
    missingEvidence.push("GC pause log");
  }

  if (report.kind === "sampler") {
    weakEvidence.push("ordinary sampler cannot always isolate one tick spike; --only-ticks-over reports are stronger for spikes");
  }

  return {
    canProve: [
      "Whether TPS/MSPT were degraded during the captured interval.",
      "Which stack-frame categories dominated sampled server-thread time.",
      "Which entity types/chunks were numerically heavy if world stats are present.",
    ],
    cannotProveAlone: [
      "The exact entity instance, block entity, or chunk coordinate that caused one spike unless tool data aligns clearly.",
      "GC stop-the-world pauses without GC logs or health GC data.",
      "A mod source when class/method source maps are absent or obfuscated.",
    ],
    availableEvidence,
    weakEvidence,
    missingEvidence,
    recommendedNextCaptures: [
      "/spark profiler --only-ticks-over 50 --timeout 120",
      "/spark healthreport --memory --network --upload",
      "GC log around the spike if heap pressure is suspected",
    ],
  };
}

function trimForAi(value: unknown, maxItems: number): unknown {
  if (Array.isArray(value)) return value.slice(0, maxItems).map((item) => trimForAi(item, maxItems));
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value as AnyRecord)
        .slice(0, maxItems)
        .map(([key, item]) => [key, trimForAi(item, maxItems)]),
    );
  }
  return value;
}

function summarizeWorldRaw(world: AnyRecord | undefined) {
  if (!world) return undefined;
  return {
    totalEntities: world.totalEntities,
    topEntityCounts: sortNamedValues(world.entityCounts).slice(0, 20),
    worlds: summarizeWorlds(world.worlds),
  };
}

function addGroupedHotspot<T extends string>(
  map: Map<string, Record<T, string> & { samples: number; maxPercent: number; frames: StackHotspot[] }>,
  key: string,
  field: T,
  hotspot: StackHotspot,
) {
  const entry = map.get(key) ?? {
    [field]: key,
    samples: 0,
    maxPercent: 0,
    frames: [],
  } as Record<T, string> & { samples: number; maxPercent: number; frames: StackHotspot[] };
  entry.samples += hotspot.samples;
  entry.maxPercent = Math.max(entry.maxPercent, hotspot.percent);
  if (entry.frames.length < 8) entry.frames.push(hotspot);
  map.set(key, entry);
}

function sortGroups<T>(groups: Map<string, T & { samples: number; maxPercent: number; frames: StackHotspot[] }>) {
  return [...groups.values()]
    .map((entry) => ({
      ...entry,
      frames: entry.frames.slice(0, 6),
    }))
    .sort((left, right) => right.samples - left.samples);
}

function sortCategoryGroups(groups: Map<string, { category: string; samples: number; maxPercent: number; frames: StackHotspot[] }>) {
  return sortGroups(groups).sort((left, right) => {
    if (left.category === "other" && right.category !== "other") return 1;
    if (right.category === "other" && left.category !== "other") return -1;
    return right.samples - left.samples;
  });
}

function collectHotPathAnchors(raw: AnyRecord, category: string) {
  const anchors: Array<{
    thread: string;
    index: number;
    node: AnyRecord;
    nodes: AnyRecord[];
    threadSamples: number;
    descendants: Array<{ node: AnyRecord; depth: number }>;
  }> = [];

  for (const thread of raw.threads ?? []) {
    if (isServerThreadHotPathCategory(category) && !isServerThreadName(thread.name ?? "")) continue;
    const nodes = thread.children ?? [];
    const rootRefs = Array.isArray(thread.childrenRefs) && thread.childrenRefs.length > 0
      ? thread.childrenRefs
      : rootNodeRefs(nodes);
    const threadSamples = maxThreadSamples(thread, nodes, rootRefs);
    const seen = new Set<number>();
    for (const rootRef of rootRefs) {
      collectHotPathAnchorsFromNode(
        nodes,
        Number(rootRef),
        thread.name ?? "unknown",
        threadSamples,
        category,
        anchors,
        seen,
      );
    }
  }

  return anchors.sort((left, right) => sumTimes(right.node.times) - sumTimes(left.node.times));
}

function isActionableHotPathCategory(category: string) {
  return [
    "entity_tick",
    "entity_ai_pathfinding",
    "chunk_task",
    "block_entity",
    "commands",
    "io",
  ].includes(category);
}

function isServerThreadHotPathCategory(category: string) {
  return ["entity_tick", "entity_ai_pathfinding", "chunk_task", "block_entity", "commands", "world_tick"].includes(category);
}

function collectHotPathAnchorsFromNode(
  nodes: AnyRecord[],
  index: number,
  thread: string,
  threadSamples: number,
  category: string,
  anchors: Array<{
    thread: string;
    index: number;
    node: AnyRecord;
    nodes: AnyRecord[];
    threadSamples: number;
    descendants: Array<{ node: AnyRecord; depth: number }>;
  }>,
  seen: Set<number>,
) {
  const node = nodes[index];
  if (!node || seen.has(index)) return;
  seen.add(index);

  const label = formatStackLabel(node);
  if (frameMatchesHotPathCategory(label, category)) {
    anchors.push({
      thread,
      index,
      node,
      nodes,
      threadSamples,
      descendants: collectDescendants(nodes, index),
    });
    return;
  }

  for (const childRef of node.childrenRefs ?? []) {
    collectHotPathAnchorsFromNode(nodes, Number(childRef), thread, threadSamples, category, anchors, seen);
  }
}

function collectDescendants(nodes: AnyRecord[], index: number, depth = 0, seen = new Set<number>()) {
  const node = nodes[index];
  if (!node || seen.has(index) || depth > HOT_PATH_MAX_DEPTH) return [] as Array<{ node: AnyRecord; depth: number }>;
  seen.add(index);
  const out: Array<{ node: AnyRecord; depth: number }> = [{ node, depth }];
  for (const childRef of node.childrenRefs ?? []) {
    out.push(...collectDescendants(nodes, Number(childRef), depth + 1, seen));
  }
  return out;
}

function summarizeHotPathCallChains(
  anchors: Array<{
    thread: string;
    index: number;
    node: AnyRecord;
    nodes: AnyRecord[];
    threadSamples: number;
  }>,
  category: string,
  classSources: AnyRecord,
  methodSources: AnyRecord,
  lineSources: AnyRecord,
  sourceMetadata: AnyRecord,
  limit: number,
) {
  const chains: AnyRecord[] = [];
  for (const anchor of anchors.slice(0, HOT_PATH_ANCHOR_LIMIT)) {
    collectHotPathCallChainsFromNode(
      anchor,
      anchor.index,
      [],
      category,
      classSources,
      methodSources,
      lineSources,
      sourceMetadata,
      chains,
      new Set<number>(),
      0,
    );
  }

  const byKey = new Map<string, AnyRecord>();
  for (const chain of chains) {
    const key = chain.path.map((entry: AnyRecord) => entry.label).join(" > ");
    const existing = byKey.get(key);
    if (!existing || chain.terminalPercent > existing.terminalPercent) byKey.set(key, chain);
  }

  return [...byKey.values()]
    .sort((left, right) => Number(right.terminalPercent ?? 0) - Number(left.terminalPercent ?? 0))
    .slice(0, limit);
}

function collectHotPathCallChainsFromNode(
  anchor: {
    thread: string;
    nodes: AnyRecord[];
    threadSamples: number;
  },
  index: number,
  path: AnyRecord[],
  category: string,
  classSources: AnyRecord,
  methodSources: AnyRecord,
  lineSources: AnyRecord,
  sourceMetadata: AnyRecord,
  chains: AnyRecord[],
  seen: Set<number>,
  depth: number,
) {
  const node = anchor.nodes[index];
  if (!node || seen.has(index) || depth > HOT_PATH_MAX_DEPTH) return;
  seen.add(index);
  const nextPath = [...path, node];
  const label = formatStackLabel(node);
  const sourceId = resolveSourceId(
    { className: node.className, methodName: node.methodName, methodDesc: node.methodDesc, lineNumber: node.lineNumber },
    classSources,
    methodSources,
    lineSources,
  );
  const childRefs = (node.childrenRefs ?? []) as unknown[];
  const isCandidate = isConcreteHotPathFrame(label, sourceId, category);

  if (isCandidate) {
    const samples = sumTimes(node.times);
    const terminalPercent = anchor.threadSamples > 0 ? (samples / anchor.threadSamples) * 100 : 0;
    const compactPath = compactCallChainPath(
      nextPath,
      classSources,
      methodSources,
      lineSources,
      sourceMetadata,
      anchor.threadSamples,
    );
    const source = sourceMetadata[sourceId] ?? {};
    chains.push({
      terminalLabel: label,
      terminalSourceId: sourceId,
      terminalSourceName: source.name ?? sourceId,
      terminalPercent,
      samples,
      thread: anchor.thread,
      path: compactPath,
      interpretation: interpretCallChain(compactPath),
    });
  }

  for (const childRef of childRefs) {
    collectHotPathCallChainsFromNode(
      anchor,
      Number(childRef),
      nextPath,
      category,
      classSources,
      methodSources,
      lineSources,
      sourceMetadata,
      chains,
      new Set(seen),
      depth + 1,
    );
  }
}

function compactCallChainPath(
  path: AnyRecord[],
  classSources: AnyRecord,
  methodSources: AnyRecord,
  lineSources: AnyRecord,
  sourceMetadata: AnyRecord,
  threadSamples: number,
) {
  const entries = path
    .map((node, index) => {
      const label = formatStackLabel(node);
      const sourceId = resolveSourceId(
        { className: node.className, methodName: node.methodName, methodDesc: node.methodDesc, lineNumber: node.lineNumber },
        classSources,
        methodSources,
        lineSources,
      );
      const source = sourceMetadata[sourceId] ?? {};
      const samples = sumTimes(node.times);
      return {
        label,
        sourceId,
        sourceName: source.name ?? sourceId,
        percent: threadSamples > 0 ? (samples / threadSamples) * 100 : 0,
        role: callChainFrameRole(label, sourceId, index, path.length),
      };
    })
    .filter((entry, index, all) =>
      index === 0 ||
      index === all.length - 1 ||
      entry.sourceId !== "unknown" ||
      isImportantCallChainWrapper(entry.label),
    );

  if (entries.length <= 10) return entries;
  return [...entries.slice(0, 3), ...entries.slice(-7)];
}

function isConcreteHotPathFrame(label: string, sourceId: string, category: string) {
  if (isImportantCallChainWrapper(label)) return false;
  if (frameMatchesHotPathCategory(label, category)) return false;
  if (isGenericFrame(label)) return false;
  if (sourceId === "unknown" && category !== "entity_tick") return false;
  if (sourceId === "unknown" && !isConcreteEntityTickFrame(label)) return false;
  return true;
}

function summarizeDominantFlamePaths(
  anchors: Array<{
    thread: string;
    index: number;
    node: AnyRecord;
    nodes: AnyRecord[];
    threadSamples: number;
  }>,
  classSources: AnyRecord,
  methodSources: AnyRecord,
  lineSources: AnyRecord,
  sourceMetadata: AnyRecord,
  limit: number,
) {
  const paths: AnyRecord[] = [];
  for (const anchor of anchors.slice(0, HOT_PATH_ANCHOR_LIMIT)) {
    paths.push(...followDominantBranches(
      anchor,
      anchor.index,
      classSources,
      methodSources,
      lineSources,
      sourceMetadata,
      limit,
    ));
  }
  const byKey = new Map<string, AnyRecord>();
  for (const path of paths) {
    const key = path.frames.map((frame: AnyRecord) => frame.label).join(" > ");
    const existing = byKey.get(key);
    if (!existing || Number(path.terminalPercent ?? 0) > Number(existing.terminalPercent ?? 0)) {
      byKey.set(key, path);
    }
  }
  return [...byKey.values()]
    .sort((left, right) => Number(right.terminalPercent ?? 0) - Number(left.terminalPercent ?? 0))
    .slice(0, limit);
}

function followDominantBranches(
  anchor: {
    thread: string;
    index: number;
    nodes: AnyRecord[];
    threadSamples: number;
  },
  index: number,
  classSources: AnyRecord,
  methodSources: AnyRecord,
  lineSources: AnyRecord,
  sourceMetadata: AnyRecord,
  limit: number,
) {
  type Candidate = {
    index: number;
    frames: AnyRecord[];
    branchPoints: AnyRecord[];
    seen: Set<number>;
  };

  const startNode = anchor.nodes[index];
  if (!startNode) return [] as AnyRecord[];

  let frontier: Candidate[] = [{
    index,
    frames: [flameFrame(startNode, anchor.threadSamples, classSources, methodSources, lineSources, sourceMetadata)],
    branchPoints: [],
    seen: new Set([index]),
  }];
  const completed: Candidate[] = [];

  for (let depth = 0; depth <= HOT_PATH_MAX_DEPTH; depth += 1) {
    const next: Candidate[] = [];
    for (const candidate of frontier) {
      const node = anchor.nodes[candidate.index];
      if (!node) continue;
      const children = ((node.childrenRefs ?? []) as unknown[])
        .map((ref) => Number(ref))
        .map((childIndex) => ({ index: childIndex, node: anchor.nodes[childIndex], samples: sumTimes(anchor.nodes[childIndex]?.times) }))
        .filter((child) => child.node && !candidate.seen.has(child.index))
        .sort((left, right) => right.samples - left.samples);

      if (!children.length) {
        completed.push(candidate);
        continue;
      }

      const branchPoint = {
        depth,
        parent: formatStackLabel(node),
        children: children.slice(0, HOT_PATH_BRANCH_WIDTH).map((child) => {
          const entry = flameFrame(child.node, anchor.threadSamples, classSources, methodSources, lineSources, sourceMetadata);
          return {
            ...entry,
            childShareOfParent: sumTimes(node.times) > 0 ? child.samples / sumTimes(node.times) : 0,
          };
        }),
      };

      for (const child of children.slice(0, HOT_PATH_BRANCH_WIDTH)) {
        next.push({
          index: child.index,
          frames: [
            ...candidate.frames,
            flameFrame(child.node, anchor.threadSamples, classSources, methodSources, lineSources, sourceMetadata),
          ],
          branchPoints: [...candidate.branchPoints, branchPoint],
          seen: new Set([...candidate.seen, child.index]),
        });
      }
    }

    if (!next.length) break;
    frontier = next
      .sort((left, right) => Number(right.frames.at(-1)?.percent ?? 0) - Number(left.frames.at(-1)?.percent ?? 0))
      .slice(0, Math.max(limit, HOT_PATH_BEAM_WIDTH));
  }
  completed.push(...frontier);

  return completed
    .filter((candidate) => candidate.frames.length > 1)
    .map((candidate) => {
      const terminal = candidate.frames.at(-1) ?? {};
      return {
        thread: anchor.thread,
        anchor: candidate.frames[0],
        terminal,
        terminalPercent: terminal.percent,
        frames: compactDominantPath(candidate.frames),
        branchPoints: candidate.branchPoints.slice(0, 16),
      };
    })
    .sort((left, right) => Number(right.terminalPercent ?? 0) - Number(left.terminalPercent ?? 0))
    .slice(0, limit);
}

function flameFrame(
  node: AnyRecord,
  threadSamples: number,
  classSources: AnyRecord,
  methodSources: AnyRecord,
  lineSources: AnyRecord,
  sourceMetadata: AnyRecord,
) {
  const label = formatStackLabel(node);
  const className = node.className ?? classNameFromLabel(label);
  const methodName = node.methodName ?? methodNameFromLabel(label);
  const sourceId = resolveSourceId(
    { className, methodName, methodDesc: node.methodDesc, lineNumber: node.lineNumber },
    classSources,
    methodSources,
    lineSources,
  );
  const source = sourceMetadata[sourceId] ?? {};
  const samples = sumTimes(node.times);
  return {
    label,
    className,
    methodName,
    sourceId,
    sourceName: source.name ?? sourceId,
    samples,
    percent: threadSamples > 0 ? (samples / threadSamples) * 100 : 0,
    category: classifyFrame(label),
  };
}

function compactDominantPath(frames: AnyRecord[]) {
  if (frames.length <= 18) return frames;
  return [...frames.slice(0, 8), ...frames.slice(-10)];
}

function isConcreteEntityTickFrame(label: string) {
  const lower = label.toLowerCase();
  return lower.includes(".m_8119_") && (
    lower.includes(".world.entity.") ||
    lower.includes(".entity.") ||
    lower.includes(".mobs.entity.")
  );
}

function isImportantCallChainWrapper(label: string) {
  const lower = label.toLowerCase();
  return lower.includes("neruina") || lower.includes("observable") || lower.includes("catchticking");
}

function callChainFrameRole(label: string, sourceId: string, index: number, length: number) {
  if (index === 0) return "anchor";
  if (index === length - 1) return "terminal";
  if (sourceId === "neruina") return "safety_wrapper";
  if (isImportantCallChainWrapper(label)) return "wrapper";
  return "callee";
}

function interpretCallChain(path: AnyRecord[]) {
  const wrappers = path.filter((entry) => entry.role === "safety_wrapper" || entry.role === "wrapper");
  const terminal = path.at(-1);
  const resolved = path.filter((entry) => entry.sourceId !== "unknown");
  return [
    wrappers.length
      ? `Wrapper frames: ${wrappers.map((entry) => `${entry.sourceName}:${entry.label}`).join(" -> ")}.`
      : "No important wrapper frame in compact chain.",
    terminal ? `Terminal hot frame: ${terminal.sourceName}:${terminal.label}.` : "",
    resolved.length
      ? `Resolved chain sources: ${[...new Set(resolved.map((entry) => entry.sourceName))].join(" -> ")}.`
      : "",
  ].filter(Boolean).join(" ");
}

function frameMatchesHotPathCategory(label: string, category: string) {
  const lower = label.toLowerCase();
  if (category === "entity_tick") return lower.includes("entityticklist") || lower.includes("guardentitytick");
  if (category === "block_entity") return lower.includes("blockentity") || lower.includes("tileentity") || lower.includes("tickingblockentity");
  if (category === "chunk_task") return lower.includes("serverchunkcache") || lower.includes("chunkmap") || lower.includes("worldgen");
  if (category === "entity_ai_pathfinding") return lower.includes("goalselector") || lower.includes("pathnavigation") || lower.includes(".brain.") || lower.includes(".sensing.");
  if (category === "commands") return lower.includes("commandfunction") || lower.includes("commandentry") || lower.includes(".commands.");
  if (category === "io") return lower.includes("filesystem") || lower.includes("file") || lower.includes("io.");
  return classifyFrame(label) === category;
}

function shouldSkipHotPathFrame(label: string, category: string) {
  const lower = label.toLowerCase();
  if (isGenericFrame(label)) return true;
  if (frameMatchesHotPathCategory(label, category)) return true;
  if (lower.includes("$$lambda/")) return true;
  if (category === "entity_tick") {
    return [
      "serverlevel.",
      "level.m_46653_",
      "forgeeventfactory.onpreentitytick",
      "forgeeventfactory.onpostentitytick",
    ].some((part) => lower.includes(part));
  }
  return false;
}

function hotPathFrameRole(label: string, className: string, methodName: string) {
  const lower = `${label} ${className} ${methodName}`.toLowerCase();
  if (lower.includes("goalselector") || lower.includes("goal.")) return "ai_goal";
  if (lower.includes("pathnavigation") || lower.includes("pathfinder")) return "pathfinding";
  if (lower.includes(".brain.") || lower.includes("sensor") || lower.includes("sensing")) return "brain_or_sensor";
  if (lower.includes("eventbus") || lower.includes("forgehooks") || lower.includes("artifactevents")) return "event_hook";
  if (lower.includes("blockentity") || lower.includes("tileentity")) return "block_entity_tick";
  if (lower.includes("commandfunction") || lower.includes("commandentry") || lower.includes(".commands.")) return "command_or_function";
  if (lower.includes("chunk") || lower.includes("worldgen")) return "chunk_task";
  if (methodName === "m_8119_") return "tick";
  if (methodName === "m_8107_") return "ai_step";
  if (methodName === "m_6140_") return "server_ai_step";
  if (methodName === "m_7023_") return "travel_or_movement";
  if (methodName === "m_6075_") return "base_tick";
  if (methodName === "m_6138_") return "push_collisions";
  return "hot_frame";
}

function hotPathInterpretation(category: string, frames: AnyRecord[]) {
  if (!frames.length) return "No focused child frames were found for this category.";
  const directSources = frames.filter((frame) => frame.sourceId !== "unknown").slice(0, 5);
  const topRoles = [...new Set(frames.slice(0, 10).map((frame) => frame.role))].join(", ");
  const topFrames = frames.slice(0, 5).map((frame) => `${frame.label} ${formatPercent(frame.maxPercent)}`).join("; ");
  return [
    `${category} child paths are visible below the aggregate hotspot.`,
    `Top roles: ${topRoles || "unknown"}.`,
    `Top frames: ${topFrames}.`,
    directSources.length
      ? `Resolved mod frames: ${directSources.map((frame) => `${frame.sourceName}:${frame.label}`).join("; ")}.`
      : "No non-unknown mod frame appears near the top of this focused path.",
  ].join(" ");
}

function classifyFrame(label: string) {
  const lower = label.toLowerCase();
  if (
    lower.includes("blockentity") ||
    lower.includes("tileentity") ||
    lower.includes("tickingblockentity") ||
    lower.includes("catchtickingblockentity") ||
    lower.includes("redirecttick") ||
    lower.includes("level.m_46463_")
  ) return "block_entity";
  if (
    lower.includes("entityticklist") ||
    lower.includes("guardentitytick") ||
    lower.includes("catchtickingentities") ||
    lower.includes("safelytickentities") ||
    lower.includes("serverlevel.m_184063_") ||
    lower.includes(".ticknonpassenger") ||
    lower.includes("onnonpassenger") ||
    lower.includes(".tickpassenger")
  ) return "entity_tick";
  if (lower.includes("serverchunkcache") || lower.includes("chunk") || lower.includes("worldgen") || lower.includes("generation")) return "chunk_task";
  if (lower.includes("pathnavigation") || lower.includes("goal") || lower.includes("brain") || lower.includes("sensor")) return "entity_ai_pathfinding";
  if (lower.includes("commandfunction") || lower.includes("commandentry") || lower.includes(".commands.")) return "commands";
  if (lower.includes("level.tick") || lower.includes("serverlevel") || lower.includes("tickchildren")) return "world_tick";
  if (lower.includes("filesystem") || lower.includes("file") || lower.includes("io.")) return "io";
  if (lower.includes("gc") || lower.includes("g1") || lower.includes("shenandoah") || lower.includes("zgc")) return "gc";
  return "other";
}

function classifyHotspot(hotspot: StackHotspot) {
  const category = classifyFrame(hotspot.label);
  if (isServerThreadHotPathCategory(category) && !isServerThreadName(hotspot.thread)) {
    if (category === "block_entity") return "background_block_entity_sync";
    if (category === "chunk_task") return "background_chunk_task";
    return `background_${category}`;
  }
  return category;
}

function isServerThreadName(thread: unknown) {
  return String(thread ?? "").toLowerCase() === "server thread";
}

function packageKey(label: string) {
  const className = classNameFromLabel(label);
  const parts = className.split(".");
  if (parts.length <= 2) return className;
  if (parts[0] === "net" && parts[1] === "minecraft") return parts.slice(0, 3).join(".");
  if (parts[0] === "com" || parts[0] === "org" || parts[0] === "me" || parts[0] === "io") return parts.slice(0, 3).join(".");
  return parts.slice(0, 2).join(".");
}

function classNameFromLabel(label: string) {
  const noLine = label.replace(/:\d+$/, "");
  const lastDot = noLine.lastIndexOf(".");
  if (lastDot <= 0) return noLine;
  return noLine.slice(0, lastDot);
}

function methodNameFromLabel(label: string) {
  const noLine = label.replace(/:\d+$/, "");
  const lastDot = noLine.lastIndexOf(".");
  return lastDot >= 0 ? noLine.slice(lastDot + 1) : noLine;
}

function hasSourceMaps(raw: AnyRecord) {
  return Boolean(
    Object.keys(raw.classSources ?? {}).length ||
    Object.keys(raw.methodSources ?? {}).length ||
    Object.keys(raw.lineSources ?? {}).length,
  );
}

function hasReportEnvironment(raw: AnyRecord) {
  const metadata = raw.metadata ?? {};
  return Boolean(
    Object.keys(metadata.platformMetadata ?? {}).length ||
    Object.keys(metadata.systemStatistics ?? {}).length ||
    Object.keys(metadata.serverConfigurations ?? {}).length ||
    Object.keys(metadata.extraPlatformMetadata ?? {}).length ||
    Object.keys(metadata.sources ?? {}).length,
  );
}

function resolveSourceId(
  frame: {
    className?: string;
    methodName?: string;
    methodDesc?: string;
    lineNumber?: number;
  },
  classSources: AnyRecord,
  methodSources: AnyRecord,
  lineSources: AnyRecord,
) {
  const className = frame.className ?? "";
  const methodName = frame.methodName ?? "";
  const lineNumber = Number(frame.lineNumber ?? 0);
  const methodDesc = frame.methodDesc ?? "";
  const lineKey = className && lineNumber > 0 ? `${className};${lineNumber}` : "";
  const methodKey = className && methodName && methodDesc ? `${className};${methodName};${methodDesc}` : "";
  const legacyMethodKey = className && methodName ? `${className}.${methodName}` : "";
  return (
    (lineKey ? lineSources[lineKey] : undefined) ??
    (methodKey ? methodSources[methodKey] : undefined) ??
    (legacyMethodKey ? methodSources[legacyMethodKey] : undefined) ??
    (className ? classSources[className] : undefined) ??
    "unknown"
  );
}

function sortedWindows(windows: AnyRecord): SparkWindow[] {
  return Object.entries(windows)
    .map(([id, value]) => ({ id, ...(value as AnyRecord) }) as SparkWindow)
    .sort((left, right) => Number(left.id) - Number(right.id));
}

function delta(current: unknown, previous: unknown) {
  const left = Number(current);
  const right = Number(previous);
  if (!Number.isFinite(left) || !Number.isFinite(right)) return undefined;
  return left - right;
}

function entityRiskSignals(entityCounts: AnyRecord | undefined) {
  const signals: string[] = [];
  for (const [name, value] of Object.entries(entityCounts ?? {})) {
    const count = Number(value);
    if (count <= 0) continue;
    const lower = name.toLowerCase();
    if (count >= 50) signals.push(`many:${name}=${count}`);
    if (lower.includes("item")) signals.push(`item_entity:${name}=${count}`);
    if (entityNamespace(name) !== "minecraft") signals.push(`mod_entity:${name}=${count}`);
  }
  return signals;
}

function summarizeEntityNamespaces(chunks: AnyRecord) {
  const namespaces = new Map<string, { namespace: string; entities: number; chunks: AnyRecord[] }>();
  for (const chunk of chunks.topChunks ?? []) {
    for (const entity of (chunk.topEntities ?? []) as NamedValue[]) {
      const namespace = entityNamespace(entity.name);
      if (!namespace || namespace === "minecraft") continue;
      const entry = namespaces.get(namespace) ?? { namespace, entities: 0, chunks: [] };
      entry.entities += Number(entity.value ?? 0);
      if (!entry.chunks.some((item) => item.world === chunk.world && item.x === chunk.x && item.z === chunk.z)) {
        entry.chunks.push(chunk);
      }
      namespaces.set(namespace, entry);
    }
  }
  return namespaces;
}

function bestNamespaceMatch(source: AnyRecord, namespaces: Map<string, { namespace: string; entities: number; chunks: AnyRecord[] }>) {
  const sourceNames = normalizedSourceNamesFor(source);
  for (const namespace of namespaces.keys()) {
    const namespaceToken = normalizeToken(namespace);
    if (sourceNames.includes(namespaceToken)) return namespace;
  }
  return undefined;
}

function matchingEntityFramesForSource(source: AnyRecord, chunks: AnyRecord[]) {
  const frames = Array.isArray(source.frames) ? source.frames : [];
  const frameLabelsText = frames.map((frame: AnyRecord) => normalizeToken(frame.label ?? "")).join(" ");
  const matches: string[] = [];
  for (const chunk of chunks) {
    for (const entity of chunk.topEntities ?? []) {
      const entityName = String(entity.name ?? "");
      const entityId = entityName.split(":").at(-1) ?? entityName;
      const token = normalizeToken(entityId);
      if (!token || token.length < 4) continue;
      if (frameLabelsText.includes(token)) {
        matches.push(entityName);
      }
    }
  }
  return [...new Set(matches)];
}

function sourceHasServerThreadFrame(source: AnyRecord) {
  return ((source.frames ?? []) as AnyRecord[]).some((frame) =>
    String(frame.thread ?? "").toLowerCase().includes("server"),
  );
}

function normalizedSourceNamesFor(source: AnyRecord) {
  return [source.sourceId, source.name]
    .filter(Boolean)
    .map(normalizeToken)
    .filter(Boolean);
}

function normalizeToken(value: string) {
  return value.toLowerCase().replace(/[^a-z0-9]/g, "");
}

function entityNamespace(name: string) {
  return String(name).split(":")[0] || "";
}

function memoryUsageSummary(usage: AnyRecord) {
  const used = Number(usage.used ?? 0);
  const committed = Number(usage.committed ?? 0);
  const max = Number(usage.max ?? 0);
  const effectiveMax = max > 0 ? max : committed;
  return {
    used,
    committed,
    max,
    usedFormatted: formatBytes(used),
    committedFormatted: formatBytes(committed),
    maxFormatted: max > 0 ? formatBytes(max) : "-",
    usedCommittedRatio: ratio(used, committed),
    usedMaxRatio: ratio(used, effectiveMax),
  };
}

function memoryPoolEnvironment(pool: AnyRecord | undefined) {
  const used = Number(pool?.used ?? 0);
  const total = Number(pool?.total ?? 0);
  return compactObject({
    used,
    total,
    usedFormatted: formatBytes(used),
    totalFormatted: formatBytes(total),
    usedRatio: ratio(used, total),
  });
}

function summarizeVmArgs(value: unknown) {
  const text = String(value ?? "").trim();
  if (!text) return undefined;
  const args = text.match(/(?:[^\s"]+|"[^"]*")+/g) ?? [];
  const important = args.filter((arg) =>
    [
      "-Xms",
      "-Xmx",
      "-XX:+Use",
      "-XX:Max",
      "-XX:G1",
      "-XX:+AlwaysPreTouch",
      "-XX:+DisableExplicitGC",
      "-javaagent",
    ].some((prefix) => arg.startsWith(prefix)),
  );
  return {
    count: args.length,
    important: important.slice(0, 32),
  };
}

function topKeyValues(value: AnyRecord, limit: number, valueLimit = 360) {
  return Object.entries(value ?? {})
    .sort(([left], [right]) => left.localeCompare(right))
    .slice(0, limit)
    .map(([key, item]) => summarizeKeyValue(key, item, valueLimit));
}

function summarizeKeyValue(key: string, value: unknown, valueLimit: number) {
  const text = typeof value === "string" ? value : JSON.stringify(value);
  const normalized = String(text ?? "").replace(/\s+/g, " ").trim();
  return {
    key,
    value: normalized.length > valueLimit ? `${normalized.slice(0, valueLimit)}...` : normalized,
    truncated: normalized.length > valueLimit,
    length: normalized.length,
  };
}

function heapSignals(heap: AnyRecord) {
  const summary = memoryUsageSummary(heap);
  const signals: AnyRecord[] = [];
  if (summary.usedMaxRatio >= 0.9) {
    signals.push({
      category: "memory",
      severity: "critical",
      title: "堆使用率接近上限",
      detail: `heap used/max ${formatPercent(summary.usedMaxRatio * 100)} (${summary.usedFormatted} / ${summary.maxFormatted})`,
    });
  } else if (summary.usedMaxRatio >= 0.75) {
    signals.push({
      category: "memory",
      severity: "warning",
      title: "堆使用率偏高",
      detail: `heap used/max ${formatPercent(summary.usedMaxRatio * 100)} (${summary.usedFormatted} / ${summary.maxFormatted})`,
    });
  }
  return signals;
}

function memoryPoolSignals(name: string, usage: AnyRecord, collectionUsage: AnyRecord) {
  const current = memoryUsageSummary(usage);
  const collected = memoryUsageSummary(collectionUsage);
  const lower = String(name ?? "").toLowerCase();
  const signals: AnyRecord[] = [];
  if ((lower.includes("old") || lower.includes("tenured")) && current.usedCommittedRatio >= 0.8) {
    signals.push({
      category: "memory",
      severity: current.usedCommittedRatio >= 0.92 ? "critical" : "warning",
      title: `${name} 使用率偏高`,
      detail: `used/committed ${formatPercent(current.usedCommittedRatio * 100)} (${current.usedFormatted} / ${current.committedFormatted})`,
    });
  }
  if (collected.usedMaxRatio >= 0.85 && (lower.includes("old") || lower.includes("tenured"))) {
    signals.push({
      category: "memory",
      severity: "warning",
      title: `${name} collection usage 偏高`,
      detail: `collection used/max ${formatPercent(collected.usedMaxRatio * 100)}`,
    });
  }
  return signals;
}

function gcSignals(name: string, gc: AnyRecord) {
  const total = Number(gc.total ?? 0);
  const avgTime = Number(gc.avgTime ?? 0);
  const avgFrequency = Number(gc.avgFrequency ?? 0);
  const lower = name.toLowerCase();
  const isOldOrFull = lower.includes("old") || lower.includes("full");
  const signals: AnyRecord[] = [];
  if (total > 0 && avgTime >= 500) {
    signals.push({
      category: "gc",
      severity: "critical",
      title: `${name} 平均暂停极高`,
      detail: `avg ${formatNumber(avgTime)}ms across ${total} collections`,
    });
  } else if (total > 0 && (avgTime >= 100 || (isOldOrFull && avgTime >= 50))) {
    signals.push({
      category: "gc",
      severity: "warning",
      title: `${name} 平均暂停偏高`,
      detail: `avg ${formatNumber(avgTime)}ms across ${total} collections`,
    });
  }
  if (total > 0 && avgFrequency > 0 && avgFrequency <= 2000) {
    signals.push({
      category: "gc",
      severity: "warning",
      title: `${name} 触发频率很高`,
      detail: `average interval ${formatNumber(avgFrequency / 1000)}s`,
    });
  }
  if (isOldOrFull && total > 0 && avgTime >= 200) {
    signals.push({
      category: "gc",
      severity: "critical",
      title: `${name} 发生长暂停`,
      detail: `old/full collector avg ${formatNumber(avgTime)}ms; this can cause visible tick spikes even when heap is not full`,
    });
  }
  return signals;
}

function memoryGcInterpretation(signals: AnyRecord[]) {
  const hasCriticalGc = signals.some((signal) => signal.category === "gc" && signal.severity === "critical");
  const hasMemory = signals.some((signal) => signal.category === "memory");
  if (hasCriticalGc) {
    return "GC 聚合数据存在严重暂停信号；这不是 OOM 结论，而是 STW/GC 行为可能制造卡顿尖峰。";
  }
  if (hasMemory) {
    return "内存池存在压力信号；需要结合 GC 日志或 only-ticks-over 报告确认是否影响 tick。";
  }
  return "未从聚合 GC/内存池数据中发现明显异常；仍可用 GC 日志排除单次停顿。";
}

function ratio(used: number, total: number) {
  if (!Number.isFinite(used) || !Number.isFinite(total) || total <= 0) return 0;
  return used / total;
}

function confidenceFromEvidence(checks: boolean[]) {
  const score = checks.filter(Boolean).length;
  if (score >= 3) return "high";
  if (score >= 2) return "medium";
  return "low";
}

function confidenceRank(value: unknown) {
  if (value === "high") return 3;
  if (value === "medium") return 2;
  if (value === "low") return 1;
  return 0;
}

function frameLabels(frames: unknown) {
  return ((Array.isArray(frames) ? frames : []) as StackHotspot[])
    .map((frame) => frame.label)
    .filter(Boolean)
    .slice(0, 4);
}

function formatChunkEvidence(chunk: AnyRecord) {
  const topEntities = ((chunk.topEntities ?? []) as NamedValue[])
    .slice(0, 4)
    .map((entity) => `${entity.name}=${entity.value}`)
    .join(", ");
  return `${chunk.world ?? "world"} x=${chunk.x} z=${chunk.z}, entities=${chunk.totalEntities}, ${topEntities}`;
}

function compactObject(value: AnyRecord) {
  return Object.fromEntries(Object.entries(value).filter(([, item]) => item !== undefined && item !== "-"));
}

function numberArg(value: unknown, fallback: number) {
  const number = Number(value);
  return Number.isFinite(number) && number > 0 ? Math.min(Math.round(number), 200) : fallback;
}

function reportTitle(kind: ReportKind, platformMeta: AnyRecord) {
  const prefix = kind === "sampler" ? "CPU Profile" : kind === "heap" ? "Heap Summary" : "Health Report";
  return `${prefix} - ${formatPlatform(platformMeta)}`;
}

function formatPlatform(meta: AnyRecord) {
  return [meta.name, meta.version, meta.minecraftVersion].filter(Boolean).join(" ") || "unknown platform";
}

function summarizeGc(gc: AnyRecord | undefined) {
  if (!gc) return [];
  return Object.entries(gc).map(([name, value]) => {
    const item = value as AnyRecord;
    return `${name}: total ${item.total ?? 0}, avg ${formatNumber(item.avgTime)}ms, freq ${formatNumber(item.avgFrequency)}`;
  });
}

function summarizeWorlds(worlds: AnyRecord[] | undefined) {
  return (worlds ?? []).map((world) => `${world.name}: ${world.totalEntities ?? 0} entities`).slice(0, 12);
}

function sortNamedValues(values: AnyRecord | undefined) {
  return Object.entries(values ?? {})
    .map(([name, value]) => ({ name, value: Number(value) }))
    .sort((left, right) => right.value - left.value);
}

function sumTimes(times: unknown) {
  return Array.isArray(times) ? times.reduce((sum, value) => sum + Number(value || 0), 0) : 0;
}

function isWrapperSource(sourceId: unknown, sourceName: unknown) {
  const value = `${String(sourceId ?? "")} ${String(sourceName ?? "")}`.toLowerCase();
  return ["neruina", "observable", "mixin", "minecraft", "unknown"].some((item) => value.includes(item));
}

function buildHotPathAttribution(report: ReportDocument, categoryResults: AnyRecord[], limit: number) {
  const chains = categoryResults.flatMap((result) =>
    ((result.callChains ?? []) as AnyRecord[]).map((chain) => ({ ...chain, category: result.category })),
  );
  const frames = categoryResults.flatMap((result) =>
    ((result.frames ?? []) as AnyRecord[]).map((frame) => ({ ...frame, category: result.category })),
  );
  const knownEntities = knownEntityIds(report);
  const attributionItems: AnyRecord[] = [...chains, ...frames];
  const global = summarizeAttributionItems(attributionItems, knownEntities, limit);
  const byCategory = categoryResults.map((result) => {
    const category = String(result.category ?? "");
    const items = attributionItems.filter((item) => String(item.category ?? "") === category);
    return {
      category,
      ...summarizeAttributionItems(items, knownEntities, limit),
      callChains: ((result.callChains ?? []) as AnyRecord[]).slice(0, Math.min(limit, HOT_PATH_CALL_CHAIN_LIMIT)),
      dominantPaths: ((result.dominantPaths ?? []) as AnyRecord[]).slice(0, Math.min(limit, 24)),
    };
  });

  return {
    topSources: global.topSources,
    entityCandidates: global.entityCandidates,
    byCategory,
    limits: {
      maxDepth: HOT_PATH_MAX_DEPTH,
      anchorLimit: HOT_PATH_ANCHOR_LIMIT,
      callChainLimit: HOT_PATH_CALL_CHAIN_LIMIT,
      branchWidth: HOT_PATH_BRANCH_WIDTH,
      beamWidth: HOT_PATH_BEAM_WIDTH,
    },
    interpretation: [
      "topSources groups concrete terminal hot frames by resolved mod/source.",
      "byCategory repeats the same attribution per selected hot path category so high-usage categories cannot hide behind a global sort.",
      "entityCandidates only appears when a terminal frame class/method can be matched to an entity id present in report world stats.",
      "This improves mod/entity attribution, but ordinary spark sampler data still cannot identify a single entity instance UUID or exact block position.",
    ],
  };
}

function summarizeAttributionItems(
  items: AnyRecord[],
  knownEntities: Array<{ id: string; namespace: string; path: string }>,
  limit: number,
) {
  const bySource = new Map<string, AnyRecord>();
  const entityCandidates: AnyRecord[] = [];

  for (const item of items) {
    const sourceId = String(item.terminalSourceId ?? item.sourceId ?? "unknown");
    const sourceName = String(item.terminalSourceName ?? item.sourceName ?? sourceId);
    const label = String(item.terminalLabel ?? item.label ?? "");
    const percent = Number(item.terminalPercent ?? item.maxPercent ?? 0);
    const category = String(item.category ?? "");
    const matched = matchKnownEntitiesToFrame(label, sourceId, sourceName, knownEntities);

    if (sourceId !== "unknown") {
      const sourceEntry = bySource.get(sourceId) ?? {
        sourceId,
        sourceName,
        maxPercent: 0,
        categories: [] as string[],
        terminalFrames: [] as AnyRecord[],
        matchedEntities: [] as AnyRecord[],
      };
      sourceEntry.maxPercent = Math.max(sourceEntry.maxPercent, percent);
      if (category && !sourceEntry.categories.includes(category)) sourceEntry.categories.push(category);
      if (label && sourceEntry.terminalFrames.length < 8) {
        sourceEntry.terminalFrames.push({ label, percent, category });
      }
      for (const entity of matched) {
        const candidate = {
          entityId: entity.id,
          sourceId,
          sourceName,
          label,
          category,
          percent,
          confidence: entity.confidence,
          reason: entity.reason,
        };
        if (!sourceEntry.matchedEntities.some((existing: AnyRecord) => existing.entityId === entity.id)) {
          sourceEntry.matchedEntities.push(candidate);
        }
      }
      bySource.set(sourceId, sourceEntry);
    }

    for (const entity of matched) {
      entityCandidates.push({
        entityId: entity.id,
        sourceId,
        sourceName,
        label,
        category,
        percent,
        confidence: entity.confidence,
        reason: entity.reason,
      });
    }
  }

  return {
    topSources: [...bySource.values()]
      .sort((left, right) => Number(right.maxPercent ?? 0) - Number(left.maxPercent ?? 0))
      .slice(0, limit),
    entityCandidates: dedupeEntityCandidates(entityCandidates).slice(0, limit),
  };
}

function knownEntityIds(report: ReportDocument) {
  const chunks = summarizeEntityChunks(report, 80);
  const ids = new Set<string>();
  for (const item of (chunks.topEntityTypes ?? []) as NamedValue[]) {
    if (item.name) ids.add(String(item.name));
  }
  for (const chunk of (chunks.topChunks ?? []) as AnyRecord[]) {
    for (const item of (chunk.topEntities ?? []) as NamedValue[]) {
      if (item.name) ids.add(String(item.name));
    }
  }
  return [...ids].map((id) => ({
    id,
    namespace: entityNamespace(id),
    path: id.includes(":") ? id.split(":").slice(1).join(":") : id,
  }));
}

function matchKnownEntitiesToFrame(
  label: string,
  sourceId: string,
  sourceName: string,
  entities: Array<{ id: string; namespace: string; path: string }>,
) {
  const normalizedLabel = normalizeToken(label);
  const simpleClass = normalizeToken(classNameFromLabel(label).split(".").at(-1) ?? "");
  const sourceTokens = normalizedSourceNamesFor({ sourceId, name: sourceName });
  const matches: Array<{ id: string; confidence: string; reason: string }> = [];

  for (const entity of entities) {
    const namespaceToken = normalizeToken(entity.namespace);
    const pathToken = normalizeToken(entity.path);
    if (!pathToken || pathToken.length < 3) continue;
    const namespaceMatches = namespaceToken && sourceTokens.includes(namespaceToken);
    const classMatches =
      normalizedLabel.includes(pathToken) ||
      normalizedLabel.includes(`entity${pathToken}`) ||
      simpleClass === pathToken ||
      simpleClass === `entity${pathToken}`;
    if (!classMatches && !namespaceMatches) continue;
    if (classMatches && namespaceMatches) {
      matches.push({ id: entity.id, confidence: "high", reason: "terminal frame class matches entity id and source namespace" });
    } else if (classMatches) {
      matches.push({ id: entity.id, confidence: "medium", reason: "terminal frame class matches entity id" });
    } else {
      matches.push({ id: entity.id, confidence: "low", reason: "source namespace matches entity namespace, but frame class is not entity-specific" });
    }
  }

  return matches.sort((left, right) => confidenceRank(right.confidence) - confidenceRank(left.confidence));
}

function dedupeEntityCandidates(candidates: AnyRecord[]) {
  const byKey = new Map<string, AnyRecord>();
  for (const candidate of candidates) {
    const key = `${candidate.entityId}|${candidate.sourceId}|${candidate.label}`;
    const existing = byKey.get(key);
    if (!existing || Number(candidate.percent ?? 0) > Number(existing.percent ?? 0)) byKey.set(key, candidate);
  }
  return [...byKey.values()].sort((left, right) =>
    confidenceRank(right.confidence) - confidenceRank(left.confidence) ||
    Number(right.percent ?? 0) - Number(left.percent ?? 0),
  );
}

function maxThreadSamples(thread: AnyRecord, nodes: AnyRecord[], rootRefs: unknown[]) {
  let max = Math.max(sumTimes(thread.times), 0);
  for (const ref of rootRefs) {
    max = Math.max(max, sumTimes(nodes[Number(ref)]?.times));
  }
  for (const node of nodes) {
    max = Math.max(max, sumTimes(node.times));
  }
  return max;
}

function formatStackLabel(node: AnyRecord) {
  const className = node.className ?? "unknown";
  const methodName = node.methodName ?? "unknown";
  const line = node.lineNumber ? `:${node.lineNumber}` : "";
  return `${className}.${methodName}${line}`;
}

function formatTimestamp(value: unknown) {
  const number = Number(value);
  if (!Number.isFinite(number) || number <= 0) return undefined;
  return new Date(number).toLocaleString();
}

function formatDuration(value: unknown) {
  const number = Number(value);
  if (!Number.isFinite(number) || number <= 0) return undefined;
  const seconds = Math.round(number / 1000);
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const restSeconds = seconds % 60;
  return [
    days ? `${days}d` : "",
    hours ? `${hours}h` : "",
    minutes ? `${minutes}m` : "",
    restSeconds || (!days && !hours && !minutes) ? `${restSeconds}s` : "",
  ].filter(Boolean).join(" ");
}

export function formatNumber(value: unknown) {
  const number = Number(value);
  if (!Number.isFinite(number)) return "-";
  return number.toLocaleString("zh-CN", { maximumFractionDigits: 2 });
}

export function formatPercent(value: unknown) {
  const number = Number(value);
  if (!Number.isFinite(number)) return "-";
  return `${formatNumber(number)}%`;
}

export function formatBytes(value: unknown) {
  const number = Number(value);
  if (!Number.isFinite(number) || number <= 0) return "-";
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let scaled = number;
  let unit = 0;
  while (scaled >= 1024 && unit < units.length - 1) {
    scaled /= 1024;
    unit += 1;
  }
  return `${formatNumber(scaled)} ${units[unit]}`;
}
