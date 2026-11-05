//! Organizational folders and the primary layer of a multi-selection.
use effectcraft_project::{Comp, Layer, LayerGroup, LayerId};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

use super::{CommandSpec, b_p, bad, comp_id, has_comp, has_layers, layers_p, resolve_layer, str_p, unlocked};
use crate::{EngineError, Result, Session, cmd};

fn position(comp: &Comp, id: LayerId) -> Option<usize> {
    comp.layers.iter().position(|layer| layer.id == id)
}

fn group_id(p: &Value, command: &str) -> Result<u64> {
    p.get("group").and_then(Value::as_u64).ok_or_else(|| bad(command, "missing numeric group id"))
}

fn name(p: &Value, command: &str) -> Result<String> {
    let value = str_p(p, "name").unwrap_or("Group").trim();
    if value.is_empty() || value.chars().count() > 256 {
        return Err(bad(command, "group name must contain 1 to 256 characters"));
    }
    Ok(value.to_owned())
}

fn owners(comp: &Comp) -> HashMap<LayerId, u64> {
    comp.layers.iter().filter_map(|l| comp.layer_group(l.id).map(|g| (l.id, g.id))).collect()
}

/// Rebuild inclusive spans from direct ownership, so moving out never fills the old folder back in.
fn rebuild_members(comp: &mut Comp, owners: &HashMap<LayerId, u64>) {
    let indices: HashMap<_, _> = comp.layer_groups.iter().enumerate().map(|(i, g)| (g.id, i)).collect();
    let parents: HashMap<_, _> = comp.layer_groups.iter().map(|g| (g.id, g.parent)).collect();
    for g in &mut comp.layer_groups {
        g.members.clear();
    }
    for layer in &comp.layers {
        let mut next = owners.get(&layer.id).copied();
        let mut seen = HashSet::new();
        for _ in 0..effectcraft_project::MAX_GROUP_DEPTH {
            let Some(id) = next.filter(|id| seen.insert(*id)) else { break };
            if let Some(g) = indices.get(&id).and_then(|i| comp.layer_groups.get_mut(*i)) {
                g.members.push(layer.id);
            }
            next = parents.get(&id).copied().flatten();
        }
    }
    comp.layer_groups.retain(|g| !g.members.is_empty());
}

fn group_locked(comp: &Comp, id: u64) -> bool {
    let mut next = Some(id);
    let mut seen = HashSet::new();
    for _ in 0..effectcraft_project::MAX_GROUP_DEPTH {
        let Some(id) = next.filter(|id| seen.insert(*id)) else { break };
        let Some(group) = comp.layer_groups.iter().find(|g| g.id == id) else { break };
        if group.locked {
            return true;
        }
        next = group.parent;
    }
    false
}

/// Inserting in a parent cannot split one of that parent's child folders.
fn boundary(comp: &Comp, layer: LayerId, parent: Option<u64>) -> LayerId {
    comp.layer_group_path(layer).into_iter().find(|g| g.parent == parent).and_then(|g| g.members.first()).copied().unwrap_or(layer)
}

fn valid_spans(comp: &Comp, command: &str) -> Result<()> {
    if !comp.layer_groups_are_valid() {
        return Err(bad(command, "the move would split a layer group"));
    }
    Ok(())
}

fn active(s: &mut Session, p: &Value) -> Result<Value> {
    let comp = s.active_comp().ok_or(EngineError::NoComp)?;
    let id = p.get("layer").and_then(|v| resolve_layer(comp, v)).ok_or_else(|| bad("layer.setActive", "unknown layer"))?;
    if !s.state.selected_layers.contains(&id) {
        return Err(bad("layer.setActive", "active layer must already be selected"));
    }
    s.state.active_layer = Some(id);
    Ok(json!({"activeLayer": id.0}))
}

fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let (cid, ids) = layers_p(s, p)?;
    let ids = unlocked(s, cid, ids, "layer.group")?;
    if ids.is_empty() {
        return Err(bad("layer.group", "select layers to group"));
    }
    let anchor = s.active_layer().filter(|id| ids.contains(id)).or_else(|| ids.first().copied()).ok_or_else(|| bad("layer.group", "select an active layer"))?;
    let title = name(p, "layer.group")?;
    let id = s.edit("Group Layers", None, |project, state| {
        let id = project.next_id;
        if id == 0 || id >= effectcraft_project::MAX_GROUP_ID {
            return Err(bad("layer.group", "project id space is exhausted"));
        }
        project.next_id = id.checked_add(1).ok_or_else(|| bad("layer.group", "project id space is exhausted"))?;
        let comp = project.comp_mut(cid).ok_or(EngineError::NoComp)?;
        if comp.layer_groups.len() >= 10_000 {
            return Err(bad("layer.group", "too many layer groups"));
        }
        let mut ownership = owners(comp);
        let complete: HashSet<_> = comp.layer_groups.iter().filter(|g| g.members.iter().all(|l| ids.contains(l))).map(|g| g.id).collect();
        let path: Vec<_> = comp.layer_group_path(anchor).into_iter().filter(|g| !complete.contains(&g.id)).map(|g| g.id).collect();
        let parent = path.into_iter().rev().find(|id| ids.iter().all(|l| comp.layer_group_path(*l).iter().any(|g| g.id == *id)));
        let parent_depth = parent
            .and_then(|id| comp.layer_groups.iter().find(|g| g.id == id))
            .and_then(|g| g.members.first())
            .map_or(0, |l| comp.layer_group_path(*l).iter().take_while(|g| Some(g.id) != parent).count() + 1);
        let inner_depth = ids.iter().map(|l| comp.layer_group_path(*l).iter().filter(|g| complete.contains(&g.id)).count()).max().unwrap_or(0);
        if parent_depth.saturating_add(1).saturating_add(inner_depth) > effectcraft_project::MAX_GROUP_DEPTH {
            return Err(bad("layer.group", "maximum group nesting depth reached"));
        }
        let anchor_index = position(comp, boundary(comp, anchor, parent)).ok_or_else(|| bad("layer.group", "active layer is missing"))?;
        let at = comp.layers.iter().take(anchor_index).filter(|l| !ids.contains(&l.id)).count();
        let picked: Vec<_> = comp.layers.iter().filter(|l| ids.contains(&l.id)).cloned().collect();
        let members: Vec<_> = picked.iter().map(|l| l.id).collect();
        comp.layers.retain(|l| !ids.contains(&l.id));
        splice_at(&mut comp.layers, at, picked);
        for group in &mut comp.layer_groups {
            if complete.contains(&group.id) && group.parent.is_none_or(|p| !complete.contains(&p)) {
                group.parent = Some(id);
            }
        }
        for layer in &ids {
            if ownership.get(layer).is_none_or(|owner| !complete.contains(owner)) {
                ownership.insert(*layer, id);
            }
        }
        comp.layer_groups.push(LayerGroup { id, name: title, members, parent, ..Default::default() });
        rebuild_members(comp, &ownership);
        valid_spans(comp, "layer.group")?;
        state.active_layer = Some(anchor);
        Ok(id)
    })?;
    Ok(json!({"group": id}))
}

fn ungroup(s: &mut Session, p: &Value) -> Result<Value> {
    let (cid, ids) = layers_p(s, p)?;
    let explicit = p.get("group").and_then(Value::as_u64);
    s.edit("Ungroup Layers", None, |project, _| {
        let comp = project.comp_mut(cid).ok_or(EngineError::NoComp)?;
        if explicit.is_some_and(|id| !comp.layer_groups.iter().any(|g| g.id == id)) {
            return Err(bad("layer.ungroup", "unknown group"));
        }
        let remove: HashSet<_> =
            if let Some(id) = explicit { [id].into_iter().collect() } else { ids.iter().filter_map(|id| comp.layer_group(*id).map(|g| g.id)).collect() };
        if remove.iter().any(|id| group_locked(comp, *id)) {
            return Err(bad("layer.ungroup", "unlock the group before ungrouping"));
        }
        let parents: HashMap<_, _> = comp.layer_groups.iter().map(|g| (g.id, g.parent)).collect();
        let promoted = |mut id: Option<u64>| {
            for _ in 0..effectcraft_project::MAX_GROUP_DEPTH {
                if id.is_none_or(|id| !remove.contains(&id)) {
                    break;
                }
                id = id.and_then(|id| parents.get(&id).copied().flatten());
            }
            id
        };
        let ownership = owners(comp).into_iter().filter_map(|(layer, id)| promoted(Some(id)).map(|id| (layer, id))).collect();
        for group in &mut comp.layer_groups {
            group.parent = promoted(group.parent);
        }
        comp.layer_groups.retain(|g| !remove.contains(&g.id));
        rebuild_members(comp, &ownership);
        Ok(json!({}))
    })
}

fn update(s: &mut Session, p: &Value) -> Result<Value> {
    let cid = comp_id(s, p)?;
    let id = group_id(p, "layer.groupUpdate")?;
    let title = if p.get("name").is_some() { Some(name(p, "layer.groupUpdate")?) } else { None };
    s.edit("Layer Group", super::merge_p(p), |project, _| {
        let comp = project.comp_mut(cid).ok_or(EngineError::NoComp)?;
        let g = comp.layer_groups.iter_mut().find(|g| g.id == id).ok_or_else(|| bad("layer.groupUpdate", "unknown group"))?;
        if let Some(title) = title {
            g.name = title;
        }
        if let Some(value) = b_p(p, "collapsed") {
            g.collapsed = value;
        }
        if let Some(value) = b_p(p, "video") {
            g.video = value;
        }
        if let Some(value) = b_p(p, "audio") {
            g.audio = value;
        }
        if let Some(value) = b_p(p, "locked") {
            g.locked = value;
        }
        Ok(json!({}))
    })
}

fn add(s: &mut Session, p: &Value) -> Result<Value> {
    let (cid, ids) = layers_p(s, p)?;
    let id = group_id(p, "layer.groupAdd")?;
    let comp = s.project.comp(cid).ok_or(EngineError::NoComp)?;
    let target = comp.layer_groups.iter().find(|g| g.id == id).ok_or_else(|| bad("layer.groupAdd", "unknown target group"))?;
    let moving: Vec<_> = ids.into_iter().filter(|l| !target.members.contains(l)).map(|id| id.0).collect();
    if moving.is_empty() {
        return Ok(json!({}));
    }
    drop_layers(s, &json!({"comp":cid.0,"layers":moving,"into":id}))
}

fn remove(s: &mut Session, p: &Value) -> Result<Value> {
    let (cid, ids) = layers_p(s, p)?;
    let ids = unlocked(s, cid, ids, "layer.groupRemove")?;
    s.edit("Remove Layers from Group", None, |project, _| {
        let comp = project.comp_mut(cid).ok_or(EngineError::NoComp)?;
        // Release the selected members after their folder, preserving each stack order.
        let groups = comp.layer_groups.clone();
        let initial_ownership = owners(comp);
        let mut ownership = initial_ownership.clone();
        for group in &groups {
            let moving: Vec<_> = group.members.iter().filter(|id| ids.contains(id) && initial_ownership.get(id) == Some(&group.id)).copied().collect();
            if moving.is_empty() {
                continue;
            }
            if moving.len() != group.members.len() {
                let picked: Vec<_> = comp.layers.iter().filter(|l| moving.contains(&l.id)).cloned().collect();
                comp.layers.retain(|l| !moving.contains(&l.id));
                let at = group
                    .members
                    .iter()
                    .filter(|id| !moving.contains(id))
                    .filter_map(|id| position(comp, *id))
                    .max()
                    .map(|at| at.saturating_add(1))
                    .unwrap_or(comp.layers.len());
                splice_at(&mut comp.layers, at, picked);
            }
            for layer in moving {
                if let Some(parent) = group.parent {
                    ownership.insert(layer, parent);
                } else {
                    ownership.remove(&layer);
                }
            }
        }
        rebuild_members(comp, &ownership);
        valid_spans(comp, "layer.groupRemove")?;
        Ok(json!({}))
    })
}

/// Atomic stack drop with explicit ownership, including insertion at folder boundaries.
fn drop_layers(s: &mut Session, p: &Value) -> Result<Value> {
    let (cid, mut ids) = layers_p(s, p)?;
    let numeric_group = |key: &str| p.get(key).map(|v| v.as_u64().ok_or_else(|| bad("layer.drop", format!("{key} must be a numeric group id")))).transpose();
    let source = numeric_group("group")?;
    let into = numeric_group("into")?;
    let comp = s.project.comp(cid).ok_or(EngineError::NoComp)?;
    if let Some(id) = source {
        let group = comp.layer_groups.iter().find(|g| g.id == id).ok_or_else(|| bad("layer.drop", "unknown source group"))?;
        ids = group.members.clone();
        if ids.iter().any(|id| comp.layer_effectively_locked(*id)) {
            return Err(bad("layer.drop", "unlock all group members before moving the group"));
        }
        if into.is_some_and(|target| target == id || comp.group_contains_group(id, target)) {
            return Err(bad("layer.drop", "a group cannot contain itself or one of its ancestors"));
        }
    } else {
        ids = unlocked(s, cid, ids, "layer.drop")?;
    }
    let above =
        if let Some(value) = p.get("above") { Some(resolve_layer(comp, value).ok_or_else(|| bad("layer.drop", "unknown insertion layer"))?) } else { None };
    let target =
        if let Some(id) = into { Some(comp.layer_groups.iter().find(|g| g.id == id).ok_or_else(|| bad("layer.drop", "unknown target group"))?) } else { None };
    if into.is_some_and(|id| group_locked(comp, id)) {
        return Err(bad("layer.drop", "unlock the destination group first"));
    }
    if let Some(source) = source {
        let parent_depth =
            target.map_or(0, |g| comp.layer_group_path(g.members.first().copied().unwrap_or(LayerId(0))).iter().take_while(|p| p.id != g.id).count() + 1);
        let subtree_depth = ids.iter().map(|id| comp.layer_group_path(*id).iter().skip_while(|g| g.id != source).count()).max().unwrap_or(1);
        if parent_depth.saturating_add(subtree_depth) > effectcraft_project::MAX_GROUP_DEPTH {
            return Err(bad("layer.drop", "maximum group nesting depth reached"));
        }
    }
    if let Some(group) = target
        && above.is_some_and(|id| !group.members.contains(&id))
    {
        return Err(bad("layer.drop", "insertion layer must belong to the target group"));
    }
    if source.is_some() && above.is_some_and(|id| ids.contains(&id)) {
        return Ok(json!({}));
    }
    let anchor = if let Some(above) = above {
        position(comp, boundary(comp, above, into)).unwrap_or(comp.layers.len())
    } else if let Some(group) = target {
        group.members.last().and_then(|id| position(comp, *id)).map(|at| at.saturating_add(1)).unwrap_or(comp.layers.len())
    } else {
        comp.layers.len()
    };
    let at = comp.layers.iter().take(anchor).filter(|l| !ids.contains(&l.id)).count();
    s.edit("Move Layers", None, |project, _| {
        let comp = project.comp_mut(cid).ok_or(EngineError::NoComp)?;
        let mut ownership = owners(comp);
        let picked: Vec<_> = comp.layers.iter().filter(|l| ids.contains(&l.id)).cloned().collect();
        comp.layers.retain(|l| !ids.contains(&l.id));
        splice_at(&mut comp.layers, at, picked);
        if let Some(source) = source {
            if let Some(group) = comp.layer_groups.iter_mut().find(|g| g.id == source) {
                group.parent = into;
            }
        } else {
            for layer in &ids {
                if let Some(into) = into {
                    ownership.insert(*layer, into);
                } else {
                    ownership.remove(layer);
                }
            }
        }
        rebuild_members(comp, &ownership);
        valid_spans(comp, "layer.drop")?;
        Ok(json!({}))
    })
}

/// A complete folder moves as one stack unit. Partial selections leave their old
/// folder when moved outside it; inserting between members joins that folder.
pub(super) fn arrange_grouped(comp: &mut Comp, ids: &[LayerId], how: &str, index: Option<usize>, above: Option<LayerId>) -> Result<()> {
    let locked: Vec<_> =
        comp.layer_groups.iter().filter(|g| group_locked(comp, g.id)).map(|g| (g.id, g.members.iter().copied().collect::<HashSet<_>>())).collect();
    let mut ownership = owners(comp);
    let complete: HashSet<_> = comp.layer_groups.iter().filter(|g| g.members.iter().all(|id| ids.contains(id))).map(|g| g.id).collect();
    let retained: Vec<_> = comp
        .layer_groups
        .iter()
        .map(|g| (g.id, g.members.iter().filter(|id| complete.contains(&g.id) || !ids.contains(id)).copied().collect::<Vec<_>>()))
        .collect();
    if above.is_none() && index.is_none() && matches!(how, "forward" | "backward") {
        let mut units: Vec<Vec<Layer>> = Vec::new();
        let mut skip = std::collections::HashSet::new();
        for layer in &comp.layers {
            if !skip.insert(layer.id) {
                continue;
            }
            if let Some(group) = comp
                .layer_group_path(layer.id)
                .into_iter()
                .find(|g| g.members.iter().all(|id| ids.contains(id)) || g.members.iter().all(|id| !ids.contains(id)))
            {
                units.push(comp.layers.iter().filter(|l| group.members.contains(&l.id)).cloned().collect());
                skip.extend(&group.members);
            } else {
                units.push(vec![layer.clone()]);
            }
        }
        let picked = |unit: &[Layer]| unit.first().is_some_and(|l| ids.contains(&l.id));
        if how == "forward" {
            for i in 1..units.len() {
                if units.get(i).is_some_and(|u| picked(u)) && units.get(i.saturating_sub(1)).is_some_and(|u| !picked(u)) {
                    units.swap(i, i.saturating_sub(1));
                }
            }
        } else {
            for i in (0..units.len().saturating_sub(1)).rev() {
                if units.get(i).is_some_and(|u| picked(u)) && units.get(i.saturating_add(1)).is_some_and(|u| !picked(u)) {
                    units.swap(i, i.saturating_add(1));
                }
            }
        }
        comp.layers = units.into_iter().flatten().collect();
    } else {
        let moves_folder = comp.layer_groups.iter().any(|group| group.members.iter().all(|id| ids.contains(id)));
        let picked: Vec<_> = comp.layers.iter().filter(|l| ids.contains(&l.id)).cloned().collect();
        comp.layers.retain(|l| !ids.contains(&l.id));
        let mut at = if let Some(above) = above {
            position(comp, above).unwrap_or(comp.layers.len())
        } else if let Some(index) = index {
            index.saturating_sub(1).min(comp.layers.len())
        } else if how == "back" {
            comp.layers.len()
        } else {
            0
        };
        if moves_folder && let Some(group) = comp.layers.get(at).and_then(|l| comp.layer_group_path(l.id).into_iter().next()) {
            at = group.members.iter().filter_map(|id| position(comp, *id)).min().unwrap_or(at);
        }
        splice_at(&mut comp.layers, at, picked);
    }
    // A selected child folder leaving a parent keeps its own subtree; only its parent link changes.
    for _ in 0..effectcraft_project::MAX_GROUP_DEPTH {
        rebuild_members(comp, &ownership);
        let mut changed = false;
        let groups = comp.layer_groups.clone();
        for group in &groups {
            let positions: Vec<_> = comp.layers.iter().enumerate().filter(|(_, l)| group.members.contains(&l.id)).map(|(i, _)| i).collect();
            let contiguous =
                positions.first().zip(positions.last()).is_some_and(|(first, last)| last.saturating_sub(*first).saturating_add(1) == group.members.len());
            if contiguous || complete.contains(&group.id) {
                continue;
            }
            for child in &mut comp.layer_groups {
                if child.parent == Some(group.id) && complete.contains(&child.id) {
                    child.parent = group.parent;
                    changed = true;
                }
            }
            for layer in ids {
                if ownership.get(layer) == Some(&group.id) {
                    if let Some(parent) = group.parent {
                        ownership.insert(*layer, parent);
                    } else {
                        ownership.remove(layer);
                    }
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    rebuild_members(comp, &ownership);
    // Allow inserted leaves to join a span, but never silently clip existing members
    // when a mixed selection pushes one complete folder through another.
    comp.normalize_layer_groups();
    if locked.iter().any(|(id, members)| comp.layer_groups.iter().any(|g| g.id == *id && g.members.iter().any(|member| !members.contains(member)))) {
        return Err(bad("layer.arrange", "the destination group is locked"));
    }
    if retained.iter().any(|(id, members)| members.iter().any(|member| !comp.layer_groups.iter().any(|g| g.id == *id && g.members.contains(member)))) {
        return Err(bad("layer.arrange", "the move would split a group; use drag and drop to choose its destination"));
    }
    valid_spans(comp, "layer.arrange")
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!("layer.setActive", "Set Active Layer", [], None, "{layer: selected layer id}", has_comp, active),
        cmd!("layer.group", "Group Layers", ["Layer"], None, "{layers?, name?}; gathers at active layer", has_layers, create),
        cmd!("layer.ungroup", "Ungroup Layers", ["Layer"], None, "{layers?, group?}; keeps layers", has_comp, ungroup),
        cmd!("layer.groupUpdate", "Edit Layer Group", [], None, "{group, name?, collapsed?, video?, audio?, locked?, merge?}", has_comp, update),
        cmd!("layer.groupAdd", "Add Layers to Group", [], None, "{group, layers?}", has_layers, add),
        cmd!("layer.groupRemove", "Remove Layers from Group", [], None, "{layers?}", has_layers, remove),
        cmd!(
            "layer.drop",
            "Move Layers",
            [],
            None,
            "{layers?, group?, into?, above?}; atomic stack drop; group moves a whole folder, into joins a folder, above inserts before a layer",
            has_comp,
            drop_layers
        ),
    ]
}

/// Insert `picked` at stack index `at` (the layer list has no `splice`).
fn splice_at(layers: &mut effectcraft_project::Layers, at: usize, picked: Vec<Layer>) {
    let mut tail = layers.split_off(at.min(layers.len()));
    layers.extend(picked);
    layers.append(&mut tail);
}
