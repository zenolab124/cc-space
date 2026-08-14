//! Select the active Claude JSONL branch while keeping the source file read-only.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::models::SessionRecord;

pub(super) struct BranchMeta {
    uuid: Option<String>,
    parent_uuid: Option<String>,
    logical_parent_uuid: Option<String>,
    active_leaf_uuid: Option<String>,
    is_sidechain: bool,
    timestamp_ms: Option<i64>,
    order: usize,
    tool_uses: Vec<String>,
    tool_results: Vec<String>,
}

impl BranchMeta {
    pub(super) fn from_json(value: &Value, order: usize) -> Self {
        let string = |key: &str| value.get(key).and_then(Value::as_str).map(String::from);
        let active_leaf_uuid = (value.get("type").and_then(Value::as_str) == Some("last-prompt"))
            .then(|| string("leafUuid"))
            .flatten();
        Self {
            uuid: string("uuid"),
            parent_uuid: string("parentUuid"),
            logical_parent_uuid: string("logicalParentUuid"),
            active_leaf_uuid,
            is_sidechain: value
                .get("isSidechain")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            timestamp_ms: string("timestamp")
                .and_then(|timestamp| chrono::DateTime::parse_from_rfc3339(&timestamp).ok())
                .map(|timestamp| timestamp.timestamp_millis()),
            order,
            tool_uses: content_ids(value, "tool_use", "id"),
            tool_results: content_ids(value, "tool_result", "tool_use_id"),
        }
    }
}

pub(super) struct BranchRecord {
    record: SessionRecord,
    meta: BranchMeta,
}

impl BranchRecord {
    pub(super) fn new(record: SessionRecord, meta: BranchMeta) -> Self {
        Self { record, meta }
    }
}

pub(super) fn select_active_branch(records: Vec<BranchRecord>) -> Vec<SessionRecord> {
    let Some(active) = active_indices(&records) else {
        return into_records(records);
    };
    let active_tool_uses: HashSet<String> = active
        .iter()
        .flat_map(|index| records[*index].meta.tool_uses.iter().cloned())
        .collect();

    records
        .into_iter()
        .enumerate()
        .filter(|(index, entry)| {
            entry.meta.uuid.is_none()
                || active.contains(index)
                || entry
                    .meta
                    .tool_results
                    .iter()
                    .any(|id| active_tool_uses.contains(id.as_str()))
        })
        .map(|(_, entry)| entry.record)
        .collect()
}

/// `None` means the graph is unsafe to filter and the caller must fail open.
fn active_indices(records: &[BranchRecord]) -> Option<HashSet<usize>> {
    let explicit_leaf = records
        .iter()
        .rev()
        .find_map(|entry| entry.meta.active_leaf_uuid.as_deref());
    let uuid_indices = collect_uuid_indices(records)?;
    let main_indices: HashMap<&str, usize> = uuid_indices
        .iter()
        .filter(|(_, index)| !records[**index].meta.is_sidechain)
        .map(|(uuid, index)| (*uuid, *index))
        .collect();
    if main_indices.is_empty() {
        return None;
    }

    let parents = build_parents(records, &main_indices)?;
    let root_count = main_indices
        .values()
        .filter(|index| parents[**index].is_none())
        .count();
    if explicit_leaf.is_none() && root_count != 1 {
        return None;
    }
    if has_cycle(&parents, main_indices.values().copied()) {
        return None;
    }
    let referenced: HashSet<usize> = parents.iter().flatten().copied().collect();
    let leaf = match explicit_leaf {
        Some(uuid) => {
            let anchor = *main_indices.get(uuid)?;
            latest_descendant_leaf(anchor, &parents, records)
        }
        None => main_indices
            .values()
            .copied()
            .filter(|index| !referenced.contains(index))
            .max_by(|a, b| compare_leaf(&records[*a].meta, &records[*b].meta))?,
    };
    Some(trace_active_chain(leaf, &parents))
}

fn latest_descendant_leaf(
    anchor: usize,
    parents: &[Option<usize>],
    records: &[BranchRecord],
) -> usize {
    let mut children = vec![Vec::new(); parents.len()];
    for (child, parent) in parents.iter().enumerate() {
        if let Some(parent) = parent {
            children[*parent].push(child);
        }
    }
    let mut stack = vec![anchor];
    let mut leaves = Vec::new();
    while let Some(index) = stack.pop() {
        if children[index].is_empty() {
            leaves.push(index);
        } else {
            stack.extend(children[index].iter().copied());
        }
    }
    leaves
        .into_iter()
        .max_by(|a, b| compare_leaf(&records[*a].meta, &records[*b].meta))
        .unwrap_or(anchor)
}

fn collect_uuid_indices(records: &[BranchRecord]) -> Option<HashMap<&str, usize>> {
    let mut uuid_indices = HashMap::new();
    for (index, entry) in records.iter().enumerate() {
        let Some(uuid) = entry.meta.uuid.as_deref() else {
            continue;
        };
        if uuid_indices.insert(uuid, index).is_some() {
            return None;
        }
    }
    Some(uuid_indices)
}

fn resolvable_parent(meta: &BranchMeta, indices: &HashMap<&str, usize>) -> Option<usize> {
    meta.parent_uuid
        .as_deref()
        .and_then(|uuid| indices.get(uuid).copied())
        .or_else(|| {
            meta.logical_parent_uuid
                .as_deref()
                .and_then(|uuid| indices.get(uuid).copied())
        })
}

fn build_parents(
    records: &[BranchRecord],
    indices: &HashMap<&str, usize>,
) -> Option<Vec<Option<usize>>> {
    let mut parents = vec![None; records.len()];
    for index in indices.values().copied() {
        let meta = &records[index].meta;
        if meta.parent_uuid.is_some() || meta.logical_parent_uuid.is_some() {
            parents[index] = Some(resolvable_parent(meta, indices)?);
        }
    }
    Some(parents)
}

fn has_cycle(parents: &[Option<usize>], indices: impl Iterator<Item = usize>) -> bool {
    let mut states = vec![0_u8; parents.len()];
    for start in indices {
        if states[start] != 0 {
            continue;
        }
        let mut path = Vec::new();
        let mut current = Some(start);
        while let Some(index) = current {
            match states[index] {
                0 => {
                    states[index] = 1;
                    path.push(index);
                    current = parents[index];
                }
                1 => return true,
                _ => break,
            }
        }
        for index in path {
            states[index] = 2;
        }
    }
    false
}

fn compare_leaf(a: &BranchMeta, b: &BranchMeta) -> Ordering {
    match (a.timestamp_ms, b.timestamp_ms) {
        (Some(a_time), Some(b_time)) if a_time != b_time => a_time.cmp(&b_time),
        _ => a.order.cmp(&b.order),
    }
}

fn trace_active_chain(leaf: usize, parents: &[Option<usize>]) -> HashSet<usize> {
    let mut active = HashSet::new();
    let mut current = Some(leaf);
    while let Some(index) = current {
        active.insert(index);
        current = parents[index];
    }
    active
}

fn into_records(records: Vec<BranchRecord>) -> Vec<SessionRecord> {
    records.into_iter().map(|entry| entry.record).collect()
}

fn content_ids(value: &Value, block_type: &str, id_key: &str) -> Vec<String> {
    value
        .pointer("/message/content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some(block_type))
        .filter_map(|block| block.get(id_key).and_then(Value::as_str).map(String::from))
        .collect()
}

#[cfg(test)]
mod tests;
