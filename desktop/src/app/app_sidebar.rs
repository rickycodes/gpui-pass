use super::*;

#[derive(Default)]
struct FolderTree {
    entries: Vec<usize>,
    children: BTreeMap<String, FolderTree>,
}

impl FolderTree {
    fn entry_count(&self) -> usize {
        self.entries.len()
            + self
                .children
                .values()
                .map(FolderTree::entry_count)
                .sum::<usize>()
    }
}

impl PassApp {
    pub(crate) fn filter(&mut self, cx: &mut Context<Self>) {
        self.state.sidebar_scroll.cancel();
        if self.state.saving.is_some() {
            return;
        }
        self.cancel_edit(cx);
        self.state.confirming_delete = None;
        self.state.session.hide();
        self.state.copy_pending = None;
        self.state.reveal = None;
        self.clear_message();
        let terms: Vec<_> = self
            .state
            .query
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        self.state.filtered = self
            .state
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, entry)| entry.matches_terms(&terms).then_some(i))
            .collect();
        let first = self
            .state
            .filtered
            .first()
            .map(|index| self.state.entries[*index].clone());
        self.state.session.select(first);
        self.rebuild_sidebar_rows();
        self.state
            .list_scroll
            .scroll_to_item(0, ScrollStrategy::Top);
    }

    fn add_folder_paths(folder: &str, folders: &mut HashSet<String>) {
        let mut path = String::new();
        for part in folder.split('/').filter(|part| !part.is_empty()) {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(part);
            folders.insert(path.clone());
        }
    }

    fn insert_entry(tree: &mut FolderTree, entry: &Entry, index: usize) {
        let mut node = tree;
        for part in entry.folder().split('/').filter(|part| !part.is_empty()) {
            node = node.children.entry(part.to_owned()).or_default();
        }
        node.entries.push(index);
    }

    fn flatten_sidebar_rows(
        tree: &FolderTree,
        prefix: &str,
        depth: usize,
        expanded: &HashSet<String>,
        rows: &mut Vec<SidebarRow>,
    ) {
        for index in &tree.entries {
            rows.push(SidebarRow::Entry {
                index: *index,
                depth,
            });
        }
        for (name, child) in &tree.children {
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            rows.push(SidebarRow::Folder {
                path: path.clone(),
                depth,
                count: child.entry_count(),
            });
            if expanded.contains(&path) {
                Self::flatten_sidebar_rows(child, &path, depth + 1, expanded, rows);
            }
        }
    }

    fn rebuild_sidebar_rows(&mut self) {
        let mut tree = FolderTree::default();
        for index in &self.state.filtered {
            Self::insert_entry(&mut tree, &self.state.entries[*index], *index);
        }
        if !self.state.query.trim().is_empty() {
            for index in &self.state.filtered {
                Self::add_folder_paths(
                    self.state.entries[*index].folder(),
                    &mut self.state.expanded_folders,
                );
            }
        }
        let previous = self
            .state
            .sidebar_rows
            .get(self.state.sidebar_cursor)
            .cloned();
        let mut rows = Vec::new();
        Self::flatten_sidebar_rows(&tree, "", 0, &self.state.expanded_folders, &mut rows);
        self.state.sidebar_rows = rows;
        self.state.sidebar_cursor = previous
            .and_then(|row| {
                self.state
                    .sidebar_rows
                    .iter()
                    .position(|candidate| *candidate == row)
            })
            .or_else(|| {
                self.state.session.selected().and_then(|selected| {
                    self.state.sidebar_rows.iter().position(|row| {
                        matches!(
                            row,
                            SidebarRow::Entry { index, .. }
                                if self.state.entries[*index] == *selected
                        )
                    })
                })
            })
            .or_else(|| {
                self.state
                    .sidebar_rows
                    .iter()
                    .position(|row| matches!(row, SidebarRow::Entry { .. }))
            })
            .unwrap_or(0);
    }

    pub(crate) fn toggle_folder(&mut self, path: &str, cx: &mut Context<Self>) {
        if self.state.expanded_folders.contains(path) {
            self.state.expanded_folders.remove(path);
        } else {
            self.state.expanded_folders.insert(path.to_owned());
        }
        self.rebuild_sidebar_rows();
        self.state.sidebar_cursor = self
            .state
            .sidebar_rows
            .iter()
            .position(
                |row| matches!(row, SidebarRow::Folder { path: row_path, .. } if row_path == path),
            )
            .unwrap_or(self.state.sidebar_cursor);
        cx.notify();
    }

    pub(crate) fn move_sidebar_horizontally(
        &mut self,
        delta: isize,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.state.custom_entry_name_focus.is_focused(window)
            || self.state.composer_content_focus.is_focused(window)
            || !self.state.query.is_empty()
        {
            return false;
        }
        let Some(row) = self
            .state
            .sidebar_rows
            .get(self.state.sidebar_cursor)
            .cloned()
        else {
            return false;
        };
        let path = match row {
            SidebarRow::Folder { path, .. } => path,
            SidebarRow::Entry { index, .. } => self.state.entries[index].folder().to_owned(),
        };
        if path.is_empty() {
            return true;
        }
        if delta < 0 {
            self.state.expanded_folders.remove(&path);
        } else {
            self.state.expanded_folders.insert(path.clone());
        }
        self.rebuild_sidebar_rows();
        self.state.sidebar_cursor = self
            .state
            .sidebar_rows
            .iter()
            .position(|row| match row {
                SidebarRow::Folder { path: row_path, .. } => row_path == &path,
                SidebarRow::Entry { index, .. } => self.state.entries[*index].folder() == path,
            })
            .unwrap_or(self.state.sidebar_cursor);
        cx.notify();
        true
    }
}
