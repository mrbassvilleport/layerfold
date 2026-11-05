//! Nested organizational folders, independent of transform parenting and blend isolation.
use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use effectcraft_time::Tick;

use crate::{Comp, LayerId};

// Reserve allocation headroom when restoring ids from an untrusted project file.
pub const MAX_GROUP_ID: u64 = i64::MAX as u64;
pub const MAX_GROUP_DEPTH: usize = 32;

fn enabled() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayerGroup {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub collapsed: bool,
    /// All descendant leaf layers, in composition stack order.
    pub members: Vec<LayerId>,
    #[serde(default)]
    pub parent: Option<u64>,
    #[serde(default = "enabled")]
    pub video: bool,
    #[serde(default = "enabled")]
    pub audio: bool,
    #[serde(default)]
    pub locked: bool,
}

impl Default for LayerGroup {
    fn default() -> Self {
        Self { id: 0, name: String::new(), collapsed: false, members: Vec::new(), parent: None, video: true, audio: true, locked: false }
    }
}

fn ancestry(id: u64, parents: &HashMap<u64, Option<u64>>) -> Option<Vec<u64>> {
    let mut path = Vec::new();
    let mut next = Some(id);
    while let Some(id) = next {
        if path.len() >= MAX_GROUP_DEPTH || path.contains(&id) {
            return None;
        }
        path.push(id);
        next = *parents.get(&id)?;
    }
    Some(path)
}

impl Comp {
    /// The innermost folder containing a layer.
    pub fn layer_group(&self, layer: LayerId) -> Option<&LayerGroup> {
        if self.layer_groups.is_empty() {
            return None;
        }
        let parents: HashMap<_, _> = self.layer_groups.iter().map(|g| (g.id, g.parent)).collect();
        self.layer_groups.iter().filter(|g| g.members.contains(&layer)).max_by_key(|g| ancestry(g.id, &parents).map_or(0, |p| p.len()))
    }

    /// Folder ancestry from the root to the innermost owner, bounded for damaged input.
    pub fn layer_group_path(&self, layer: LayerId) -> Vec<&LayerGroup> {
        let Some(owner) = self.layer_group(layer) else { return Vec::new() };
        let groups: HashMap<_, _> = self.layer_groups.iter().map(|g| (g.id, g)).collect();
        let mut path = Vec::new();
        let mut next = Some(owner.id);
        while let Some(id) = next {
            if path.len() >= MAX_GROUP_DEPTH || path.iter().any(|g: &&LayerGroup| g.id == id) {
                break;
            }
            let Some(g) = groups.get(&id).copied() else { break };
            path.push(g);
            next = g.parent;
        }
        path.reverse();
        path
    }

    pub fn group_contains_group(&self, ancestor: u64, descendant: u64) -> bool {
        if ancestor == descendant {
            return false;
        }
        let parents: HashMap<_, _> = self.layer_groups.iter().map(|g| (g.id, g.parent)).collect();
        ancestry(descendant, &parents).is_some_and(|p| p.into_iter().skip(1).any(|id| id == ancestor))
    }

    /// The folder itself followed by its descendants in stored order.
    pub fn group_descendants(&self, group: u64) -> Vec<u64> {
        let parents: HashMap<_, _> = self.layer_groups.iter().map(|g| (g.id, g.parent)).collect();
        if !parents.contains_key(&group) {
            return Vec::new();
        }
        std::iter::once(group)
            .chain(self.layer_groups.iter().filter(|g| g.id != group && ancestry(g.id, &parents).is_some_and(|p| p.contains(&group))).map(|g| g.id))
            .collect()
    }

    /// The folder gate also applies to matte sources, whose own Video switch is ignored.
    pub fn layer_group_video_enabled(&self, layer: LayerId) -> bool {
        self.layer_groups.iter().all(|g| g.video || !g.members.contains(&layer))
    }

    pub fn layer_group_audio_enabled(&self, layer: LayerId) -> bool {
        self.layer_groups.iter().all(|g| g.audio || !g.members.contains(&layer))
    }

    pub fn layer_effectively_visible(&self, layer: LayerId) -> bool {
        self.layer(layer).is_some_and(|l| l.switches.video) && self.layer_group_video_enabled(layer)
    }

    pub fn layer_effectively_audio(&self, layer: LayerId) -> bool {
        self.layer(layer).is_some_and(|l| l.switches.audio) && self.layer_group_audio_enabled(layer)
    }

    pub fn layer_effectively_locked(&self, layer: LayerId) -> bool {
        self.layer(layer).is_some_and(|l| l.switches.locked) || self.layer_groups.iter().any(|g| g.locked && g.members.contains(&layer))
    }

    /// Time a folder's layers cover, merged into sorted disjoint `[start, end)` segments. Every
    /// member counts regardless of its switches; `exclude` drops layers in a running gesture.
    pub fn layer_group_spans(&self, group: u64, exclude: &[LayerId]) -> Vec<(Tick, Tick)> {
        let Some(g) = self.layer_groups.iter().find(|g| g.id == group) else { return Vec::new() };
        let mut spans: Vec<(Tick, Tick)> = g
            .members
            .iter()
            .filter(|id| !exclude.contains(id))
            .filter_map(|id| self.layer(*id))
            .filter(|l| l.in_point < l.out_point)
            .map(|l| (l.in_point, l.out_point))
            .collect();
        spans.sort();
        let mut merged: Vec<(Tick, Tick)> = Vec::with_capacity(spans.len());
        for (start, end) in spans {
            match merged.last_mut() {
                Some(last) if start <= last.1 => last.1 = last.1.max(end),
                _ => merged.push((start, end)),
            }
        }
        merged
    }

    /// Repair invalid ancestry and crossing folders without changing the layer stack. Newly
    /// inserted layers between a folder's endpoints join it and all its ancestors.
    pub fn normalize_layer_groups(&mut self) -> bool {
        if self.layer_groups_are_valid() {
            return false;
        }
        self.layer_groups.truncate(self.layers.len().saturating_mul(MAX_GROUP_DEPTH).min(10_000));
        let positions: HashMap<_, _> = self.layers.iter().enumerate().map(|(i, l)| (l.id, i)).collect();
        let mut ids = HashSet::new();
        self.layer_groups.retain_mut(|g| {
            if g.id == 0 || g.id >= MAX_GROUP_ID || !ids.insert(g.id) {
                return false;
            }
            g.name = g.name.trim().chars().take(256).collect();
            if g.name.is_empty() {
                g.name = "Group".into();
            }
            let mut seen = HashSet::new();
            g.members.retain(|id| positions.contains_key(id) && seen.insert(*id));
            true
        });
        let mut parents: HashMap<_, _> = self.layer_groups.iter().map(|g| (g.id, g.parent)).collect();
        for g in &mut self.layer_groups {
            if ancestry(g.id, &parents).is_none() {
                g.parent = None;
                parents.insert(g.id, None);
            }
        }
        let paths: HashMap<_, _> = self.layer_groups.iter().map(|g| (g.id, ancestry(g.id, &parents).unwrap_or_default())).collect();
        let related = |a: u64, b: u64| paths.get(&a).is_some_and(|p| p.contains(&b)) || paths.get(&b).is_some_and(|p| p.contains(&a));
        let mut owners: HashMap<LayerId, Vec<u64>> = HashMap::new();
        for g in &mut self.layer_groups {
            g.members.retain(|id| {
                let owners = owners.entry(*id).or_default();
                if owners.iter().any(|owner| !related(*owner, g.id)) {
                    return false;
                }
                owners.push(g.id);
                true
            });
        }
        // A parent's saved span may be incomplete; a live child still belongs to it.
        for g in &self.layer_groups {
            if let Some(path) = paths.get(&g.id) {
                for id in &g.members {
                    let owners = owners.entry(*id).or_default();
                    for ancestor in path {
                        if !owners.contains(ancestor) {
                            owners.push(*ancestor);
                        }
                    }
                }
            }
        }
        let mut bounds: HashMap<u64, (usize, usize)> = HashMap::new();
        for (id, groups) in &owners {
            let Some(pos) = positions.get(id).copied() else { continue };
            for group in groups {
                bounds
                    .entry(*group)
                    .and_modify(|(first, last)| {
                        *first = (*first).min(pos);
                        *last = (*last).max(pos);
                    })
                    .or_insert((pos, pos));
            }
        }
        for g in &mut self.layer_groups {
            let Some((first, last)) = bounds.get(&g.id).copied() else {
                g.members.clear();
                continue;
            };
            g.members = self
                .layers
                .iter()
                .enumerate()
                .skip(first)
                .take_while(|(i, l)| *i <= last && owners.get(&l.id).is_none_or(|v| v.iter().all(|owner| related(*owner, g.id))))
                .map(|(_, l)| l.id)
                .collect();
        }
        // Clip children to every ancestor's repaired span before dropping empty folders.
        let spans: HashMap<_, _> = self
            .layer_groups
            .iter()
            .filter_map(|g| {
                let first = g.members.first().and_then(|id| positions.get(id)).copied()?;
                Some((g.id, (first, first.saturating_add(g.members.len()))))
            })
            .collect();
        for g in &mut self.layer_groups {
            if let Some(path) = paths.get(&g.id) {
                g.members.retain(|id| {
                    positions
                        .get(id)
                        .is_some_and(|pos| path.iter().skip(1).all(|ancestor| spans.get(ancestor).is_some_and(|(first, end)| *first <= *pos && *pos < *end)))
                });
            }
        }
        self.layer_groups.retain(|g| !g.members.is_empty());
        let live: HashSet<_> = self.layer_groups.iter().map(|g| g.id).collect();
        for g in &mut self.layer_groups {
            if g.parent.is_some_and(|id| !live.contains(&id)) {
                g.parent = None;
            }
        }
        true
    }

    pub fn layer_groups_are_valid(&self) -> bool {
        if self.layer_groups.is_empty() {
            return true;
        }
        if self.layer_groups.len() > self.layers.len().saturating_mul(MAX_GROUP_DEPTH).min(10_000) {
            return false;
        }
        let positions: HashMap<_, _> = self.layers.iter().enumerate().map(|(i, l)| (l.id, i)).collect();
        let parents: HashMap<_, _> = self.layer_groups.iter().map(|g| (g.id, g.parent)).collect();
        let paths: HashMap<_, _> = self.layer_groups.iter().filter_map(|g| ancestry(g.id, &parents).map(|p| (g.id, p))).collect();
        let spans: HashMap<_, _> = self
            .layer_groups
            .iter()
            .filter_map(|g| {
                let first = g.members.first().and_then(|id| positions.get(id)).copied()?;
                Some((g.id, (first, first.saturating_add(g.members.len()))))
            })
            .collect();
        let mut ids = HashSet::new();
        let mut owners: HashMap<LayerId, Vec<u64>> = HashMap::new();
        self.layer_groups.iter().all(|g| {
            if g.id == 0
                || g.id >= MAX_GROUP_ID
                || !ids.insert(g.id)
                || !paths.contains_key(&g.id)
                || g.name.is_empty()
                || g.name.trim() != g.name
                || g.name.chars().count() > 256
                || g.members.is_empty()
            {
                return false;
            }
            let Some(first) = g.members.first().and_then(|id| positions.get(id).copied()) else { return false };
            let end = first.saturating_add(g.members.len());
            if g.parent.is_some_and(|parent| !spans.get(&parent).is_some_and(|(a, b)| *a <= first && end <= *b)) {
                return false;
            }
            let contiguous = self
                .layers
                .as_arcs()
                .get(first..first.saturating_add(g.members.len()))
                .is_some_and(|layers| layers.iter().zip(&g.members).all(|(l, id)| l.id == *id));
            contiguous
                && g.members.iter().all(|id| {
                    let prior = owners.entry(*id).or_default();
                    let compatible =
                        prior.iter().all(|owner| paths.get(owner).is_some_and(|p| p.contains(&g.id)) || paths.get(&g.id).is_some_and(|p| p.contains(owner)));
                    prior.push(g.id);
                    compatible
                })
        })
    }
}
