import fs from "node:fs/promises";
import protobuf from "protobufjs";

const [file] = process.argv.slice(2);
if (!file) {
  console.error("usage: node scripts/agent-smoke.mjs <file.sparkprofile>");
  process.exit(1);
}

const root = await protobuf.load(["public/proto/spark_sampler.proto", "public/proto/spark_heap.proto"]);
const sampler = root.lookupType("spark.SamplerData");
const raw = sampler.toObject(sampler.decode(await fs.readFile(file)), {
  longs: Number,
  enums: String,
  bytes: String,
  defaults: false,
  arrays: true,
  objects: true,
});
const summary = summarize(raw, file);
const toolCalls = [
  ["report_inventory", {}],
  ["overview", {}],
  ["hotspots", { limit: 12 }],
  ["hotspot_groups", { limit: 12 }],
  ["hot_paths", { category: "auto", limit: 16 }],
  ["mod_sources", { limit: 12 }],
  ["time_windows", { limit: 10 }],
  ["worst_windows", { limit: 6 }],
  ["entities", {}],
  ["entity_chunks", { limit: 12 }],
  ["memory_gc", {}],
  ["diagnostic_hypotheses", {}],
  ["evidence_gaps", {}],
];

console.log("# Agent tool smoke");
for (const [tool, args] of toolCalls) {
  const result = execute(tool, args);
  console.log(`\n## ${tool}`);
  console.log(JSON.stringify(result, null, 2).slice(0, 5000));
}

function execute(tool, args = {}) {
  if (tool === "report_inventory") {
    return {
      kind: "sampler",
      source: file,
      availableTools: [
        "overview",
        "hotspots",
        "hotspot_groups",
        "hot_paths",
        "mod_sources",
        "time_windows",
        "worst_windows",
        "entities",
        "entity_chunks",
        "memory_gc",
        "diagnostic_hypotheses",
        "evidence_gaps",
        "raw_field",
      ],
    };
  }
  if (tool === "overview") return summary;
  if (tool === "hotspots") return collectHotspots(raw).slice(0, args.limit ?? 12);
  if (tool === "hotspot_groups") return hotspotGroups(raw, args.limit ?? 12);
  if (tool === "hot_paths") return hotPaths(raw, args.category ?? "entity_tick", args.limit ?? 16);
  if (tool === "mod_sources") return modSources(raw, args.limit ?? 12);
  if (tool === "time_windows") return { windows: sortedWindows(raw).slice(0, args.limit ?? 10) };
  if (tool === "worst_windows") return worstWindows(raw, args.limit ?? 6);
  if (tool === "entities") return raw.metadata?.platformStatistics?.world ?? {};
  if (tool === "entity_chunks") return entityChunks(raw, args.limit ?? 12);
  if (tool === "memory_gc") return memoryGc(raw);
  if (tool === "diagnostic_hypotheses") return diagnosticHypotheses(raw);
  if (tool === "evidence_gaps") return evidenceGaps(raw);
  return { error: `unknown tool ${tool}` };
}

function summarize(raw, source) {
  const metadata = raw.metadata ?? {};
  const platform = metadata.platformMetadata ?? {};
  const stats = metadata.platformStatistics ?? {};
  return {
    source,
    platform: [platform.name, platform.version, platform.minecraftVersion].filter(Boolean).join(" "),
    tps: stats.tps,
    mspt: stats.mspt,
    heap: stats.memory?.heap,
    world: {
      totalEntities: stats.world?.totalEntities,
      topEntities: Object.entries(stats.world?.entityCounts ?? {})
        .map(([name, value]) => ({ name, value: Number(value) }))
        .sort((a, b) => b.value - a.value)
        .slice(0, 12),
    },
  };
}

function collectHotspots(raw) {
  const hotspots = [];
  for (const thread of raw.threads ?? []) {
    const nodes = thread.children ?? [];
    const roots = rootNodeRefs(nodes);
    const total = Math.max(...nodes.map((node) => sumTimes(node.times)), 0);
    for (const rootRef of roots) {
      visit(nodes[rootRef], nodes, thread.name ?? "unknown", total, hotspots, 0);
    }
  }
  const byLabel = new Map();
  for (const item of hotspots) {
    if (isGeneric(item.label)) continue;
    const key = `${item.thread}|${item.label}`;
    const prev = byLabel.get(key);
    if (!prev || item.samples > prev.samples) byLabel.set(key, item);
  }
  return [...byLabel.values()].sort((a, b) => b.samples - a.samples);
}

function hotspotGroups(raw, limit) {
  const groups = new Map();
  for (const item of collectHotspots(raw)) {
    const category = classifyHotspot(item);
    const entry = groups.get(category) ?? { category, samples: 0, maxPercent: 0, frames: [] };
    entry.samples += item.samples;
    entry.maxPercent = Math.max(entry.maxPercent, item.percent);
    if (entry.frames.length < 6) entry.frames.push(item);
    groups.set(category, entry);
  }
  return [...groups.values()]
    .sort((a, b) => {
      if (a.category === "other" && b.category !== "other") return 1;
      if (b.category === "other" && a.category !== "other") return -1;
      return b.samples - a.samples;
    })
    .slice(0, limit);
}

function hotPaths(raw, category, limit) {
  if (category === "auto") {
    const actionableGroups = hotspotGroups(raw, 32)
      .filter((group) => actionableHotPathCategory(group.category))
      .sort((a, b) => Number(b.maxPercent ?? 0) - Number(a.maxPercent ?? 0));
    const selectedCategories = (actionableGroups.some((group) => Number(group.maxPercent ?? 0) >= 3)
      ? actionableGroups.filter((group) => Number(group.maxPercent ?? 0) >= 3)
      : actionableGroups.slice(0, 1))
      .slice(0, 6)
      .map((group) => group.category);
    const categories = [...new Set(selectedCategories)].map((item) => hotPaths(raw, item, limit));
    return {
      category: "auto",
      selectedCategories: [...new Set(selectedCategories)],
      callChains: categories
        .flatMap((result) => (result.callChains ?? []).map((chain) => ({ ...chain, category: result.category })))
        .sort((a, b) => Number(b.terminalPercent ?? 0) - Number(a.terminalPercent ?? 0))
        .slice(0, limit),
      categories,
      frames: categories
        .flatMap((result) => (result.frames ?? []).map((frame) => ({ ...frame, category: result.category })))
        .sort((a, b) => Number(b.maxPercent ?? 0) - Number(a.maxPercent ?? 0))
        .slice(0, limit),
    };
  }

  const classSources = raw.classSources ?? {};
  const methodSources = raw.methodSources ?? {};
  const lineSources = raw.lineSources ?? {};
  const sources = raw.metadata?.sources ?? {};
  const grouped = new Map();
  for (const thread of raw.threads ?? []) {
    if (serverThreadCategory(category) && !serverThreadName(thread.name)) continue;
    const nodes = thread.children ?? [];
    const roots = rootNodeRefs(nodes);
    const total = Math.max(...nodes.map((node) => sumTimes(node.times)), 0);
    for (const rootRef of roots) {
      for (const anchor of findAnchors(nodes, Number(rootRef), category)) {
        for (const node of descendants(nodes, anchor)) {
          const label = nodeLabel(node);
          if (skipHotPathFrame(label, category)) continue;
          const sourceId = resolveSourceId(node, classSources, methodSources, lineSources);
          const metadata = sources[sourceId] ?? {};
          const key = `${label}|${sourceId}`;
          const samples = sumTimes(node.times);
          const percent = total ? (samples / total) * 100 : 0;
          const entry = grouped.get(key) ?? {
            label,
            className: node.className,
            methodName: node.methodName,
            sourceId,
            sourceName: metadata.name ?? sourceId,
            sourceVersion: metadata.version,
            samples: 0,
            maxPercent: 0,
            role: hotPathRole(label, node.methodName),
          };
          entry.samples = Math.max(entry.samples, samples);
          entry.maxPercent = Math.max(entry.maxPercent, percent);
          grouped.set(key, entry);
        }
      }
    }
  }
  return {
    category,
    callChains: hotPathCallChains(raw, category, Math.min(limit, 16)),
    frames: [...grouped.values()]
      .sort((a, b) => b.maxPercent - a.maxPercent || b.samples - a.samples)
      .slice(0, limit),
  };
}

function hotPathCallChains(raw, category, limit) {
  const classSources = raw.classSources ?? {};
  const methodSources = raw.methodSources ?? {};
  const lineSources = raw.lineSources ?? {};
  const sources = raw.metadata?.sources ?? {};
  const chains = [];
  for (const thread of raw.threads ?? []) {
    if (serverThreadCategory(category) && !serverThreadName(thread.name)) continue;
    const nodes = thread.children ?? [];
    const roots = rootNodeRefs(nodes);
    const total = Math.max(sumTimes(thread.times), ...nodes.map((node) => sumTimes(node.times)), 0);
    for (const rootRef of roots) {
      for (const anchor of findAnchors(nodes, Number(rootRef), category)) {
        collectChains(nodes, anchor, [], total, category, classSources, methodSources, lineSources, sources, chains, new Set(), 0);
      }
    }
  }
  const byKey = new Map();
  for (const chain of chains) {
    const key = chain.path.map((entry) => entry.label).join(" > ");
    const previous = byKey.get(key);
    if (!previous || chain.terminalPercent > previous.terminalPercent) byKey.set(key, chain);
  }
  return [...byKey.values()].sort((a, b) => b.terminalPercent - a.terminalPercent).slice(0, limit);
}

function collectChains(nodes, index, path, total, category, classSources, methodSources, lineSources, sources, out, seen, depth) {
  const node = nodes[index];
  if (!node || seen.has(index) || depth > 36) return;
  seen.add(index);
  const nextPath = [...path, node];
  const sourceId = resolveSourceId(node, classSources, methodSources, lineSources);
  const label = nodeLabel(node);
  if (sourceId !== "unknown" && !importantWrapper(label) && !matchesHotPathCategory(label, category) && !isGeneric(label)) {
    const metadata = sources[sourceId] ?? {};
    const terminalPercent = total ? (sumTimes(node.times) / total) * 100 : 0;
    out.push({
      terminalLabel: label,
      terminalSourceId: sourceId,
      terminalSourceName: metadata.name ?? sourceId,
      terminalPercent,
      path: compactChainPath(nextPath, total, classSources, methodSources, lineSources, sources),
    });
  }
  for (const childRef of node.childrenRefs ?? []) {
    collectChains(nodes, Number(childRef), nextPath, total, category, classSources, methodSources, lineSources, sources, out, new Set(seen), depth + 1);
  }
}

function compactChainPath(path, total, classSources, methodSources, lineSources, sources) {
  const entries = path
    .map((node, index) => {
      const sourceId = resolveSourceId(node, classSources, methodSources, lineSources);
      const metadata = sources[sourceId] ?? {};
      const label = nodeLabel(node);
      return {
        label,
        sourceId,
        sourceName: metadata.name ?? sourceId,
        percent: total ? (sumTimes(node.times) / total) * 100 : 0,
        role: index === 0 ? "anchor" : index === path.length - 1 ? "terminal" : sourceId === "neruina" ? "safety_wrapper" : importantWrapper(label) ? "wrapper" : "callee",
      };
    })
    .filter((entry, index, all) => index === 0 || index === all.length - 1 || entry.sourceId !== "unknown" || importantWrapper(entry.label));
  return entries.length <= 10 ? entries : [...entries.slice(0, 3), ...entries.slice(-7)];
}

function importantWrapper(label) {
  const lower = label.toLowerCase();
  return lower.includes("neruina") || lower.includes("observable") || lower.includes("catchticking");
}

function actionableHotPathCategory(category) {
  return ["entity_tick", "entity_ai_pathfinding", "chunk_task", "block_entity", "commands", "io"].includes(category);
}

function findAnchors(nodes, index, category, seen = new Set()) {
  const node = nodes[index];
  if (!node || seen.has(index)) return [];
  seen.add(index);
  const label = nodeLabel(node);
  if (matchesHotPathCategory(label, category)) return [index];
  return (node.childrenRefs ?? []).flatMap((childRef) => findAnchors(nodes, Number(childRef), category, seen));
}

function descendants(nodes, index, seen = new Set(), depth = 0) {
  const node = nodes[index];
  if (!node || seen.has(index) || depth > 36) return [];
  seen.add(index);
  return [node, ...(node.childrenRefs ?? []).flatMap((childRef) => descendants(nodes, Number(childRef), seen, depth + 1))];
}

function matchesHotPathCategory(label, category) {
  const lower = label.toLowerCase();
  if (category === "entity_tick") return lower.includes("entityticklist") || lower.includes("guardentitytick") || lower.includes("safelytickentities") || lower.includes("catchtickingentities");
  if (category === "block_entity") return lower.includes("blockentity") || lower.includes("tileentity") || lower.includes("tickingblockentity") || lower.includes("catchtickingblockentity");
  if (category === "chunk_task") return lower.includes("serverchunkcache") || lower.includes("chunkmap") || lower.includes("worldgen");
  if (category === "commands") return lower.includes("commandfunction") || lower.includes("commandentry") || lower.includes(".commands.");
  if (category === "io") return lower.includes("filesystem") || lower.includes("file") || lower.includes("io.");
  return classify(label) === category;
}

function skipHotPathFrame(label, category) {
  const lower = label.toLowerCase();
  if (isGeneric(label) || matchesHotPathCategory(label, category) || lower.includes("$$lambda/")) return true;
  if (category === "entity_tick") {
    return lower.includes("serverlevel.") || lower.includes("level.m_46653_");
  }
  return false;
}

function hotPathRole(label, methodName) {
  const lower = label.toLowerCase();
  if (lower.includes("goalselector") || lower.includes("goal.")) return "ai_goal";
  if (lower.includes("pathnavigation") || lower.includes("pathfinder")) return "pathfinding";
  if (lower.includes("eventbus") || lower.includes("forgehooks")) return "event_hook";
  if (lower.includes("commandfunction") || lower.includes("commandentry") || lower.includes(".commands.")) return "command_or_function";
  if (methodName === "m_8119_") return "tick";
  if (methodName === "m_8107_") return "ai_step";
  if (methodName === "m_6140_") return "server_ai_step";
  if (methodName === "m_7023_") return "travel_or_movement";
  if (methodName === "m_6138_") return "push_collisions";
  return "hot_frame";
}

function modSources(raw, limit) {
  const sources = raw.metadata?.sources ?? {};
  const classSources = raw.classSources ?? {};
  const methodSources = raw.methodSources ?? {};
  const lineSources = raw.lineSources ?? {};
  const grouped = new Map();
  for (const item of collectHotspots(raw)) {
    const className = item.className ?? item.label.replace(/:\d+$/, "").split(".").slice(0, -1).join(".");
    const methodName = item.methodName ?? item.label.replace(/:\d+$/, "").split(".").at(-1) ?? "";
    const sourceId = resolveSourceId(
      { className, methodName, methodDesc: item.methodDesc, lineNumber: item.lineNumber },
      classSources,
      methodSources,
      lineSources,
    );
    const metadata = sources[sourceId] ?? {};
    const entry = grouped.get(sourceId) ?? {
      sourceId,
      name: metadata.name ?? sourceId,
      version: metadata.version,
      samples: 0,
      maxPercent: 0,
      frames: [],
    };
    entry.samples += item.samples;
    entry.maxPercent = Math.max(entry.maxPercent, item.percent);
    if (entry.frames.length < 6) entry.frames.push(item);
    grouped.set(sourceId, entry);
  }
  const allSources = [...grouped.values()].sort((a, b) => b.samples - a.samples);
  const unresolvedFrameBucket = allSources.find((source) => source.sourceId === "unknown");
  const resolvedSources = allSources.filter((source) => source.sourceId !== "unknown");
  return {
    sourceMapAvailable: hasSourceMaps(raw),
    sourceCount: Object.keys(sources).length,
    resolvedSourceCount: resolvedSources.length,
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
    notableSources: allSources
      .filter((source) => source.sourceId !== "unknown" && source.maxPercent >= 3)
      .sort((a, b) => b.maxPercent - a.maxPercent || b.samples - a.samples)
      .slice(0, limit),
  };
}

function worstWindows(raw, limit) {
  const windows = sortedWindows(raw);
  const enriched = windows.map((window, index) => {
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
    worstByMaxMspt: [...enriched].sort((a, b) => Number(b.msptMax ?? 0) - Number(a.msptMax ?? 0)).slice(0, limit),
    worstByMedianMspt: [...enriched].sort((a, b) => Number(b.msptMedian ?? 0) - Number(a.msptMedian ?? 0)).slice(0, limit),
    lowTpsWindows: [...enriched].sort((a, b) => Number(a.tps ?? 20) - Number(b.tps ?? 20)).slice(0, limit),
  };
}

function entityChunks(raw, limit) {
  const chunks = [];
  const world = raw.metadata?.platformStatistics?.world;
  for (const worldEntry of world?.worlds ?? []) {
    for (const region of worldEntry.regions ?? []) {
      for (const chunk of region.chunks ?? []) {
        chunks.push({
          world: worldEntry.name,
          x: chunk.x,
          z: chunk.z,
          totalEntities: chunk.totalEntities ?? 0,
          topEntities: Object.entries(chunk.entityCounts ?? {})
            .map(([name, value]) => ({ name, value: Number(value) }))
            .sort((a, b) => b.value - a.value)
            .slice(0, 8),
          riskSignals: entityRiskSignals(chunk.entityCounts),
        });
      }
    }
  }
  return {
    totalEntities: world?.totalEntities,
    topEntityTypes: Object.entries(world?.entityCounts ?? {})
      .map(([name, value]) => ({ name, value: Number(value) }))
      .sort((a, b) => b.value - a.value)
      .slice(0, 20),
    topChunks: chunks.sort((a, b) => b.totalEntities - a.totalEntities).slice(0, limit),
  };
}

function memoryGc(raw) {
  const platformStats = raw.metadata?.platformStatistics ?? {};
  const systemStats = raw.metadata?.systemStatistics ?? {};
  const heap = platformStats.memory?.heap ?? {};
  const gcCollectors = Object.entries({ ...(systemStats.gc ?? {}), ...(platformStats.gc ?? {}) })
    .map(([name, gc]) => ({
      name,
      total: Number(gc.total ?? 0),
      avgTimeMs: Number(gc.avgTime ?? 0),
      avgFrequencyMs: Number(gc.avgFrequency ?? 0),
      avgFrequencySeconds: Number(gc.avgFrequency ?? 0) / 1000,
      signals: smokeGcSignals(name, gc),
    }))
    .sort((a, b) => b.avgTimeMs - a.avgTimeMs);
  const heapMax = Number(heap.max || heap.committed || 0);
  return {
    heap: {
      used: Number(heap.used ?? 0),
      committed: Number(heap.committed ?? 0),
      max: Number(heap.max ?? 0),
      usedMaxRatio: heapMax > 0 ? Number(heap.used ?? 0) / heapMax : 0,
    },
    gcCollectors,
    signals: gcCollectors.flatMap((collector) => collector.signals),
  };
}

function smokeGcSignals(name, gc) {
  const total = Number(gc.total ?? 0);
  const avgTime = Number(gc.avgTime ?? 0);
  const avgFrequency = Number(gc.avgFrequency ?? 0);
  const lower = name.toLowerCase();
  const signals = [];
  if (total > 0 && avgTime >= 500) signals.push({ severity: "critical", title: `${name} average pause is very high`, detail: `${formatNumber(avgTime)}ms` });
  else if (total > 0 && (avgTime >= 100 || ((lower.includes("old") || lower.includes("full")) && avgTime >= 50))) signals.push({ severity: "warning", title: `${name} average pause is high`, detail: `${formatNumber(avgTime)}ms` });
  if (total > 0 && avgFrequency > 0 && avgFrequency <= 2000) signals.push({ severity: "warning", title: `${name} is frequent`, detail: `${formatNumber(avgFrequency / 1000)}s interval` });
  if ((lower.includes("old") || lower.includes("full")) && total > 0 && avgTime >= 200) signals.push({ severity: "critical", title: `${name} old/full pause is long`, detail: `${formatNumber(avgTime)}ms` });
  return signals;
}

function diagnosticHypotheses(raw) {
  const groups = hotspotGroups(raw, 10);
  const sources = modSources(raw, 200);
  const chunks = entityChunks(raw, 10);
  const windows = worstWindows(raw, 6);
  const memory = memoryGc(raw);
  const categoryMap = new Map(groups.map((entry) => [entry.category, entry]));
  const hypotheses = [];
  const blockEntityGroup = categoryMap.get("block_entity");
  const entityGroup = categoryMap.get("entity_tick");
  const denseChunk = chunks.topChunks[0];
  const namespaceStats = summarizeEntityNamespaces(chunks);
  const sourceCandidates = (sources.notableSources.length ? sources.notableSources : sources.topSources)
    .filter((source) => source.sourceId !== "unknown")
    .filter(sourceHasServerThreadFrame)
    .slice(0, 32);

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
      conclusion: `${source.name ?? source.sourceId} has sampled hotspot frames; same-namespace entity chunks are scene clues only unless matching entity CPU frames exist.`,
      evidence: [
        `${source.name ?? source.sourceId} max ${formatPercent(source.maxPercent)}, frames: ${frameLabels(source.frames).join("; ")}`,
        blockEntityGroup ? `block_entity max ${formatPercent(blockEntityGroup.maxPercent)}` : "no block_entity group",
        entityGroup ? `entity_tick max ${formatPercent(entityGroup.maxPercent)}` : "no entity_tick group",
        hasEntityCpuEvidence ? `matching entity CPU frames: ${entityFrameMatches.slice(0, 4).join("; ")}` : "no matching entity-type CPU frames",
        hasEntityNamespace
          ? `same-namespace entity chunks (scene clue, not CPU evidence): ${matchedChunks.slice(0, 3).map(formatChunkEvidence).join(" | ")}`
          : "no matching entity namespace chunks",
      ],
    });
  }

  if (blockEntityGroup && Number(blockEntityGroup.maxPercent ?? 0) >= 10) {
    const blockPaths = hotPaths(raw, "block_entity", 12);
    const frames = blockPaths.frames ?? [];
    const resolvedFrames = frames.filter((frame) => frame.sourceId !== "unknown");
    const blockEntitySourceFrames = (sources.notableSources ?? sources.topSources ?? [])
      .flatMap((source) => (source.frames ?? []).map((frame) => ({ source, frame })))
      .filter(({ frame }) => classify(frame.label) === "block_entity" && serverThreadName(frame.thread))
      .slice(0, 8);
    hypotheses.push({
      id: "block_entity_hot_path",
      confidence: confidenceFromEvidence([
        Number(blockEntityGroup.maxPercent ?? 0) >= 25,
        frames.length > 0,
        resolvedFrames.length > 0,
      ]),
      conclusion: "Block entity ticking is a main-thread hot path and must be included in the diagnosis.",
      evidence: [
        `block_entity max ${formatPercent(blockEntityGroup.maxPercent)}`,
        `hot_paths(block_entity): ${frames.slice(0, 8).map((frame) => `${frame.sourceName ?? frame.sourceId}:${frame.label} ${formatPercent(frame.maxPercent)}`).join("; ")}`,
        blockEntitySourceFrames.length
          ? `mod_sources block entity frames: ${blockEntitySourceFrames.map(({ source, frame }) => `${source.name ?? source.sourceId}:${frame.label} ${formatPercent(frame.percent)}`).join("; ")}`
          : "mod_sources did not list block entity source frames",
      ],
    });
  }

  if (denseChunk && Number(denseChunk.totalEntities ?? 0) >= 50) {
    hypotheses.push({
      id: "high_density_entity_chunk",
      confidence: confidenceFromEvidence([
        Number(denseChunk.totalEntities ?? 0) >= 80,
        Boolean(categoryMap.get("entity_tick")),
        Boolean(denseChunk.riskSignals?.length),
      ]),
      conclusion: "A high-density entity chunk is a first inspection target.",
      evidence: [
        `top chunk: ${formatChunkEvidence(denseChunk)}`,
        `risk signals: ${denseChunk.riskSignals?.join(", ") || "none"}`,
      ],
    });
  }

  if (entityGroup) {
    hypotheses.push({
      id: "entity_tick_load",
      confidence: confidenceFromEvidence([entityGroup.maxPercent > 15, Boolean(chunks.topEntityTypes?.length)]),
      conclusion: "Server-thread entity ticking is a relevant load source.",
      evidence: [
        `entity_tick max ${formatPercent(entityGroup.maxPercent)}`,
        `top entity types: ${chunks.topEntityTypes?.slice(0, 6).map((item) => `${item.name}=${item.value}`).join(", ")}`,
      ],
    });
  }

  const chunkGroup = categoryMap.get("chunk_task");
  const worst = windows.worstByMaxMspt?.[0];
  if (chunkGroup || Number(worst?.deltas?.chunksFromPrevious ?? 0) > 500) {
    hypotheses.push({
      id: "chunk_task_or_generation_spike",
      confidence: confidenceFromEvidence([Boolean(chunkGroup), Number(worst?.msptMax ?? 0) > 200, Number(worst?.deltas?.chunksFromPrevious ?? 0) > 300]),
      conclusion: "Lag spikes align with chunk tasks/loading/generation.",
      evidence: [
        chunkGroup ? `chunk_task max ${formatPercent(chunkGroup.maxPercent)}` : "no explicit chunk_task group",
        worst ? `worst window ${worst.id}: max MSPT ${formatNumber(worst.msptMax)}, chunks delta ${worst.deltas?.chunksFromPrevious ?? "-"}` : "no window data",
      ],
    });
  }

  if (memory.signals.length) {
    hypotheses.push({
      id: "memory_gc_pressure",
      confidence: confidenceFromEvidence([
        memory.signals.some((signal) => signal.severity === "critical"),
        memory.signals.length > 0,
      ]),
      conclusion: "GC/memory aggregate statistics show abnormal signals.",
      evidence: [
        ...memory.gcCollectors.slice(0, 3).map((gc) => `${gc.name}: avg ${formatNumber(gc.avgTimeMs)}ms, every ${formatNumber(gc.avgFrequencySeconds)}s`),
        ...memory.signals.slice(0, 3).map((signal) => `${signal.title}: ${signal.detail}`),
      ],
    });
  }

  return {
    hypotheses: hypotheses.sort((a, b) => confidenceRank(b.confidence) - confidenceRank(a.confidence)),
    topChunks: chunks.topChunks?.slice(0, 5),
  };
}

function evidenceGaps(raw) {
  return {
    availableEvidence: [
      raw.threads?.length ? "sampled CPU hotspot tree" : null,
      raw.timeWindowStatistics ? "time window statistics" : null,
      raw.metadata?.platformStatistics?.world ? "world/entity/chunk statistics" : null,
      hasSourceMaps(raw) ? "class/method/line source map" : null,
    ].filter(Boolean),
    cannotProveAlone: [
      "exact entity instance",
      "GC pauses without GC log correlation",
      "mod attribution when source maps are absent",
    ],
  };
}

function visit(node, nodes, thread, total, out, depth) {
  if (!node || depth > 40) return;
  const samples = sumTimes(node.times);
  out.push({
    label: nodeLabel(node),
    samples,
    percent: total ? (samples / total) * 100 : 0,
    thread,
    className: node.className,
    methodName: node.methodName,
    methodDesc: node.methodDesc,
    lineNumber: node.lineNumber,
  });
  for (const ref of node.childrenRefs ?? []) visit(nodes[Number(ref)], nodes, thread, total, out, depth + 1);
}

function nodeLabel(node) {
  return `${node.className ?? "unknown"}.${node.methodName ?? "unknown"}${node.lineNumber ? `:${node.lineNumber}` : ""}`;
}

function rootNodeRefs(nodes) {
  const referenced = new Set();
  for (const node of nodes) for (const ref of node.childrenRefs ?? []) referenced.add(Number(ref));
  return nodes.map((_, index) => index).filter((index) => !referenced.has(index));
}

function sortedWindows(raw) {
  return Object.entries(raw.timeWindowStatistics ?? {})
    .map(([id, value]) => ({ id, ...value }))
    .sort((a, b) => Number(a.id) - Number(b.id));
}

function delta(current, previous) {
  const left = Number(current);
  const right = Number(previous);
  if (!Number.isFinite(left) || !Number.isFinite(right)) return undefined;
  return left - right;
}

function entityRiskSignals(entityCounts) {
  const signals = [];
  for (const [name, value] of Object.entries(entityCounts ?? {})) {
    const count = Number(value);
    if (count >= 50) signals.push(`many:${name}=${count}`);
    const lower = name.toLowerCase();
    if (lower.includes("item")) signals.push(`item_entity:${name}=${count}`);
    if (entityNamespace(name) !== "minecraft") signals.push(`mod_entity:${name}=${count}`);
  }
  return signals;
}

function summarizeEntityNamespaces(chunks) {
  const namespaces = new Map();
  for (const chunk of chunks.topChunks ?? []) {
    for (const entity of chunk.topEntities ?? []) {
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

function bestNamespaceMatch(source, namespaces) {
  const sourceNames = normalizedSourceNamesFor(source);
  for (const namespace of namespaces.keys()) {
    const namespaceToken = normalizeToken(namespace);
    if (sourceNames.includes(namespaceToken)) return namespace;
  }
  return undefined;
}

function matchingEntityFramesForSource(source, chunks) {
  const frames = Array.isArray(source.frames) ? source.frames : [];
  const frameLabelsText = frames.map((frame) => normalizeToken(frame.label ?? "")).join(" ");
  const matches = [];
  for (const chunk of chunks) {
    for (const entity of chunk.topEntities ?? []) {
      const entityName = String(entity.name ?? "");
      const entityId = entityName.split(":").at(-1) ?? entityName;
      const token = normalizeToken(entityId);
      if (!token || token.length < 4) continue;
      if (frameLabelsText.includes(token)) matches.push(entityName);
    }
  }
  return [...new Set(matches)];
}

function sourceHasServerThreadFrame(source) {
  return (source.frames ?? []).some((frame) => String(frame.thread ?? "").toLowerCase().includes("server"));
}

function normalizedSourceNamesFor(source) {
  return [source.sourceId, source.name]
    .filter(Boolean)
    .map(normalizeToken)
    .filter(Boolean);
}

function normalizeToken(value) {
  return value.toLowerCase().replace(/[^a-z0-9]/g, "");
}

function entityNamespace(name) {
  return String(name).split(":")[0] || "";
}

function confidenceFromEvidence(checks) {
  const score = checks.filter(Boolean).length;
  if (score >= 3) return "high";
  if (score >= 2) return "medium";
  return "low";
}

function confidenceRank(value) {
  if (value === "high") return 3;
  if (value === "medium") return 2;
  if (value === "low") return 1;
  return 0;
}

function frameLabels(frames) {
  return (Array.isArray(frames) ? frames : [])
    .map((frame) => frame.label)
    .filter(Boolean)
    .slice(0, 4);
}

function formatChunkEvidence(chunk) {
  const topEntities = (chunk.topEntities ?? [])
    .slice(0, 4)
    .map((entity) => `${entity.name}=${entity.value}`)
    .join(", ");
  return `${chunk.world ?? "world"} x=${chunk.x} z=${chunk.z}, entities=${chunk.totalEntities}, ${topEntities}`;
}

function formatPercent(value) {
  return `${formatNumber(value)}%`;
}

function formatNumber(value) {
  const number = Number(value);
  return Number.isFinite(number) ? number.toLocaleString("zh-CN", { maximumFractionDigits: 2 }) : "-";
}

function sumTimes(times) {
  return Array.isArray(times) ? times.reduce((sum, value) => sum + Number(value || 0), 0) : 0;
}

function isGeneric(label) {
  if (isMinecraftLoopFrame(label)) return true;
  return [
    "java.lang.Thread.",
    "MinecraftServer.runServer",
    "MinecraftServer.waitUntilNextTick",
    "FileWatcher$WatcherThread.run",
    "io.netty.util.internal.ThreadExecutorMap$2.run",
    "io.netty.util.concurrent.SingleThreadEventExecutor$4.run",
    "io.netty.channel.nio.NioEventLoop.run",
    "io.netty.util.concurrent.SingleThreadEventExecutor.runAllTasks",
    "io.netty.util.concurrent.AbstractEventExecutor.safeExecute",
    "io.netty.util.concurrent.AbstractEventExecutor.runTask",
    "jdk.internal.",
    "$$Lambda.",
    "mixinextras$bridge",
    "LockSupport.park",
    "libjvm.",
    "libsystem_",
  ].some((pattern) => label.includes(pattern));
}

function isMinecraftLoopFrame(label) {
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

function classify(label) {
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
  if (lower.includes("chunk") || lower.includes("worldgen")) return "chunk_task";
  if (lower.includes("pathnavigation") || lower.includes("goal") || lower.includes("brain")) return "entity_ai_pathfinding";
  if (lower.includes("commandfunction") || lower.includes("commandentry") || lower.includes(".commands.")) return "commands";
  if (lower.includes("serverlevel") || lower.includes("level.tick")) return "world_tick";
  return "other";
}

function classifyHotspot(item) {
  const category = classify(item.label);
  if (serverThreadCategory(category) && !serverThreadName(item.thread)) {
    if (category === "block_entity") return "background_block_entity_sync";
    if (category === "chunk_task") return "background_chunk_task";
    return `background_${category}`;
  }
  return category;
}

function serverThreadCategory(category) {
  return ["entity_tick", "entity_ai_pathfinding", "chunk_task", "block_entity", "commands", "world_tick"].includes(category);
}

function serverThreadName(thread) {
  return String(thread ?? "").toLowerCase() === "server thread";
}

function hasSourceMaps(raw) {
  return Boolean(
    Object.keys(raw.classSources ?? {}).length ||
    Object.keys(raw.methodSources ?? {}).length ||
    Object.keys(raw.lineSources ?? {}).length,
  );
}

function resolveSourceId(frame, classSources, methodSources, lineSources) {
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
