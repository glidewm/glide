use std::iter;

use serde::{Deserialize, Serialize};

use super::layout_tree::TreeEvent;
use super::tree::{NodeId, NodeMap};

#[derive(Default, Serialize, Deserialize)]
pub struct Selection {
    nodes: slotmap::SecondaryMap<NodeId, SelectionInfo>,
}

#[derive(Serialize, Deserialize)]
struct SelectionInfo {
    selected_child: NodeId,
    stop_here: bool,
}

impl Selection {
    pub(super) fn current_selection(&self, root: NodeId) -> NodeId {
        let mut node = root;
        while let Some(info) = self.nodes.get(node) {
            if info.stop_here {
                break;
            }
            node = info.selected_child;
        }
        node
    }

    pub(super) fn last_selection(&self, _map: &NodeMap, node: NodeId) -> Option<NodeId> {
        self.nodes.get(node).map(|info| info.selected_child)
    }

    pub(super) fn local_selection(&self, map: &NodeMap, node: NodeId) -> Option<NodeId> {
        let result = self.nodes.get(node);
        if let Some(result) = result {
            debug_assert_eq!(result.selected_child.parent(map), Some(node));
        }
        result.filter(|info| !info.stop_here).map(|info| info.selected_child)
    }

    /// Select the node within its parent.
    ///
    /// Returns whether the selection changed.
    pub(super) fn select_locally(&mut self, map: &NodeMap, node: NodeId) -> bool {
        if let Some(parent) = node.parent(map) {
            self.nodes
                .insert(
                    parent,
                    SelectionInfo {
                        selected_child: node,
                        stop_here: false,
                    },
                )
                .map(|info| info.selected_child != node)
                .unwrap_or(true)
        } else {
            false
        }
    }

    pub(super) fn select(&mut self, map: &NodeMap, selection: NodeId) {
        if let Some(info) = self.nodes.get_mut(selection) {
            info.stop_here = true;
        }
        let mut node = selection;
        while let Some(parent) = node.parent(map) {
            self.nodes.insert(
                parent,
                SelectionInfo {
                    selected_child: node,
                    stop_here: false,
                },
            );
            node = parent;
        }
    }

    pub(super) fn handle_event(&mut self, map: &NodeMap, event: TreeEvent) {
        use TreeEvent::*;
        match event {
            AddedToForest(_node) => {}
            AddedToParent(_node) => {}
            Copied { src, dest, .. } => {
                let Some(info) = self.nodes.get(src) else {
                    return;
                };
                let selected_child = iter::zip(src.children(map), dest.children(map))
                    .filter(|(src_child, _)| *src_child == info.selected_child)
                    .map(|(_, dest_child)| dest_child)
                    .next()
                    .unwrap_or_else(|| {
                        panic!(
                            "Dest tree had different structure, or source node \
                            had nonexistent selection: {src:?}, {dest:?}"
                        )
                    });
                self.nodes.insert(
                    dest,
                    SelectionInfo {
                        selected_child,
                        stop_here: self.nodes[src].stop_here,
                    },
                );
            }
            RemovingFromParent(node) => {
                let parent = node.parent(map).unwrap();
                if self.nodes.get(parent).map(|n| n.selected_child) == Some(node) {
                    if let Some(new_selection) = node.next_sibling(map).or(node.prev_sibling(map)) {
                        self.nodes[parent].selected_child = new_selection;
                    } else {
                        self.nodes.remove(parent);
                    }
                }
            }
            RemovedFromForest(node) => {
                self.nodes.remove(node);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::actor::app::WindowId;
    use crate::model::Direction;
    use crate::model::layout_tree::LayoutTree;
    use crate::model::size::ContainerKind;

    #[test]
    fn it_moves_as_nodes_are_added_and_removed() {
        let mut tree = LayoutTree::new();
        let layout = tree.create_layout();
        let root = tree.root(layout);
        let n1 = tree.add_window_under(layout, root, WindowId::new(1, 1));
        let n2 = tree.add_window_under(layout, root, WindowId::new(1, 2));
        let n3 = tree.add_window_under(layout, root, WindowId::new(1, 3));
        assert_eq!(tree.selection(layout), root);
        tree.select(n2);
        assert_eq!(tree.selection(layout), n2);
        tree.remove_window(WindowId::new(1, 2));
        assert_eq!(tree.selection(layout), n3);
        tree.remove_window(WindowId::new(1, 3));
        assert_eq!(tree.selection(layout), n1);
        tree.remove_window(WindowId::new(1, 1));
        assert_eq!(tree.selection(layout), root);
    }

    #[test]
    fn remembers_nested_paths() {
        let mut tree = LayoutTree::new();
        let layout = tree.create_layout();
        let root = tree.root(layout);
        let a1 = tree.add_window_under(layout, root, WindowId::new(1, 1));
        let a2 = tree.add_container(root, ContainerKind::Horizontal);
        let _b1 = tree.add_window_under(layout, a2, WindowId::new(1, 2));
        let b2 = tree.add_window_under(layout, a2, WindowId::new(1, 3));
        let _b3 = tree.add_window_under(layout, a2, WindowId::new(1, 4));
        let a3 = tree.add_window_under(layout, root, WindowId::new(1, 5));

        tree.select(b2);
        assert_eq!(tree.selection(layout), b2);
        tree.select(a1);
        assert_eq!(tree.selection(layout), a1);
        tree.select(a3);
        assert_eq!(tree.selection(layout), a3);
        tree.remove_window(WindowId::new(1, 5));
        assert_eq!(tree.selection(layout), b2);
    }

    #[test]
    fn preserves_selection_after_move_within_parent() {
        let mut tree = LayoutTree::new();
        let layout = tree.create_layout();
        let root = tree.root(layout);
        let _n1 = tree.add_window_under(layout, root, WindowId::new(1, 1));
        let n2 = tree.add_window_under(layout, root, WindowId::new(1, 2));
        let _n3 = tree.add_window_under(layout, root, WindowId::new(1, 3));
        tree.select(n2);
        assert_eq!(tree.selection(layout), n2);
        tree.move_node(layout, n2, Direction::Left);
        assert_eq!(tree.selection(layout), n2);
    }

    #[test]
    fn allows_parent_selection() {
        let mut tree = LayoutTree::new();
        let layout = tree.create_layout();
        let root = tree.root(layout);
        let _a1 = tree.add_window_under(layout, root, WindowId::new(1, 1));
        let a2 = tree.add_container(root, ContainerKind::Horizontal);
        let b1 = tree.add_window_under(layout, a2, WindowId::new(1, 2));
        tree.select(b1);
        assert_eq!(tree.selection(layout), b1);
        tree.select(a2);
        assert_eq!(tree.selection(layout), a2);
    }
}
