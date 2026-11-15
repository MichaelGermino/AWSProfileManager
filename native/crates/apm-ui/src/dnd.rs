//! Drag-and-drop on the Profiles screen (1.x `Profiles.tsx` with @dnd-kit): profiles reorder within
//! and between folders, folders reorder. The pure parts live here (where a drop lands, the new
//! order to persist, the keyboard drop positions); the wiring is in `profiles.rs`.
//!
//! Persisted as in 1.x: `profiles:applyLayout` with the whole order plus the moved profile's folder,
//! rebuilt through the folder grouping so each folder's members stay contiguous;
//! `folders:reorder` with the folder ids.

use std::collections::HashSet;

use apm_core::grouping::{flatten_grouping, group_profiles_by_folder};
use gpui::{div, prelude::*, px, Context, IntoElement, SharedString, Window};
use serde_json::Value;

use crate::theme::{size, Palette};
use crate::widgets::icon;

fn id(v: &Value) -> &str {
    v.get("id").and_then(Value::as_str).unwrap_or("")
}

/// What is being dragged.
#[derive(Debug, Clone, PartialEq)]
pub enum DragItem {
    Profile(String),
    Folder(String),
}

/// Where in a folder a profile goes.
#[derive(Debug, Clone, PartialEq)]
pub enum Place {
    Before(String),
    After(String),
    End,
}

/// A drop position for a profile: its folder (`None`: no folder) and the place in it.
pub type Slot = (Option<String>, Place);

/// One section of the list as shown: a folder (or `None`, the profiles with no folder) and its rows.
#[derive(Debug, Clone)]
pub struct Section {
    pub folder: Option<Value>,
    pub rows: Vec<Value>,
}

impl Section {
    pub fn folder_id(&self) -> Option<String> {
        self.folder.as_ref().map(|f| id(f).to_string())
    }
}

/// A keyboard drag in progress (1.x dnd-kit KeyboardSensor: Space picks up, arrows move, Space or
/// Enter drops, Escape cancels).
#[derive(Debug, Clone)]
pub struct KbDrag {
    pub item: DragItem,
    /// Profiles: every position the arrows step through, fixed at pick-up. Folders: unused (the
    /// position is a folder index).
    pub slots: Vec<Slot>,
    pub slot: usize,
    /// Where it started, so dropping there changes nothing.
    pub origin: usize,
}

/// The positions a dragged profile can take, in screen order: before each row of each section, and
/// at its end. A collapsed folder is one position (into it).
pub fn profile_slots(sections: &[Section], collapsed: &HashSet<String>, dragged: &str) -> Vec<Slot> {
    let mut out = Vec::new();
    for sec in sections {
        let fid = sec.folder_id();
        if fid.as_ref().is_some_and(|f| collapsed.contains(f)) {
            out.push((fid, Place::End));
            continue;
        }
        for r in sec.rows.iter().filter(|r| id(r) != dragged) {
            out.push((fid.clone(), Place::Before(id(r).to_string())));
        }
        out.push((fid, Place::End));
    }
    out
}

/// Where the dragged profile is now, as a slot.
pub fn slot_of(sections: &[Section], dragged: &str) -> Option<Slot> {
    for sec in sections {
        if let Some(i) = sec.rows.iter().position(|r| id(r) == dragged) {
            let place = sec.rows.get(i + 1).map(|r| Place::Before(id(r).to_string())).unwrap_or(Place::End);
            return Some((sec.folder_id(), place));
        }
    }
    None
}

/// The list with the dragged profile shown at `slot` (the keyboard drag previews its move).
pub fn preview_profile(sections: &mut [Section], dragged: &str, slot: &Slot) {
    let mut moved = None;
    for sec in sections.iter_mut() {
        if let Some(i) = sec.rows.iter().position(|r| id(r) == dragged) {
            moved = Some(sec.rows.remove(i));
        }
    }
    let Some(row) = moved else { return };
    let Some(target) = sections.iter_mut().find(|s| s.folder_id() == slot.0) else { return };
    let at = match &slot.1 {
        Place::Before(x) => target.rows.iter().position(|r| id(r) == x).unwrap_or(target.rows.len()),
        Place::After(x) => target.rows.iter().position(|r| id(r) == x).map(|i| i + 1).unwrap_or(target.rows.len()),
        Place::End => target.rows.len(),
    };
    target.rows.insert(at, row);
}

/// The list with the dragged folder shown at folder position `to`.
pub fn preview_folder(sections: &mut Vec<Section>, dragged: &str, to: usize) {
    let folders = sections.iter().filter(|s| s.folder.is_some()).count();
    let Some(from) = sections.iter().position(|s| s.folder_id().as_deref() == Some(dragged)) else { return };
    let sec = sections.remove(from);
    sections.insert(to.min(folders.saturating_sub(1)), sec);
}

/// The new profile order after moving `moved` into `folder` at `place` (1.x `handleDragEnd`).
pub fn apply_profile_move(profiles: &[Value], folders: &[Value], moved: &str, folder: Option<&str>, place: &Place) -> Vec<Value> {
    let mut grouped = group_profiles_by_folder(profiles, folders);
    let mut row = None;
    for bucket in std::iter::once(&mut grouped.ungrouped).chain(grouped.groups.iter_mut().map(|g| &mut g.profiles)) {
        if let Some(i) = bucket.iter().position(|p| id(p) == moved) {
            row = Some(bucket.remove(i));
        }
    }
    let Some(mut row) = row else { return profiles.to_vec() };
    let dest = match folder.and_then(|f| grouped.groups.iter().position(|g| id(&g.folder) == f)) {
        Some(i) => {
            row["folderId"] = Value::String(folder.unwrap_or_default().to_string());
            &mut grouped.groups[i].profiles
        }
        None => {
            if let Some(o) = row.as_object_mut() {
                o.remove("folderId");
            }
            &mut grouped.ungrouped
        }
    };
    let at = match place {
        Place::Before(x) => dest.iter().position(|p| id(p) == x).unwrap_or(dest.len()),
        Place::After(x) => dest.iter().position(|p| id(p) == x).map(|i| i + 1).unwrap_or(dest.len()),
        Place::End => dest.len(),
    };
    dest.insert(at, row);
    flatten_grouping(&grouped)
}

/// A profile dropped on another profile's row takes that row's place: before it when coming from
/// above or from another folder, after it when moving down within the folder (1.x arrayMove).
pub fn place_on_row(profiles: &[Value], folders: &[Value], dragged: &str, target: &str) -> Place {
    let grouped = group_profiles_by_folder(profiles, folders);
    for bucket in std::iter::once(&grouped.ungrouped).chain(grouped.groups.iter().map(|g| &g.profiles)) {
        let from = bucket.iter().position(|p| id(p) == dragged);
        let to = bucket.iter().position(|p| id(p) == target);
        if let (Some(f), Some(t)) = (from, to) {
            return if f < t { Place::After(target.to_string()) } else { Place::Before(target.to_string()) };
        }
    }
    Place::Before(target.to_string())
}

/// The folders with `moved` at position `to`.
pub fn reorder_folders(folders: &[Value], moved: &str, to: usize) -> Vec<Value> {
    let mut out = folders.to_vec();
    if let Some(from) = out.iter().position(|f| id(f) == moved) {
        let f = out.remove(from);
        out.insert(to.min(out.len()), f);
    }
    out
}

// -------------------------------------------------------------------------------------------------
// Mouse drag payloads and the preview that follows the pointer
// -------------------------------------------------------------------------------------------------

#[derive(Clone)]
pub struct DraggedProfile {
    pub id: String,
    pub name: String,
    /// "" when in no folder.
    pub folder: String,
    /// Position in the stored order, to draw the drop line above or below a row.
    pub order: usize,
    pub palette: Palette,
}

#[derive(Clone)]
pub struct DraggedFolder {
    pub id: String,
    pub name: String,
    pub palette: Palette,
}

pub struct DragPreview {
    label: SharedString,
    icon: &'static str,
    palette: Palette,
}

impl DragPreview {
    pub fn new(label: impl Into<SharedString>, icon: &'static str, palette: Palette) -> Self {
        DragPreview { label: label.into(), icon, palette }
    }
}

impl Render for DragPreview {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette;
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(36.))
            .px(px(12.))
            .rounded(px(size::RADIUS))
            .border_1()
            .border_color(p.accent)
            .bg(p.panel)
            .shadow_lg()
            .text_size(px(size::TEXT_SM))
            .text_color(p.text)
            .child(icon("grip-vertical", 14., p.text_muted))
            .child(icon(self.icon, 14., p.text_muted))
            .child(self.label.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ids(v: &[Value]) -> Vec<&str> {
        v.iter().map(id).collect()
    }

    fn fixture() -> (Vec<Value>, Vec<Value>) {
        let profiles = vec![
            json!({ "id": "p1", "folderId": "f1" }),
            json!({ "id": "p2", "folderId": "f2" }),
            json!({ "id": "p3", "folderId": "f1" }),
        ];
        let folders = vec![json!({ "id": "f1" }), json!({ "id": "f2" })];
        (profiles, folders)
    }

    #[test]
    fn keyboard_up_one_slot() {
        let (profiles, folders) = fixture();
        let sections = vec![
            Section { folder: Some(folders[0].clone()), rows: vec![profiles[0].clone(), profiles[2].clone()] },
            Section { folder: Some(folders[1].clone()), rows: vec![profiles[1].clone()] },
            Section { folder: None, rows: vec![] },
        ];
        let slots = profile_slots(&sections, &HashSet::new(), "p3");
        let at = slots.iter().position(|s| Some(s) == slot_of(&sections, "p3").as_ref()).unwrap();
        assert_eq!(at, 1);
        let (f, place) = slots[at - 1].clone();
        let out = apply_profile_move(&profiles, &folders, "p3", f.as_deref(), &place);
        assert_eq!(ids(&out), ["p3", "p1", "p2"]);
        assert_eq!(out[0]["folderId"], "f1");
    }

    #[test]
    fn drop_on_row_and_between_folders() {
        let (profiles, folders) = fixture();
        // Down within the folder: after the target.
        assert_eq!(place_on_row(&profiles, &folders, "p1", "p3"), Place::After("p3".into()));
        let out = apply_profile_move(&profiles, &folders, "p1", Some("f1"), &Place::After("p3".into()));
        assert_eq!(ids(&out), ["p3", "p1", "p2"]);
        // Into another folder, then out of every folder.
        let out = apply_profile_move(&profiles, &folders, "p1", Some("f2"), &Place::End);
        assert_eq!(ids(&out), ["p3", "p2", "p1"]);
        let out = apply_profile_move(&profiles, &folders, "p1", None, &Place::End);
        assert_eq!(ids(&out), ["p3", "p2", "p1"]);
        assert!(out[2].get("folderId").is_none());
        assert_eq!(ids(&reorder_folders(&folders, "f2", 0)), ["f2", "f1"]);
    }
}
