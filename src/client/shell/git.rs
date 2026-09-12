use super::*;

impl ClientShellState {
    pub(super) fn toggle_sidebar_git_view(&mut self, outcome: &mut ClientShellInput) {
        let next = match self.sidebar_view {
            SidebarSpacesView::Spaces => SidebarSpacesView::Git,
            SidebarSpacesView::Git => SidebarSpacesView::Spaces,
        };
        self.set_sidebar_view(next, outcome);
    }

    pub(super) fn set_sidebar_view(
        &mut self,
        view: SidebarSpacesView,
        outcome: &mut ClientShellInput,
    ) {
        if self.sidebar_view == view {
            return;
        }
        self.sidebar_view = view;
        self.git_panel = ClientGitPanelState::default();
        outcome.repaint = true;
        self.sync_git_panel_watch(outcome);
    }

    /// Tells the server whether the focused workspace's git panel is currently visible in this
    /// client, so the background working-tree refresh only runs while someone can see it.
    fn sync_git_panel_watch(&mut self, outcome: &mut ClientShellInput) {
        let Some(workspace_id) = self
            .snapshot
            .as_deref()
            .and_then(|snapshot| snapshot.focused_workspace_id.clone())
        else {
            return;
        };
        let active = self.sidebar_view == SidebarSpacesView::Git;
        self.push_endpoint_method_with_kind(
            crate::api::schema::Method::GitPanelSetActive(
                crate::api::schema::GitPanelSetActiveParams {
                    workspace_id,
                    active,
                },
            ),
            PendingEndpointKind::Generic,
            outcome,
        );
    }
}
