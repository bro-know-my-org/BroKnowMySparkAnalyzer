import fs from "node:fs/promises";
import path from "node:path";
import protobuf from "protobufjs";

const files = process.argv.slice(2);
if (files.length === 0) {
  console.error("usage: node scripts/inspect-sparkprofile.mjs <file.sparkprofile>...");
  process.exit(1);
}

const root = await protobuf.load([
  "public/proto/spark_sampler.proto",
  "public/proto/spark_heap.proto",
]);
const sampler = root.lookupType("spark.SamplerData");

for (const file of files) {
  const bytes = await fs.readFile(file);
  const decoded = sampler.decode(bytes);
  const raw = sampler.toObject(decoded, {
    longs: Number,
    enums: String,
    bytes: String,
    defaults: false,
    arrays: true,
    objects: true,
  });

  const metadata = raw.metadata ?? {};
  const platform = metadata.platformMetadata ?? {};
  const stats = metadata.platformStatistics ?? {};
  const windows = Object.values(raw.timeWindowStatistics ?? {});
  const hotspots = collectHotspots(raw).slice(0, 8);

  console.log(`\n${path.basename(file)}`);
  console.log(`platform: ${[platform.name, platform.version, platform.minecraftVersion].filter(Boolean).join(" ")}`);
  console.log(`threads: ${(raw.threads ?? []).length}`);
  console.log(`windows: ${windows.length}`);
  console.log(`tps: ${stats.tps?.last1m ?? "-"} / ${stats.tps?.last5m ?? "-"} / ${stats.tps?.last15m ?? "-"}`);
  console.log(`mspt: median ${stats.mspt?.last1m?.median ?? "-"} p95 ${stats.mspt?.last1m?.percentile95 ?? "-"} max ${stats.mspt?.last1m?.max ?? "-"}`);
  console.log("hotspots:");
  for (const item of hotspots) {
    console.log(`- ${item.percent.toFixed(2)}% ${item.samples.toFixed(0)} ${item.thread} ${item.label}`);
  }

  const prompt = [
    "请分析这份 Minecraft spark CPU profile 摘要，给出优先级、证据和下一步验证。",
    `平台: ${[platform.name, platform.version, platform.minecraftVersion].filter(Boolean).join(" ")}`,
    `TPS 1m/5m/15m: ${stats.tps?.last1m ?? "-"} / ${stats.tps?.last5m ?? "-"} / ${stats.tps?.last15m ?? "-"}`,
    `MSPT median/P95/max: ${stats.mspt?.last1m?.median ?? "-"} / ${stats.mspt?.last1m?.percentile95 ?? "-"} / ${stats.mspt?.last1m?.max ?? "-"}`,
    "热点:",
    ...hotspots.slice(0, 10).map((item) => `- ${item.percent.toFixed(2)}% ${item.thread} ${item.label}`),
  ].join("\n");
  await fs.writeFile(`${file}.prompt.txt`, prompt, "utf8");
}

function collectHotspots(raw) {
  const hotspots = [];
  for (const thread of raw.threads ?? []) {
    const nodes = thread.children ?? [];
    const rootRefs = Array.isArray(thread.childrenRefs) && thread.childrenRefs.length > 0
      ? thread.childrenRefs
      : rootNodeRefs(nodes);
    const threadSamples = maxThreadSamples(thread, nodes, rootRefs);
    for (const rootRef of rootRefs) {
      const node = nodes[Number(rootRef)];
      if (!node) {
        continue;
      }
      visitStackNode(node, nodes, thread.name ?? "unknown", threadSamples, hotspots, 0);
    }
  }
  const byLabel = new Map();
  for (const hotspot of hotspots) {
    if (hotspot.samples <= 0 || isGenericFrame(hotspot.label)) {
      continue;
    }
    const key = `${hotspot.thread}|${hotspot.label}`;
    const existing = byLabel.get(key);
    if (!existing || hotspot.samples > existing.samples) {
      byLabel.set(key, hotspot);
    }
  }
  return [...byLabel.values()].sort((left, right) => right.samples - left.samples);
}

function rootNodeRefs(nodes) {
  const referenced = new Set();
  nodes.forEach((node) => {
    for (const childRef of node.childrenRefs ?? []) {
      referenced.add(Number(childRef));
    }
  });
  const roots = nodes
    .map((_, index) => index)
    .filter((index) => !referenced.has(index));
  return roots.length ? roots : nodes.map((_, index) => index);
}

function visitStackNode(node, siblings, thread, threadSamples, hotspots, depth) {
  const samples = sumTimes(node.times);
  hotspots.push({
    label: `${node.className ?? "unknown"}.${node.methodName ?? "unknown"}${node.lineNumber ? `:${node.lineNumber}` : ""}`,
    samples,
    percent: threadSamples > 0 ? (samples / threadSamples) * 100 : 0,
    thread,
  });
  if (depth > 32) {
    return;
  }
  for (const childRef of node.childrenRefs ?? []) {
    const child = siblings[Number(childRef)];
    if (child) {
      visitStackNode(child, siblings, thread, threadSamples, hotspots, depth + 1);
    }
  }
}

function sumTimes(times) {
  return Array.isArray(times) ? times.reduce((sum, value) => sum + Number(value || 0), 0) : 0;
}

function maxThreadSamples(thread, nodes, rootRefs) {
  let max = Math.max(sumTimes(thread.times), 0);
  for (const ref of rootRefs) {
    max = Math.max(max, sumTimes(nodes[Number(ref)]?.times));
  }
  for (const node of nodes) {
    max = Math.max(max, sumTimes(node.times));
  }
  return max;
}

function isGenericFrame(label) {
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
    "libjvm.",
    "libsystem_pthread.",
    "libsystem_kernel.",
    "__psynch_cvwait",
    "WorkerThread::run",
    "Thread::call_run",
    "thread_native_entry",
    "jdk.internal.",
    "sun.nio.",
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
