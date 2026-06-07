use crate::spotify::TrackSummary;

/// A node in the local file tree.
#[derive(Clone)]
pub enum LocalNode {
    Folder {
        name: String,
        depth: usize,
        expanded: bool,
        #[allow(dead_code)]
        children_start: usize,
        children_count: usize,
    },
    Track {
        track: TrackSummary,
        depth: usize,
    },
}

impl LocalNode {
    pub fn depth(&self) -> usize {
        match self {
            LocalNode::Folder { depth, .. } => *depth,
            LocalNode::Track { depth, .. } => *depth,
        }
    }

    pub fn is_folder(&self) -> bool {
        matches!(self, LocalNode::Folder { .. })
    }

    pub fn track(&self) -> Option<&TrackSummary> {
        match self {
            LocalNode::Track { track, .. } => Some(track),
            _ => None,
        }
    }
}

#[derive(Default, Clone)]
pub struct LocalFileTree {
    pub all_nodes: Vec<LocalNode>,
    pub visible: Vec<usize>,
}

impl LocalFileTree {
    pub fn new(nodes: Vec<LocalNode>) -> Self {
        let mut tree = Self {
            all_nodes: nodes,
            visible: Vec::new(),
        };
        tree.rebuild_visible();
        tree
    }

    pub fn rebuild_visible(&mut self) {
        self.visible.clear();
        let mut skip_depth: Option<usize> = None;

        for (i, node) in self.all_nodes.iter().enumerate() {
            if let Some(depth) = skip_depth {
                if node.depth() > depth {
                    continue;
                }
                skip_depth = None;
            }

            self.visible.push(i);

            if let LocalNode::Folder {
                expanded: false,
                depth,
                ..
            } = node
            {
                skip_depth = Some(*depth);
            }
        }
    }

    pub fn toggle_folder(&mut self, visible_idx: usize) {
        let Some(&node_idx) = self.visible.get(visible_idx) else {
            return;
        };
        if let LocalNode::Folder { expanded, .. } = &mut self.all_nodes[node_idx] {
            *expanded = !*expanded;
        }
        self.rebuild_visible();
    }

    pub fn visible_len(&self) -> usize {
        self.visible.len()
    }

    pub fn get_visible(&self, visible_idx: usize) -> Option<&LocalNode> {
        self.visible
            .get(visible_idx)
            .and_then(|&i| self.all_nodes.get(i))
    }

    pub fn all_tracks_flat(&self) -> Vec<TrackSummary> {
        self.all_nodes
            .iter()
            .filter_map(|n| n.track().cloned())
            .collect()
    }

    pub fn tracks_under_folder(&self, visible_idx: usize) -> Vec<TrackSummary> {
        let Some(&node_idx) = self.visible.get(visible_idx) else {
            return vec![];
        };
        let folder_depth = self.all_nodes[node_idx].depth();
        self.all_nodes[node_idx + 1..]
            .iter()
            .take_while(|n| n.depth() > folder_depth)
            .filter_map(|n| n.track().cloned())
            .collect()
    }
}

pub const LIBRARY_ITEMS: &[&str] = &[
    "Liked Songs",
    "Albums",
    "Artists",
    "Podcasts",
    "Local Files",
];
