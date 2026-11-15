//! Partition profiles into folders for display. Port of `src/shared/profileGrouping.ts`.
//!
//! The flat profile list stays the canonical order; folders are a render-time partition. A profile
//! whose `folderId` names a folder that no longer exists falls into `ungrouped` instead of
//! vanishing (the self-heal for dangling ids after a restore or a hand edit).

use serde_json::Value;

/// Anything with an optional folder id (a profile or a dashboard summary).
pub trait Foldered {
    fn folder_id(&self) -> Option<&str>;
}

/// Anything with an id (a folder).
pub trait Identified {
    fn id(&self) -> &str;
}

impl Foldered for Value {
    fn folder_id(&self) -> Option<&str> {
        self.get("folderId").and_then(Value::as_str)
    }
}

impl Identified for Value {
    fn id(&self) -> &str {
        self.get("id").and_then(Value::as_str).unwrap_or("")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FolderGroup<F, P> {
    pub folder: F,
    pub profiles: Vec<P>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Grouped<F, P> {
    /// In folder order; every folder appears, including empty ones.
    pub groups: Vec<FolderGroup<F, P>>,
    /// Profiles with no (existing) folder, in profile order.
    pub ungrouped: Vec<P>,
}

pub fn group_profiles_by_folder<F: Identified + Clone, P: Foldered + Clone>(profiles: &[P], folders: &[F]) -> Grouped<F, P> {
    let mut groups: Vec<FolderGroup<F, P>> =
        folders.iter().map(|f| FolderGroup { folder: f.clone(), profiles: Vec::new() }).collect();
    let mut ungrouped = Vec::new();
    for profile in profiles {
        // JavaScript `Map` built from the folders: a duplicate folder id maps to the LAST group.
        let target = profile
            .folder_id()
            .filter(|id| !id.is_empty())
            .and_then(|id| groups.iter().rposition(|g| g.folder.id() == id));
        match target {
            Some(i) => groups[i].profiles.push(profile.clone()),
            None => ungrouped.push(profile.clone()),
        }
    }
    Grouped { groups, ungrouped }
}

/// Folders first (in folder order, members contiguous), then ungrouped: the order
/// `profiles:applyLayout` persists after a drag.
pub fn flatten_grouping<F, P: Clone>(grouped: &Grouped<F, P>) -> Vec<P> {
    grouped
        .groups
        .iter()
        .flat_map(|g| g.profiles.iter().cloned())
        .chain(grouped.ungrouped.iter().cloned())
        .collect()
}
