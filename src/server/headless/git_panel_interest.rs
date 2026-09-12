use super::*;

impl HeadlessServer {
    /// Applies a `git.panel.set_active` request: records which workspace (if any) `client_id`
    /// is watching, and updates the aggregated per-workspace demand that gates the background
    /// working-tree refresh (`App::set_git_panel_demand`). A client watches at most one
    /// workspace at a time; switching workspaces implicitly releases the previous one.
    ///
    /// Returns whether the aggregated demand changed for any workspace.
    pub(super) fn set_git_panel_watch(
        &mut self,
        client_id: u64,
        workspace_id: String,
        active: bool,
    ) -> bool {
        let Some(client) = self.clients.get_mut(&client_id) else {
            return false;
        };
        let previous = client.git_panel_open_workspace_id.clone();
        client.git_panel_open_workspace_id = active.then(|| workspace_id.clone());

        let mut changed = false;
        if let Some(previous) = previous {
            if !active || previous != workspace_id {
                changed |= self.release_git_panel_watch(client_id, &previous);
            }
        }
        if active {
            changed |= self.acquire_git_panel_watch(client_id, &workspace_id);
        }
        changed
    }

    /// Releases whatever git panel `client_id` was watching. Called when the connection drops
    /// without an explicit `git.panel.set_active { active: false }`.
    pub(super) fn release_git_panel_watch_on_disconnect(&mut self, client_id: u64) {
        if let Some(workspace_id) = self
            .clients
            .get_mut(&client_id)
            .and_then(|client| client.git_panel_open_workspace_id.take())
        {
            self.release_git_panel_watch(client_id, &workspace_id);
        }
    }

    fn acquire_git_panel_watch(&mut self, client_id: u64, workspace_id: &str) -> bool {
        let watchers = self
            .git_panel_watchers
            .entry(workspace_id.to_owned())
            .or_default();
        let was_empty = watchers.is_empty();
        watchers.insert(client_id);
        was_empty && self.app.set_git_panel_demand(workspace_id, true)
    }

    fn release_git_panel_watch(&mut self, client_id: u64, workspace_id: &str) -> bool {
        let Some(watchers) = self.git_panel_watchers.get_mut(workspace_id) else {
            return false;
        };
        watchers.remove(&client_id);
        if watchers.is_empty() {
            self.git_panel_watchers.remove(workspace_id);
            self.app.set_git_panel_demand(workspace_id, false)
        } else {
            false
        }
    }
}
