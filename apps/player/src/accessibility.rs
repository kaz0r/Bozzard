//! Native screen reader adapter. Callbacks enqueue actions and never touch the simulation world.
use accesskit::{
    ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, Node, NodeId, Role, Tree,
    TreeId, TreeUpdate,
};
use accesskit_winit::Adapter;
use bozzard_scene::middleware::ui::Frame;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};
use winit::{event_loop::ActiveEventLoop, window::Window};
struct Activate {
    full: Arc<AtomicBool>,
    window: Arc<Window>,
}
impl ActivationHandler for Activate {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.full.store(true, Ordering::Release);
        self.window.request_redraw();
        None
    }
}
struct Actions {
    sender: mpsc::SyncSender<ActionRequest>,
    window: Arc<Window>,
}
impl ActionHandler for Actions {
    fn do_action(&mut self, request: ActionRequest) {
        let _ = self.sender.try_send(request);
        self.window.request_redraw();
    }
}
struct Deactivate;
impl DeactivationHandler for Deactivate {
    fn deactivate_accessibility(&mut self) {}
}
pub struct Accessibility {
    pub adapter: Adapter,
    full: Arc<AtomicBool>,
    previous: BTreeMap<NodeId, Node>,
    requests: mpsc::Receiver<ActionRequest>,
}
impl Accessibility {
    pub fn new(event_loop: &ActiveEventLoop, window: &Arc<Window>) -> Self {
        let full = Arc::new(AtomicBool::new(true));
        let (sender, requests) = mpsc::sync_channel(256);
        let adapter = Adapter::with_direct_handlers(
            event_loop,
            window,
            Activate {
                full: full.clone(),
                window: window.clone(),
            },
            Actions {
                sender,
                window: window.clone(),
            },
            Deactivate,
        );
        Self {
            adapter,
            full,
            previous: BTreeMap::new(),
            requests,
        }
    }
    pub fn drain(&self) -> Vec<ActionRequest> {
        self.requests.try_iter().take(256).collect()
    }
    pub fn update(&mut self, frame: &Frame, title: &str, scale: f32) {
        // No semantic-tree allocation or comparison while accessibility is inactive.
        self.adapter.update_if_active(|| {
            let full = self.full.swap(false, Ordering::AcqRel);
            let mut root = Node::new(Role::Window);
            root.set_label(title);
            // Invisible layout-only ancestors are omitted; authored reading order is preserved.
            root.set_children(
                frame
                    .elements
                    .iter()
                    .filter(|e| e.clip.size.iter().all(|v| *v > 0.))
                    .map(|e| NodeId(e.id))
                    .collect::<Vec<_>>(),
            );
            let mut nodes = BTreeMap::from([(NodeId(0), root)]);
            nodes.extend(
                frame
                    .elements
                    .iter()
                    .filter(|e| e.clip.size.iter().all(|v| *v > 0.))
                    .map(|e| {
                        (
                            NodeId(e.id),
                            bozzard_render_assets::accessibility::node(e, [0.; 2], scale),
                        )
                    }),
            );
            let changed = nodes
                .iter()
                .filter(|(id, node)| full || self.previous.get(id).is_none_or(|old| old != *node))
                .map(|(id, node)| (*id, node.clone()))
                .collect();
            self.previous = nodes;
            TreeUpdate {
                nodes: changed,
                tree: full.then(|| Tree::new(NodeId(0))),
                tree_id: TreeId::ROOT,
                focus: frame
                    .elements
                    .iter()
                    .find(|e| e.focused && e.clip.size.iter().all(|v| *v > 0.))
                    .map_or(NodeId(0), |e| NodeId(e.id)),
            }
        });
    }
}
