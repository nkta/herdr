use super::*;

fn test_server_with_workspaces(labels: &[&str]) -> HeadlessServer {
    let mut server = test_headless_server();
    server.app.state.workspaces = labels
        .iter()
        .map(|label| crate::workspace::Workspace::test_new(label))
        .collect();
    server.app.state.active = Some(0);
    server.app.state.selected = 0;
    server
}

fn insert_test_shell_client(server: &mut HeadlessServer, client_id: u64) {
    server.clients.insert(
        client_id,
        ClientConnection::new(
            (80, 24),
            crate::kitty_graphics::HostCellSize::default(),
            1,
            RenderEncoding::SemanticFrame,
            None,
        ),
    );
}

#[test]
fn last_watcher_leaving_clears_workspace_demand() {
    let mut server = test_server_with_workspaces(&["one"]);
    let workspace_id = server.app.state.workspaces[0].id.clone();
    insert_test_shell_client(&mut server, 1);
    insert_test_shell_client(&mut server, 2);

    assert!(server.set_git_panel_watch(1, workspace_id.clone(), true));
    assert!(server.app.state.workspaces[0].git_panel_demand);
    assert!(!server.set_git_panel_watch(2, workspace_id.clone(), true));

    assert!(!server.set_git_panel_watch(1, workspace_id.clone(), false));
    assert!(server.app.state.workspaces[0].git_panel_demand);

    assert!(server.set_git_panel_watch(2, workspace_id, false));
    assert!(!server.app.state.workspaces[0].git_panel_demand);
}

#[test]
fn switching_watched_workspace_releases_the_previous_one() {
    let mut server = test_server_with_workspaces(&["one", "two"]);
    let first = server.app.state.workspaces[0].id.clone();
    let second = server.app.state.workspaces[1].id.clone();
    insert_test_shell_client(&mut server, 1);

    assert!(server.set_git_panel_watch(1, first, true));
    assert!(server.set_git_panel_watch(1, second, true));

    assert!(!server.app.state.workspaces[0].git_panel_demand);
    assert!(server.app.state.workspaces[1].git_panel_demand);
}

#[test]
fn reactivating_the_same_workspace_is_a_no_op() {
    let mut server = test_server_with_workspaces(&["one"]);
    let workspace_id = server.app.state.workspaces[0].id.clone();
    insert_test_shell_client(&mut server, 1);

    assert!(server.set_git_panel_watch(1, workspace_id.clone(), true));
    assert!(!server.set_git_panel_watch(1, workspace_id, true));
}

#[test]
fn disconnecting_without_deactivating_releases_the_watch() {
    let mut server = test_server_with_workspaces(&["one"]);
    let workspace_id = server.app.state.workspaces[0].id.clone();
    insert_test_shell_client(&mut server, 1);
    server.set_git_panel_watch(1, workspace_id, true);

    server.remove_client(1);

    assert!(!server.app.state.workspaces[0].git_panel_demand);
}
