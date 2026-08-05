use crate::analysis::{
    arr_at, classify_frame, classify_hotspot, f64_at, i64_at, is_generic_frame, is_io_frame,
    is_server_thread_category, is_server_thread_name, obj_at, str_at,
};
use crate::Report;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

const MAX_DEPTH: usize = 64;
const ANCHOR_LIMIT: usize = 40;
const CALL_CHAIN_LIMIT: usize = 48;
const MIN_BRANCH_WIDTH: usize = 8;
const BRANCH_WIDTH: usize = 16;
const BRANCH_COVERAGE: f64 = 0.92;
const MIN_CHILD_SHARE: f64 = 0.02;
const BEAM_WIDTH: usize = 96;
const MAX_ANCHOR_CANDIDATES: usize = 256;
const MAX_DOMINANT_EXPANSIONS: usize = 20_000;
const MAX_DESCENDANT_VISITS: usize = 100_000;
const MAX_DESCENDANT_GROUPS: usize = 4_096;
const MAX_CHILD_REFS_PER_NODE: usize = 4_096;

#[derive(Clone)]
struct Anchor<'a> {
    thread: &'a str,
    index: usize,
    nodes: &'a [Value],
    thread_samples: f64,
}

fn sum_times(node: &Value) -> f64 {
    arr_at(node, "times")
        .into_iter()
        .flatten()
        .filter_map(Value::as_f64)
        .sum()
}

fn stack_label(node: &Value) -> String {
    let class = str_at(node, "className").unwrap_or("unknown");
    let method = str_at(node, "methodName").unwrap_or("unknown");
    let line = i64_at(node, "lineNumber").filter(|line| *line > 0);
    format!(
        "{class}.{method}{}",
        line.map(|line| format!(":{line}")).unwrap_or_default()
    )
}

fn class_from_label(label: &str) -> &str {
    let no_line = label
        .rsplit_once(':')
        .filter(|(_, line)| line.parse::<i64>().is_ok())
        .map_or(label, |(value, _)| value);
    no_line.rsplit_once('.').map_or(no_line, |(class, _)| class)
}

fn method_from_label(label: &str) -> &str {
    let no_line = label
        .rsplit_once(':')
        .filter(|(_, line)| line.parse::<i64>().is_ok())
        .map_or(label, |(value, _)| value);
    no_line
        .rsplit_once('.')
        .map_or(no_line, |(_, method)| method)
}

fn root_refs(thread: &Value, nodes: &[Value]) -> Vec<usize> {
    let refs = arr_at(thread, "childrenRefs")
        .into_iter()
        .flatten()
        .take(MAX_CHILD_REFS_PER_NODE)
        .filter_map(Value::as_u64)
        .filter_map(|value| usize::try_from(value).ok())
        .filter(|index| *index < nodes.len())
        .collect::<Vec<_>>();
    if !refs.is_empty() {
        return refs;
    }
    let used = nodes
        .iter()
        .flat_map(|node| {
            arr_at(node, "childrenRefs")
                .into_iter()
                .flatten()
                .take(MAX_CHILD_REFS_PER_NODE)
        })
        .filter_map(Value::as_u64)
        .filter_map(|value| usize::try_from(value).ok())
        .filter(|index| *index < nodes.len())
        .collect::<HashSet<_>>();
    let roots = (0..nodes.len())
        .filter(|index| !used.contains(index))
        .collect::<Vec<_>>();
    if roots.is_empty() {
        (0..nodes.len()).collect()
    } else {
        roots
    }
}

fn thread_samples(thread: &Value, nodes: &[Value], roots: &[usize]) -> f64 {
    let thread_total = arr_at(thread, "times")
        .into_iter()
        .flatten()
        .filter_map(Value::as_f64)
        .sum::<f64>();
    if thread_total > 0.0 {
        thread_total
    } else {
        roots
            .iter()
            .filter_map(|index| nodes.get(*index))
            .map(sum_times)
            .sum()
    }
}

fn category_matches(label: &str, category: &str) -> bool {
    let lower = label.to_lowercase();
    match category {
        "entity_tick" => lower.contains("entityticklist") || lower.contains("guardentitytick"),
        "block_entity" => {
            lower.contains("blockentity")
                || lower.contains("tileentity")
                || lower.contains("tickingblockentity")
        }
        "chunk_task" => {
            lower.contains("serverchunkcache")
                || lower.contains("chunkmap")
                || lower.contains("worldgen")
        }
        "entity_ai_pathfinding" => {
            lower.contains("goalselector")
                || lower.contains("pathnavigation")
                || lower.contains(".brain.")
                || lower.contains(".sensing.")
        }
        "commands" => {
            lower.contains("commandfunction")
                || lower.contains("commandentry")
                || lower.contains(".commands.")
        }
        "io" => is_io_frame(&lower),
        _ => classify_frame(label) == category,
    }
}

fn actionable(category: &str) -> bool {
    matches!(
        category,
        "entity_tick" | "entity_ai_pathfinding" | "chunk_task" | "block_entity" | "commands" | "io"
    )
}

fn find_anchors<'a>(report: &'a Report, category: &str) -> Vec<Anchor<'a>> {
    #[allow(clippy::too_many_arguments)]
    fn visit<'a>(
        nodes: &'a [Value],
        index: usize,
        thread: &'a str,
        samples: f64,
        category: &str,
        seen: &mut HashSet<usize>,
        out: &mut Vec<Anchor<'a>>,
        depth: usize,
        work: &mut usize,
    ) {
        let Some(node) = nodes.get(index) else { return };
        *work += 1;
        if depth > MAX_DEPTH || *work > 100_000 || !seen.insert(index) {
            return;
        }
        if category_matches(&stack_label(node), category) {
            let candidate = Anchor {
                thread,
                index,
                nodes,
                thread_samples: samples,
            };
            if out.len() < MAX_ANCHOR_CANDIDATES {
                out.push(candidate);
            } else if let Some((smallest_index, smallest_samples)) = out
                .iter()
                .enumerate()
                .map(|(position, anchor)| (position, sum_times(&anchor.nodes[anchor.index])))
                .min_by(|left, right| left.1.total_cmp(&right.1))
            {
                let candidate_samples = sum_times(node);
                if candidate_samples > smallest_samples {
                    out[smallest_index] = candidate;
                }
            }
            return;
        }
        for child in arr_at(node, "childrenRefs")
            .into_iter()
            .flatten()
            .take(MAX_CHILD_REFS_PER_NODE)
            .filter_map(Value::as_u64)
            .filter_map(|value| usize::try_from(value).ok())
        {
            visit(
                nodes,
                child,
                thread,
                samples,
                category,
                seen,
                out,
                depth + 1,
                work,
            );
        }
    }

    let mut out = Vec::new();
    for thread in arr_at(&report.raw, "threads").into_iter().flatten() {
        let name = str_at(thread, "name").unwrap_or("unknown");
        if is_server_thread_category(category) && !is_server_thread_name(name) {
            continue;
        }
        let Some(nodes) = arr_at(thread, "children") else {
            continue;
        };
        let roots = root_refs(thread, nodes);
        let samples = thread_samples(thread, nodes, &roots);
        let mut seen = HashSet::new();
        let mut work = 0;
        for root in roots {
            visit(
                nodes, root, name, samples, category, &mut seen, &mut out, 0, &mut work,
            );
        }
    }
    out.sort_by(|left, right| {
        sum_times(&right.nodes[right.index]).total_cmp(&sum_times(&left.nodes[left.index]))
    });
    out
}

pub(crate) fn resolve_source_id(
    report: &Report,
    class: &str,
    method: &str,
    desc: Option<&str>,
    line: Option<i64>,
) -> Option<String> {
    let line_key = line
        .filter(|line| *line > 0)
        .map(|line| format!("{class};{line}"));
    let method_key = desc
        .filter(|_| !class.is_empty() && !method.is_empty())
        .map(|desc| format!("{class};{method};{desc}"));
    let legacy_key = (!class.is_empty() && !method.is_empty()).then(|| format!("{class}.{method}"));
    let resolved = [
        ("lineSources", line_key.as_deref()),
        ("methodSources", method_key.as_deref()),
        ("methodSources", legacy_key.as_deref()),
        ("classSources", (!class.is_empty()).then_some(class)),
    ]
    .into_iter()
    .find_map(|(map, key)| {
        obj_at(&report.raw, map)?
            .get(key?)?
            .as_str()
            .map(str::to_owned)
    });
    resolved
}

pub(crate) fn resolve_source_metadata(
    report: &Report,
    class: &str,
    method: &str,
    desc: Option<&str>,
    line: Option<i64>,
) -> Option<(String, Value)> {
    let id = resolve_source_id(report, class, method, desc, line)?;
    let metadata = obj_at(&report.raw, "metadata.sources")
        .and_then(|sources| sources.get(&id))
        .cloned()
        .unwrap_or(Value::Null);
    Some((id, metadata))
}

fn source_for_node(report: &Report, node: &Value) -> (String, String) {
    let label = stack_label(node);
    let class = str_at(node, "className").unwrap_or_else(|| class_from_label(&label));
    let method = str_at(node, "methodName").unwrap_or_else(|| method_from_label(&label));
    let id = resolve_source_id(
        report,
        class,
        method,
        str_at(node, "methodDesc"),
        i64_at(node, "lineNumber"),
    )
    .unwrap_or_else(|| "unknown".into());
    let name = obj_at(&report.raw, "metadata.sources")
        .and_then(|sources| sources.get(&id))
        .and_then(|metadata| str_at(metadata, "name"))
        .unwrap_or(&id)
        .to_owned();
    (id, name)
}

fn frame_role(label: &str, method: &str) -> &'static str {
    let lower = label.to_lowercase();
    if lower.contains("goalselector") || lower.contains("goal.") {
        "ai_goal"
    } else if lower.contains("pathnavigation") || lower.contains("pathfinder") {
        "pathfinding"
    } else if lower.contains(".brain.") || lower.contains("sensor") || lower.contains("sensing") {
        "brain_or_sensor"
    } else if lower.contains("eventbus") || lower.contains("forgehooks") {
        "event_hook"
    } else if lower.contains("blockentity") || lower.contains("tileentity") {
        "block_entity_tick"
    } else if lower.contains("commandfunction") || lower.contains(".commands.") {
        "command_or_function"
    } else if lower.contains("chunk") || lower.contains("worldgen") {
        "chunk_task"
    } else {
        match method {
            "m_8119_" => "tick",
            "m_8107_" => "ai_step",
            "m_6140_" => "server_ai_step",
            "m_7023_" => "travel_or_movement",
            "m_6075_" => "base_tick",
            "m_6138_" => "push_collisions",
            _ => "hot_frame",
        }
    }
}

fn wrapper(label: &str) -> bool {
    let lower = label.to_lowercase();
    lower.contains("neruina") || lower.contains("observable") || lower.contains("catchticking")
}

fn skip_frame(label: &str, category: &str) -> bool {
    if is_generic_frame(label) || category_matches(label, category) || label.contains("$$Lambda/") {
        return true;
    }
    category == "entity_tick"
        && [
            "serverlevel.",
            "level.m_46653_",
            "forgeeventfactory.onpreentitytick",
            "forgeeventfactory.onpostentitytick",
        ]
        .iter()
        .any(|part| label.to_lowercase().contains(part))
}

fn concrete_terminal(label: &str, source: &str, category: &str) -> bool {
    if wrapper(label) || category_matches(label, category) || is_generic_frame(label) {
        return false;
    }
    source != "unknown"
        || (category == "entity_tick"
            && label.to_lowercase().contains(".m_8119_")
            && (label.to_lowercase().contains(".world.entity.")
                || label.to_lowercase().contains(".entity.")
                || label.to_lowercase().contains(".mobs.entity.")))
}

fn flame_frame(report: &Report, node: &Value, samples: f64) -> Value {
    let label = stack_label(node);
    let class = str_at(node, "className").unwrap_or_else(|| class_from_label(&label));
    let method = str_at(node, "methodName").unwrap_or_else(|| method_from_label(&label));
    let (source_id, source_name) = source_for_node(report, node);
    let node_samples = sum_times(node);
    json!({
        "label": label,
        "className": class,
        "methodName": method,
        "sourceId": source_id,
        "sourceName": source_name,
        "samples": node_samples,
        "percent": if samples > 0.0 { node_samples / samples * 100.0 } else { 0.0 },
        "category": classify_frame(&stack_label(node)),
    })
}

fn descendant_frames(
    report: &Report,
    anchors: &[Anchor<'_>],
    category: &str,
    limit: usize,
) -> Vec<Value> {
    #[allow(clippy::too_many_arguments)]
    fn visit(
        report: &Report,
        anchor: &Anchor<'_>,
        index: usize,
        depth: usize,
        category: &str,
        seen: &mut HashSet<usize>,
        groups: &mut HashMap<String, Value>,
        work: &mut usize,
    ) {
        let Some(node) = anchor.nodes.get(index) else {
            return;
        };
        *work += 1;
        if depth > MAX_DEPTH || *work > MAX_DESCENDANT_VISITS || !seen.insert(index) {
            return;
        }
        let label = stack_label(node);
        if !skip_frame(&label, category) {
            let class = str_at(node, "className").unwrap_or_else(|| class_from_label(&label));
            let method = str_at(node, "methodName").unwrap_or_else(|| method_from_label(&label));
            let (source_id, source_name) = source_for_node(report, node);
            let samples = sum_times(node);
            let percent = if anchor.thread_samples > 0.0 {
                samples / anchor.thread_samples * 100.0
            } else {
                0.0
            };
            let key = format!("{label}|{source_id}");
            let replace = groups
                .get(&key)
                .and_then(|value| f64_at(value, "maxPercent"))
                .is_none_or(|old| percent > old);
            if replace && (groups.contains_key(&key) || groups.len() < MAX_DESCENDANT_GROUPS) {
                groups.insert(key, json!({
                    "label":label,"className":class,"methodName":method,
                    "sourceId":source_id,"sourceName":source_name,
                    "sourceVersion":obj_at(&report.raw,"metadata.sources").and_then(|sources|sources.get(&source_id)).and_then(|meta|str_at(meta,"version")),
                    "samples":samples,"maxPercent":percent,
                    "role":frame_role(&label,method),
                }));
            }
        }
        for child in arr_at(node, "childrenRefs")
            .into_iter()
            .flatten()
            .take(MAX_CHILD_REFS_PER_NODE)
            .filter_map(Value::as_u64)
            .filter_map(|value| usize::try_from(value).ok())
        {
            visit(
                report,
                anchor,
                child,
                depth + 1,
                category,
                seen,
                groups,
                work,
            );
        }
    }
    let mut groups = HashMap::new();
    let mut work = 0usize;
    for anchor in anchors {
        if work >= MAX_DESCENDANT_VISITS {
            break;
        }
        visit(
            report,
            anchor,
            anchor.index,
            0,
            category,
            &mut HashSet::new(),
            &mut groups,
            &mut work,
        );
    }
    let mut frames = groups.into_values().collect::<Vec<_>>();
    frames.sort_by(|left, right| {
        f64_at(right, "maxPercent")
            .unwrap_or_default()
            .total_cmp(&f64_at(left, "maxPercent").unwrap_or_default())
    });
    frames.truncate(limit);
    frames
}

fn compact_chain(report: &Report, anchor: &Anchor<'_>, path: &[usize]) -> Vec<Value> {
    let mut entries = path
        .iter()
        .enumerate()
        .filter_map(|(position, index)| {
            let node = anchor.nodes.get(*index)?;
            let label = stack_label(node);
            let (id, name) = source_for_node(report, node);
            let samples = sum_times(node);
            let role = if position == 0 {
                "anchor"
            } else if position + 1 == path.len() {
                "terminal"
            } else if id == "neruina" {
                "safety_wrapper"
            } else if wrapper(&label) {
                "wrapper"
            } else {
                "callee"
            };
            (position == 0 || position + 1 == path.len() || id != "unknown" || wrapper(&label))
                .then(|| json!({"label":label,"sourceId":id,"sourceName":name,"percent":if anchor.thread_samples>0.0{samples/anchor.thread_samples*100.0}else{0.0},"role":role}))
        })
        .collect::<Vec<_>>();
    if entries.len() > 10 {
        let tail = entries.split_off(entries.len() - 7);
        entries.truncate(3);
        entries.extend(tail);
    }
    entries
}

fn call_chains(
    report: &Report,
    anchors: &[Anchor<'_>],
    category: &str,
    limit: usize,
) -> Vec<Value> {
    #[allow(clippy::too_many_arguments)]
    fn visit(
        report: &Report,
        anchor: &Anchor<'_>,
        category: &str,
        index: usize,
        path: &mut Vec<usize>,
        seen: &mut HashSet<usize>,
        depth: usize,
        out: &mut Vec<Value>,
        work: &mut usize,
        max_output: usize,
    ) {
        let Some(node) = anchor.nodes.get(index) else {
            return;
        };
        *work += 1;
        if depth > MAX_DEPTH || *work > 100_000 || out.len() >= max_output || !seen.insert(index) {
            return;
        }
        path.push(index);
        let label = stack_label(node);
        let (source_id, source_name) = source_for_node(report, node);
        if concrete_terminal(&label, &source_id, category) {
            let samples = sum_times(node);
            let path_json = compact_chain(report, anchor, path);
            out.push(json!({
                "terminalLabel":label,"terminalSourceId":source_id,"terminalSourceName":source_name,
                "terminalPercent":if anchor.thread_samples>0.0{samples/anchor.thread_samples*100.0}else{0.0},
                "samples":samples,"thread":anchor.thread,"path":path_json,
            }));
        }
        for child in arr_at(node, "childrenRefs")
            .into_iter()
            .flatten()
            .take(MAX_CHILD_REFS_PER_NODE)
            .filter_map(Value::as_u64)
            .filter_map(|value| usize::try_from(value).ok())
        {
            visit(
                report,
                anchor,
                category,
                child,
                path,
                &mut seen.clone(),
                depth + 1,
                out,
                work,
                max_output,
            );
        }
        path.pop();
    }
    let mut all = Vec::new();
    let mut work = 0;
    let max_output = limit.saturating_mul(8).clamp(64, 8_000);
    for anchor in anchors.iter().take(ANCHOR_LIMIT) {
        visit(
            report,
            anchor,
            category,
            anchor.index,
            &mut Vec::new(),
            &mut HashSet::new(),
            0,
            &mut all,
            &mut work,
            max_output,
        );
    }
    let mut unique = HashMap::new();
    for chain in all {
        let key = chain["path"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.get("label").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(" > ");
        let percent = f64_at(&chain, "terminalPercent").unwrap_or_default();
        if unique
            .get(&key)
            .and_then(|old| f64_at(old, "terminalPercent"))
            .is_none_or(|old| percent > old)
        {
            unique.insert(key, chain);
        }
    }
    let mut chains = unique.into_values().collect::<Vec<_>>();
    chains.sort_by(|left, right| {
        f64_at(right, "terminalPercent")
            .unwrap_or_default()
            .total_cmp(&f64_at(left, "terminalPercent").unwrap_or_default())
    });
    chains.truncate(limit);
    chains
}

#[derive(Clone)]
struct BranchCandidate {
    index: usize,
    frames: Vec<Value>,
    branch_points: Vec<Value>,
    seen: HashSet<usize>,
}

fn dominant_paths(report: &Report, anchors: &[Anchor<'_>], limit: usize) -> Vec<Value> {
    let mut paths = Vec::new();
    let selected_anchors = anchors.iter().take(ANCHOR_LIMIT).collect::<Vec<_>>();
    let expansions_per_anchor = (MAX_DOMINANT_EXPANSIONS / selected_anchors.len().max(1)).max(1);
    for anchor in selected_anchors {
        let mut expansions = 0usize;
        let Some(start) = anchor.nodes.get(anchor.index) else {
            continue;
        };
        let mut frontier = vec![BranchCandidate {
            index: anchor.index,
            frames: vec![flame_frame(report, start, anchor.thread_samples)],
            branch_points: Vec::new(),
            seen: HashSet::from([anchor.index]),
        }];
        let mut completed = Vec::new();
        for depth in 0..MAX_DEPTH {
            let mut next = Vec::new();
            for candidate in std::mem::take(&mut frontier) {
                if expansions >= expansions_per_anchor {
                    completed.push(candidate);
                    continue;
                }
                let Some(node) = anchor.nodes.get(candidate.index) else {
                    continue;
                };
                let mut unique_children = HashSet::new();
                let mut children = arr_at(node, "childrenRefs")
                    .into_iter()
                    .flatten()
                    .take(MAX_CHILD_REFS_PER_NODE)
                    .filter_map(Value::as_u64)
                    .filter_map(|index| usize::try_from(index).ok())
                    .filter(|index| unique_children.insert(*index))
                    .filter(|index| {
                        !candidate.seen.contains(index) && anchor.nodes.get(*index).is_some()
                    })
                    .map(|index| (index, sum_times(&anchor.nodes[index])))
                    .collect::<Vec<_>>();
                children.sort_by(|left, right| right.1.total_cmp(&left.1));
                if children.is_empty() {
                    completed.push(candidate);
                    continue;
                }
                let parent_samples = sum_times(node);
                let mut selected = Vec::new();
                let mut covered = 0.0;
                for child in children.iter().copied() {
                    let share = if parent_samples > 0.0 {
                        child.1 / parent_samples
                    } else {
                        0.0
                    };
                    let keep = selected.len() < MIN_BRANCH_WIDTH
                        || (selected.len() < BRANCH_WIDTH
                            && (covered < BRANCH_COVERAGE || share >= MIN_CHILD_SHARE));
                    if !keep {
                        break;
                    }
                    selected.push(child);
                    covered += share;
                }
                let branch = json!({
                    "depth":depth,"parent":stack_label(node),"coveredShare":covered,
                    "omittedChildren":children.len().saturating_sub(selected.len()),
                    "children":selected.iter().map(|(index,samples)|{
                        let mut frame=flame_frame(report,&anchor.nodes[*index],anchor.thread_samples);
                        frame["childShareOfParent"]=json!(if parent_samples>0.0{*samples/parent_samples}else{0.0});frame
                    }).collect::<Vec<_>>()
                });
                for (index, _) in selected {
                    if expansions >= expansions_per_anchor {
                        break;
                    }
                    expansions += 1;
                    let mut child = candidate.clone();
                    child.index = index;
                    child.frames.push(flame_frame(
                        report,
                        &anchor.nodes[index],
                        anchor.thread_samples,
                    ));
                    child.branch_points.push(branch.clone());
                    child.seen.insert(index);
                    next.push(child);
                }
            }
            if next.is_empty() {
                break;
            }
            next.sort_by(|left, right| {
                f64_at(right.frames.last().unwrap_or(&Value::Null), "percent")
                    .unwrap_or_default()
                    .total_cmp(
                        &f64_at(left.frames.last().unwrap_or(&Value::Null), "percent")
                            .unwrap_or_default(),
                    )
            });
            next.truncate(limit.max(BEAM_WIDTH));
            frontier = next;
        }
        completed.extend(frontier);
        for candidate in completed
            .into_iter()
            .filter(|candidate| candidate.frames.len() > 1)
        {
            let terminal = candidate.frames.last().cloned().unwrap_or(Value::Null);
            let frames = if candidate.frames.len() <= 18 {
                candidate.frames
            } else {
                candidate.frames[..8]
                    .iter()
                    .chain(candidate.frames[candidate.frames.len() - 10..].iter())
                    .cloned()
                    .collect()
            };
            paths.push(json!({"thread":anchor.thread,"anchor":frames.first(),"terminal":terminal,"terminalPercent":f64_at(&terminal,"percent"),"frames":frames,"branchPoints":candidate.branch_points.into_iter().take(16).collect::<Vec<_>>() }));
        }
    }
    let mut unique = HashMap::new();
    for path in paths {
        let key = path["frames"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|frame| frame.get("label").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(" > ");
        let percent = f64_at(&path, "terminalPercent").unwrap_or_default();
        if unique
            .get(&key)
            .and_then(|old| f64_at(old, "terminalPercent"))
            .is_none_or(|old| percent > old)
        {
            unique.insert(key, path);
        }
    }
    let mut result = unique.into_values().collect::<Vec<_>>();
    result.sort_by(|a, b| {
        f64_at(b, "terminalPercent")
            .unwrap_or_default()
            .total_cmp(&f64_at(a, "terminalPercent").unwrap_or_default())
    });
    result.truncate(limit);
    result
}

fn normalize(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .filter(|char| char.is_ascii_alphanumeric())
        .collect()
}

#[derive(Clone)]
struct EntityInfo {
    id: String,
    namespace: String,
}

struct EntityIndex {
    by_token: HashMap<String, Vec<EntityInfo>>,
    truncated: bool,
}

struct EntityMatches<'a> {
    entities: Vec<&'a EntityInfo>,
    truncated: bool,
}

fn known_entities(report: &Report) -> EntityIndex {
    const MAX_ENTITY_IDS: usize = 4_096;
    const MAX_ENTITY_INDEX_BYTES: usize = 1024 * 1024;
    const MAX_ENTITY_ID_BYTES: usize = 4_096;
    const MAX_ENTITY_NAMESPACE_BYTES: usize = 512;
    const MAX_ENTITY_PATH_BYTES: usize = 4_096;

    let mut seen = HashSet::new();
    let mut by_token: HashMap<String, Vec<EntityInfo>> = HashMap::new();
    let mut indexed_bytes = 0usize;
    let mut truncated = false;
    let mut add_id = |id: &str| {
        if seen.contains(id) {
            return;
        }
        if seen.len() >= MAX_ENTITY_IDS {
            truncated = true;
            return;
        }
        let (namespace, path) = id.split_once(':').unwrap_or(("", id));
        let token = normalize(path);
        if token.len() < 3 {
            return;
        }
        let normalized_namespace = normalize(namespace);
        let added_bytes = id
            .len()
            .saturating_mul(2)
            .saturating_add(token.len())
            .saturating_add(normalized_namespace.len());
        if id.len() > MAX_ENTITY_ID_BYTES
            || namespace.len() > MAX_ENTITY_NAMESPACE_BYTES
            || path.len() > MAX_ENTITY_PATH_BYTES
            || indexed_bytes.saturating_add(added_bytes) > MAX_ENTITY_INDEX_BYTES
        {
            truncated = true;
            return;
        }
        seen.insert(id.to_owned());
        indexed_bytes += added_bytes;
        by_token.entry(token).or_default().push(EntityInfo {
            id: id.to_owned(),
            namespace: normalized_namespace,
        });
    };
    if let Some(counts) = obj_at(
        &report.raw,
        "metadata.platformStatistics.world.entityCounts",
    ) {
        for id in counts.keys() {
            add_id(id);
        }
    }
    for world in arr_at(&report.raw, "metadata.platformStatistics.world.worlds")
        .into_iter()
        .flatten()
    {
        for region in arr_at(world, "regions").into_iter().flatten() {
            for chunk in arr_at(region, "chunks").into_iter().flatten() {
                if let Some(counts) = obj_at(chunk, "entityCounts") {
                    for id in counts.keys() {
                        add_id(id);
                    }
                }
            }
        }
    }
    for entities in by_token.values_mut() {
        entities.sort_by(|left, right| left.id.cmp(&right.id));
    }
    EntityIndex {
        by_token,
        truncated,
    }
}

fn matching_entities<'a>(index: &'a EntityIndex, label: &str) -> EntityMatches<'a> {
    let simple = class_from_label(label).rsplit('.').next().unwrap_or(label);
    let mut end = simple.len().min(16 * 1024);
    while !simple.is_char_boundary(end) {
        end -= 1;
    }
    let truncated_label = end < simple.len();
    let normalized = normalize(&simple[..end]);
    let mut candidate_tokens = vec![normalized.as_str()];
    if let Some(token) = normalized.strip_suffix("entity") {
        if token.len() >= 3 {
            candidate_tokens.push(token);
        }
    }
    if let Some(token) = normalized.strip_prefix("entity") {
        if token.len() >= 3 {
            candidate_tokens.push(token);
        }
    }
    let mut seen = HashSet::new();
    let mut matches = Vec::new();
    for token in candidate_tokens {
        if let Some(entities) = index.by_token.get(token) {
            for entity in entities {
                if seen.insert(entity.id.as_str()) {
                    matches.push(entity);
                    if matches.len() >= 32 {
                        return EntityMatches {
                            entities: matches,
                            truncated: true,
                        };
                    }
                }
            }
        }
    }
    EntityMatches {
        entities: matches,
        truncated: truncated_label,
    }
}

fn attribution(report: &Report, categories: &[Value], limit: usize) -> Value {
    let entities = known_entities(report);
    let mut entity_matching_truncated = entities.truncated;
    let mut by_source: HashMap<String, Value> = HashMap::new();
    let mut entity_candidates = Vec::new();
    for result in categories {
        let category = result.get("category").and_then(Value::as_str).unwrap_or("");
        let items = result
            .get("callChains")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|item| (item, true))
            .chain(
                result
                    .get("frames")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|item| (item, false)),
            );
        for (item, chain) in items {
            let id = item
                .get(if chain {
                    "terminalSourceId"
                } else {
                    "sourceId"
                })
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let name = item
                .get(if chain {
                    "terminalSourceName"
                } else {
                    "sourceName"
                })
                .and_then(Value::as_str)
                .unwrap_or(id);
            let label = item
                .get(if chain { "terminalLabel" } else { "label" })
                .and_then(Value::as_str)
                .unwrap_or("");
            let percent = item
                .get(if chain {
                    "terminalPercent"
                } else {
                    "maxPercent"
                })
                .and_then(Value::as_f64)
                .unwrap_or_default();
            let source_tokens = [normalize(id), normalize(name)];
            let entity_matches = matching_entities(&entities, label);
            entity_matching_truncated |= entity_matches.truncated;
            let matched=entity_matches.entities.into_iter().map(|entity|{let namespace_match=!entity.namespace.is_empty()&&source_tokens.contains(&entity.namespace);json!({"entityId":entity.id,"sourceId":id,"sourceName":name,"label":label,"category":category,"percent":percent,"confidence":if namespace_match{"high"}else{"medium"},"reason":if namespace_match{"terminal frame class matches entity id and source namespace"}else{"terminal frame class matches entity id"}})}).collect::<Vec<_>>();
            entity_candidates.extend(matched.clone());
            if id != "unknown" {
                let entry=by_source.entry(id.into()).or_insert_with(||json!({"sourceId":id,"sourceName":name,"maxPercent":0.0,"categories":[],"terminalFrames":[],"matchedEntities":[]}));
                entry["maxPercent"] = json!(entry["maxPercent"]
                    .as_f64()
                    .unwrap_or_default()
                    .max(percent));
                let cats = entry["categories"].as_array_mut().unwrap();
                if !cats.iter().any(|v| v.as_str() == Some(category)) {
                    cats.push(json!(category));
                }
                let frames = entry["terminalFrames"].as_array_mut().unwrap();
                if !label.is_empty() && frames.len() < 8 {
                    frames.push(json!({"label":label,"percent":percent,"category":category}));
                }
                let ents = entry["matchedEntities"].as_array_mut().unwrap();
                for candidate in matched {
                    if !ents
                        .iter()
                        .any(|old| old["entityId"] == candidate["entityId"])
                    {
                        ents.push(candidate);
                    }
                }
            }
        }
    }
    let mut top = by_source.into_values().collect::<Vec<_>>();
    top.sort_by(|a, b| {
        f64_at(b, "maxPercent")
            .unwrap_or_default()
            .total_cmp(&f64_at(a, "maxPercent").unwrap_or_default())
    });
    top.truncate(limit);
    entity_candidates.sort_by(|a, b| {
        f64_at(b, "percent")
            .unwrap_or_default()
            .total_cmp(&f64_at(a, "percent").unwrap_or_default())
    });
    let mut seen = HashSet::new();
    entity_candidates.retain(|item| {
        seen.insert(format!(
            "{}|{}|{}",
            item["entityId"], item["sourceId"], item["label"]
        ))
    });
    entity_candidates.truncate(limit);
    let by_category=categories.iter().map(|result|{let category=result["category"].clone();let items=vec![result.clone()];let local=attribution_shallow(&items,&entities,limit);json!({"category":category,"topSources":local["topSources"],"entityCandidates":local["entityCandidates"],"entityMatchingTruncated":local["entityMatchingTruncated"],"callChains":result["callChains"],"dominantPaths":result["dominantPaths"]})}).collect::<Vec<_>>();
    json!({"topSources":top,"entityCandidates":entity_candidates,"entityMatchingTruncated":entity_matching_truncated,"byCategory":by_category,"limits":{"maxDepth":MAX_DEPTH,"anchorLimit":ANCHOR_LIMIT,"callChainLimit":CALL_CHAIN_LIMIT,"branchWidth":BRANCH_WIDTH,"beamWidth":BEAM_WIDTH,"entityIds":4_096,"entityIndexBytes":1024*1024}})
}

fn attribution_shallow(categories: &[Value], entities: &EntityIndex, limit: usize) -> Value {
    let mut sources: HashMap<String, Value> = HashMap::new();
    let mut candidates = Vec::new();
    let mut entity_matching_truncated = entities.truncated;
    for result in categories {
        let category = result["category"].as_str().unwrap_or("");
        for (item, chain) in result["callChains"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|v| (v, true))
            .chain(
                result["frames"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|v| (v, false)),
            )
        {
            let id = item[if chain {
                "terminalSourceId"
            } else {
                "sourceId"
            }]
            .as_str()
            .unwrap_or("unknown");
            let name = item[if chain {
                "terminalSourceName"
            } else {
                "sourceName"
            }]
            .as_str()
            .unwrap_or(id);
            let label = item[if chain { "terminalLabel" } else { "label" }]
                .as_str()
                .unwrap_or("");
            let percent = item[if chain {
                "terminalPercent"
            } else {
                "maxPercent"
            }]
            .as_f64()
            .unwrap_or_default();
            if id != "unknown" {
                let e=sources.entry(id.into()).or_insert_with(||json!({"sourceId":id,"sourceName":name,"maxPercent":0.0,"categories":[],"terminalFrames":[]}));
                e["maxPercent"] = json!(e["maxPercent"].as_f64().unwrap_or_default().max(percent));
                e["categories"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!(category));
                if e["terminalFrames"].as_array().unwrap().len() < 8 {
                    e["terminalFrames"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!({"label":label,"percent":percent,"category":category}));
                }
            }
            let entity_matches = matching_entities(entities, label);
            entity_matching_truncated |= entity_matches.truncated;
            for entity in entity_matches.entities {
                candidates.push(json!({"entityId":entity.id,"sourceId":id,"sourceName":name,"label":label,"category":category,"percent":percent,"confidence":"medium","reason":"terminal frame class matches entity id"}));
            }
        }
    }
    let mut top = sources.into_values().collect::<Vec<_>>();
    top.sort_by(|a, b| {
        f64_at(b, "maxPercent")
            .unwrap_or_default()
            .total_cmp(&f64_at(a, "maxPercent").unwrap_or_default())
    });
    top.truncate(limit);
    candidates.sort_by(|a, b| {
        f64_at(b, "percent")
            .unwrap_or_default()
            .total_cmp(&f64_at(a, "percent").unwrap_or_default())
    });
    let mut seen = HashSet::new();
    candidates.retain(|item| {
        seen.insert(format!(
            "{}|{}|{}",
            item["entityId"], item["sourceId"], item["label"]
        ))
    });
    candidates.truncate(limit);
    json!({"topSources":top,"entityCandidates":candidates,"entityMatchingTruncated":entity_matching_truncated})
}

fn specific(report: &Report, category: &str, limit: usize) -> Value {
    let anchors = find_anchors(report, category);
    let frames = descendant_frames(report, &anchors, category, limit);
    let chains = call_chains(report, &anchors, category, limit.min(CALL_CHAIN_LIMIT));
    let dominant = dominant_paths(report, &anchors, limit.min(24));
    json!({"category":category,"anchors":anchors.iter().take(8).map(|anchor|{let node=&anchor.nodes[anchor.index];let samples=sum_times(node);json!({"thread":anchor.thread,"label":stack_label(node),"samples":samples,"percent":if anchor.thread_samples>0.0{samples/anchor.thread_samples*100.0}else{0.0}})}).collect::<Vec<_>>(),"dominantPaths":dominant,"callChains":chains,"frames":frames,"interpretation":if frames.is_empty(){"No focused child frames were found for this category."}else{"Focused child paths and terminal sources were resolved below the aggregate hotspot."},"limitations":["Frames are sampled inclusive stack data, not exact exclusive CPU time.","A sampler cannot identify one entity UUID or exact block position without matching per-instance context."]})
}

pub(crate) fn execute(report: &Report, wanted: &str, limit: usize) -> Value {
    if wanted != "auto" {
        return specific(report, wanted, limit);
    }
    let mut groups: HashMap<String, f64> = HashMap::new();
    for hotspot in &report.summary.top_hotspots {
        let category = classify_hotspot(&hotspot.label, &hotspot.thread);
        let value = groups.entry(category).or_default();
        *value = value.max(hotspot.percent);
    }
    let mut actionable_groups = groups
        .iter()
        .filter(|(category, _)| actionable(category))
        .map(|(category, percent)| (category.clone(), *percent))
        .collect::<Vec<_>>();
    actionable_groups.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut selected = if actionable_groups.iter().any(|(_, percent)| *percent >= 3.0) {
        actionable_groups
            .iter()
            .filter(|(_, percent)| *percent >= 3.0)
            .take(6)
            .map(|(category, _)| category.clone())
            .collect::<Vec<_>>()
    } else {
        actionable_groups
            .first()
            .map(|(category, _)| vec![category.clone()])
            .unwrap_or_default()
    };
    selected.dedup();
    let categories = selected
        .iter()
        .map(|category| specific(report, category, limit))
        .collect::<Vec<_>>();
    let mut chains = categories
        .iter()
        .flat_map(|result| {
            result["callChains"]
                .as_array()
                .into_iter()
                .flatten()
                .map(move |chain| {
                    let mut chain = chain.clone();
                    chain["category"] = result["category"].clone();
                    chain
                })
        })
        .collect::<Vec<_>>();
    chains.sort_by(|a, b| {
        f64_at(b, "terminalPercent")
            .unwrap_or_default()
            .total_cmp(&f64_at(a, "terminalPercent").unwrap_or_default())
    });
    chains.truncate(limit);
    let mut frames = categories
        .iter()
        .flat_map(|result| {
            result["frames"]
                .as_array()
                .into_iter()
                .flatten()
                .map(move |frame| {
                    let mut frame = frame.clone();
                    frame["category"] = result["category"].clone();
                    frame
                })
        })
        .collect::<Vec<_>>();
    frames.sort_by(|a, b| {
        f64_at(b, "maxPercent")
            .unwrap_or_default()
            .total_cmp(&f64_at(a, "maxPercent").unwrap_or_default())
    });
    frames.truncate(limit);
    let attr = attribution(report, &categories, limit);
    let mut skipped = groups
        .into_iter()
        .filter(|(category, _)| !selected.contains(category))
        .collect::<Vec<_>>();
    skipped.sort_by(|a, b| b.1.total_cmp(&a.1));
    json!({"category":"auto","selectedCategories":selected,"callChains":chains,"categories":categories,"frames":frames,"attribution":attr,"selectionRule":"Drill actionable hotspot categories with maxPercent >= 3%, sorted by category maxPercent.","skippedCategories":skipped.into_iter().take(8).map(|(category,max)|json!({"category":category,"maxPercent":max,"reason":if actionable(&category){"below selection limit"}else{"aggregate or unsupported category"}})).collect::<Vec<_>>()})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ReportKind, ReportSummary, StackHotspot};

    fn fixture() -> Report {
        let node = |class: &str, method: &str, times: f64, children: Vec<usize>| json!({"className":class,"methodName":method,"methodDesc":"()V","lineNumber":42,"times":[times],"childrenRefs":children});
        Report {
            kind: ReportKind::Sampler,
            source: "fixture".into(),
            raw: json!({"metadata":{"sources":{"create":{"name":"Create","version":"1"},"method-source":{"name":"Method Source"},"class-source":{"name":"Class Source"}},"platformStatistics":{"world":{"entityCounts":{"create:contraption":4},"worlds":[]}}},"classSources":{"com.simibubi.create.content.processing.MechanicalPress":"class-source"},"methodSources":{"com.simibubi.create.content.processing.MechanicalPress;tick;()V":"method-source"},"lineSources":{"com.simibubi.create.content.processing.MechanicalPress;42":"create"},"threads":[{"name":"Server thread","times":[100.0],"childrenRefs":[0],"children":[node("net.minecraft.server.level.ServerLevel","tick",100.0,vec![1]),node("net.minecraft.world.level.block.entity.TickingBlockEntity","tick",70.0,vec![2]),node("com.bawnorton.neruina.hooks.CatchTicking","wrap",60.0,vec![3]),node("com.simibubi.create.content.processing.MechanicalPress","tick",55.0,vec![])]}]}),
            summary: ReportSummary {
                title: "fixture".into(),
                top_hotspots: vec![StackHotspot {
                    label: "net.minecraft.world.level.block.entity.TickingBlockEntity.tick:42"
                        .into(),
                    samples: 70.0,
                    percent: 70.0,
                    thread: "Server thread".into(),
                    source: None,
                    class_name: Some(
                        "net.minecraft.world.level.block.entity.TickingBlockEntity".into(),
                    ),
                    method_name: Some("tick".into()),
                    method_desc: Some("()V".into()),
                    line_number: Some(42),
                }],
                ..Default::default()
            },
        }
    }

    #[test]
    fn returns_real_chain_dominant_path_and_attribution() {
        let value = execute(&fixture(), "auto", 64);
        assert_eq!(value["selectedCategories"][0], "block_entity");
        assert!(value
            .pointer("/categories/0/dominantPaths/0/frames")
            .and_then(Value::as_array)
            .is_some_and(|frames| frames.len() >= 3));
        assert_eq!(
            value
                .pointer("/attribution/topSources/0/sourceId")
                .and_then(Value::as_str),
            Some("create")
        );
        assert_eq!(
            value.pointer("/categories/0/callChains/0/path/1/role"),
            Some(&json!("wrapper"))
        );
        assert_eq!(
            value.pointer("/attribution/byCategory/0/topSources/0/sourceId"),
            Some(&json!("create"))
        );
        assert!(value["attribution"]["entityCandidates"]
            .as_array()
            .is_some_and(Vec::is_empty));
        assert!(value
            .pointer("/categories/0/dominantPaths/0/branchPoints/0/coveredShare")
            .and_then(Value::as_f64)
            .is_some_and(|share| share > 0.0));
        assert_eq!(
            value
                .pointer("/categories/0/callChains/0/terminalSourceId")
                .and_then(Value::as_str),
            Some("create")
        );
    }

    #[test]
    fn exact_line_source_wins() {
        let mut report = fixture();
        assert_eq!(
            resolve_source_id(
                &report,
                "com.simibubi.create.content.processing.MechanicalPress",
                "tick",
                Some("()V"),
                Some(42)
            )
            .as_deref(),
            Some("create")
        );
        report.raw["lineSources"] = json!({});
        assert_eq!(
            resolve_source_id(
                &report,
                "com.simibubi.create.content.processing.MechanicalPress",
                "tick",
                Some("()V"),
                Some(42)
            )
            .as_deref(),
            Some("method-source")
        );
        report.raw["methodSources"] = json!({});
        assert_eq!(
            resolve_source_id(
                &report,
                "com.simibubi.create.content.processing.MechanicalPress",
                "tick",
                Some("()V"),
                Some(42)
            )
            .as_deref(),
            Some("class-source")
        );
    }

    #[test]
    fn server_thread_categories_ignore_background_threads() {
        let mut report = fixture();
        report.raw["threads"][0]["name"] = json!("Worker-1");
        let value = execute(&report, "block_entity", 64);
        assert!(value["anchors"].as_array().unwrap().is_empty());
        assert!(value["callChains"].as_array().unwrap().is_empty());
    }

    #[test]
    fn entity_matching_uses_class_boundaries_not_substrings() {
        let index = EntityIndex {
            by_token: HashMap::from([(
                "pig".to_string(),
                vec![EntityInfo {
                    id: "minecraft:pig".into(),
                    namespace: "minecraft".into(),
                }],
            )]),
            truncated: false,
        };
        assert!(
            matching_entities(&index, "org.bukkit.craftbukkit.SpigotScheduler.run")
                .entities
                .is_empty()
        );
        assert_eq!(
            matching_entities(&index, "net.minecraft.world.entity.animal.PigEntity.tick").entities
                [0]
            .id,
            "minecraft:pig"
        );
    }
}
