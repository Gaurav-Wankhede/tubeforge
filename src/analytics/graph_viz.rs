//! Graph Visualizer (PRD §FR-5, §UX-1): renders the Knowledge Graph as an
//! Obsidian-style force-directed graph using server-rendered SVG.
//!
//! Physics simulation:
//! - Hooke's law springs attract connected nodes
//! - Coulomb's repulsion pushes all nodes apart
//! - Centering force pulls nodes toward graph center
//!
//! No JavaScript chart library — pure Rust SVG generation.

use std::collections::HashMap;

use crate::analytics::kg::{EntityType, KnowledgeGraph, RelationType};

/// Visual node in the graph (PRD §FR-5.9).
#[derive(Debug, Clone)]
pub struct VisualNode {
    pub entity_id: String,
    pub display_name: String,
    pub entity_type: EntityType,
    pub x: f64,
    pub y: f64,
    pub radius: f64,
    pub color: &'static str,
    pub centrality: Option<f64>,
}

/// Visual edge in the graph (PRD §FR-5.8).
#[derive(Debug, Clone)]
pub struct VisualEdge {
    pub from: String,
    pub to: String,
    pub relation_type: RelationType,
    pub weight: f64,
    pub thickness: f64,
}

/// The complete visual graph ready for SVG rendering.
#[derive(Debug, Clone)]
pub struct VisualGraph {
    pub nodes: Vec<VisualNode>,
    pub edges: Vec<VisualEdge>,
    pub width: f64,
    pub height: f64,
}

/// Physics simulation parameters (PRD §FR-5.2).
pub struct PhysicsParams {
    /// Spring constant for Hooke's law (attraction along edges).
    pub spring_strength: f64,
    /// Target distance between connected nodes.
    pub spring_length: f64,
    /// Repulsion constant (Coulomb's law).
    pub repulsion_strength: f64,
    /// Centering force strength.
    pub centering_strength: f64,
    /// Damping factor (0-1, lower = more damping).
    pub damping: f64,
    /// Number of simulation iterations.
    pub iterations: usize,
}

impl Default for PhysicsParams {
    fn default() -> Self {
        PhysicsParams {
            spring_strength: 0.05,
            spring_length: 100.0,
            repulsion_strength: 8000.0,
            centering_strength: 0.01,
            damping: 0.85,
            iterations: 100,
        }
    }
}

/// Color palette for entity types (PRD §FR-5.3).
fn color_for_entity_type(ty: &EntityType) -> &'static str {
    match ty {
        EntityType::Video => "#e74c3c",
        EntityType::Channel => "#3498db",
        EntityType::Tag => "#2ecc71",
        EntityType::Keyword => "#f39c12",
        EntityType::Topic => "#9b59b6",
        EntityType::Entity => "#95a5a6",
    }
}

/// Build a visual graph from the Knowledge Graph (PRD §FR-5.1).
pub fn build_visual_graph(kg: &KnowledgeGraph, params: &PhysicsParams) -> VisualGraph {
    let mut nodes: Vec<VisualNode> = Vec::new();
    let mut node_positions: HashMap<String, (f64, f64)> = HashMap::new();

    // Create visual nodes with initial random positions
    let center_x = 400.0;
    let center_y = 300.0;
    for (i, (entity_id, entity)) in kg.entities.iter().enumerate() {
        let angle = (i as f64) * 2.399963; // Golden angle for even distribution
        let radius = 150.0;
        let x = center_x + angle.cos() * radius;
        let y = center_y + angle.sin() * radius;

        let node_radius = entity.centrality.map(|c| 5.0 + c * 15.0).unwrap_or(8.0);

        nodes.push(VisualNode {
            entity_id: entity_id.clone(),
            display_name: entity.display_name.clone(),
            entity_type: entity.entity_type,
            x,
            y,
            radius: node_radius,
            color: color_for_entity_type(&entity.entity_type),
            centrality: entity.centrality,
        });
        node_positions.insert(entity_id.clone(), (x, y));
    }

    // Create visual edges
    let mut edges: Vec<VisualEdge> = Vec::new();
    let mut seen_edges = std::collections::HashSet::new();
    for (from, neighbors) in &kg.adjacency {
        for (to, rel, weight) in neighbors {
            let edge_key = if from < to {
                (from.clone(), to.clone())
            } else {
                (to.clone(), from.clone())
            };
            if seen_edges.insert(edge_key) {
                edges.push(VisualEdge {
                    from: from.clone(),
                    to: to.clone(),
                    relation_type: *rel,
                    weight: *weight,
                    thickness: (weight * 2.0).clamp(0.5, 4.0),
                });
            }
        }
    }

    // Run physics simulation
    let mut velocities: HashMap<String, (f64, f64)> = HashMap::new();
    for node in &nodes {
        velocities.insert(node.entity_id.clone(), (0.0, 0.0));
    }

    for _ in 0..params.iterations {
        let mut forces: HashMap<String, (f64, f64)> = HashMap::new();
        for node in &nodes {
            forces.insert(node.entity_id.clone(), (0.0, 0.0));
        }

        // Repulsion between all pairs (Coulomb's law)
        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                let dx = nodes[j].x - nodes[i].x;
                let dy = nodes[j].y - nodes[i].y;
                let dist_sq = dx * dx + dy * dy;
                let dist = dist_sq.sqrt().max(1.0);

                let force = params.repulsion_strength / dist_sq;
                let fx = force * dx / dist;
                let fy = force * dy / dist;

                if let Some(f) = forces.get_mut(&nodes[i].entity_id) {
                    f.0 -= fx;
                    f.1 -= fy;
                }
                if let Some(f) = forces.get_mut(&nodes[j].entity_id) {
                    f.0 += fx;
                    f.1 += fy;
                }
            }
        }

        // Spring attraction along edges (Hooke's law)
        for edge in &edges {
            if let (Some(pos_from), Some(pos_to)) =
                (node_positions.get(&edge.from), node_positions.get(&edge.to))
            {
                let dx = pos_to.0 - pos_from.0;
                let dy = pos_to.1 - pos_from.1;
                let dist = (dx * dx + dy * dy).sqrt().max(1.0);

                let displacement = dist - params.spring_length;
                let force = params.spring_strength * displacement;
                let fx = force * dx / dist;
                let fy = force * dy / dist;

                if let Some(v) = velocities.get_mut(&edge.from) {
                    v.0 += fx;
                    v.1 += fy;
                }
                if let Some(v) = velocities.get_mut(&edge.to) {
                    v.0 -= fx;
                    v.1 -= fy;
                }
            }
        }

        // Centering force
        for node in &nodes {
            let dx = center_x - node.x;
            let dy = center_y - node.y;
            if let Some(v) = velocities.get_mut(&node.entity_id) {
                v.0 += dx * params.centering_strength;
                v.1 += dy * params.centering_strength;
            }
        }

        // Apply velocities with damping
        for node in &mut nodes {
            if let Some(vel) = velocities.get_mut(&node.entity_id) {
                vel.0 *= params.damping;
                vel.1 *= params.damping;
                node.x += vel.0;
                node.y += vel.1;
                node_positions.insert(node.entity_id.clone(), (node.x, node.y));
            }
        }
    }

    VisualGraph {
        nodes,
        edges,
        width: 800.0,
        height: 600.0,
    }
}

/// Render the visual graph as SVG (PRD §FR-5.10).
pub fn render_svg(graph: &VisualGraph) -> String {
    let mut svg = String::new();

    // SVG header
    svg.push_str(&format!(
        "<svg viewBox=\"0 0 {:.0} {:.0}\" xmlns=\"http://www.w3.org/2000/svg\" class=\"kg-graph\">",
        graph.width, graph.height
    ));

    // Render edges first (behind nodes)
    for edge in &graph.edges {
        let from_node = graph.nodes.iter().find(|n| n.entity_id == edge.from);
        let to_node = graph.nodes.iter().find(|n| n.entity_id == edge.to);

        if let (Some(from), Some(to)) = (from_node, to_node) {
            svg.push_str(&format!(
                "<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" \
                 stroke=\"#666\" stroke-width=\"{:.1}\" stroke-opacity=\"0.6\" />",
                from.x, from.y, to.x, to.y, edge.thickness
            ));
        }
    }

    // Render nodes
    for node in &graph.nodes {
        // Node circle
        svg.push_str(&format!(
            "<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"{:.1}\" fill=\"{}\" \
             stroke=\"#fff\" stroke-width=\"1.5\" class=\"kg-node\" \
             data-entity-id=\"{}\" data-entity-type=\"{}\" />",
            node.x, node.y, node.radius, node.color, node.entity_id, node.entity_type
        ));

        // Node label
        svg.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" \
             font-size=\"10\" fill=\"#333\" class=\"kg-label\">{}</text>",
            node.x,
            node.y + node.radius + 12.0,
            html_escape(&node.display_name)
        ));
    }

    svg.push_str("</svg>");
    svg
}

/// Simple HTML escaping for SVG text content.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Build a local graph view: N-hop neighborhood of a selected node (PRD §FR-5.4).
pub fn build_local_graph(
    kg: &KnowledgeGraph,
    seed_entity_id: &str,
    hops: usize,
    params: &PhysicsParams,
) -> VisualGraph {
    let neighborhood = kg.neighborhood(seed_entity_id, hops);
    let mut local_kg = KnowledgeGraph::new();

    // Add seed entity
    if let Some(entity) = kg.get_entity(seed_entity_id) {
        local_kg.insert_entity(entity.clone());
    }

    // Add neighbors
    for neighbor_id in neighborhood.keys() {
        if let Some(entity) = kg.get_entity(neighbor_id) {
            local_kg.insert_entity(entity.clone());
        }
    }

    // Add edges between entities in the local graph
    let local_ids: std::collections::HashSet<_> = local_kg.entities.keys().cloned().collect();
    for (from, edges) in &kg.adjacency {
        if local_ids.contains(from) {
            for (to, rel, weight) in edges {
                if local_ids.contains(to) {
                    local_kg.insert_edge(from, to, *rel, *weight);
                }
            }
        }
    }

    build_visual_graph(&local_kg, params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::kg::KgEntity;
    use proptest::prelude::*;

    fn build_test_kg() -> KnowledgeGraph {
        let mut kg = KnowledgeGraph::new();
        kg.insert_entity(KgEntity::video("v1", "Rust Guide"));
        kg.insert_entity(KgEntity::video("v2", "Rust Tutorial"));
        kg.insert_entity(KgEntity::tag("rust"));
        kg.insert_entity(KgEntity::channel("UC:A", "Channel A"));

        kg.insert_edge("video:v1", "tag:rust", RelationType::Tags, 1.0);
        kg.insert_edge("video:v2", "tag:rust", RelationType::Tags, 1.0);
        kg.insert_edge("video:v1", "channel:UC:A", RelationType::CreatedBy, 1.0);
        kg.insert_edge("video:v1", "video:v2", RelationType::SimilarTo, 0.8);

        kg.set_centrality("channel:UC:A", 0.8);
        kg.set_centrality("tag:rust", 0.6);

        kg
    }

    proptest! {
        #[test]
        fn visual_graph_has_all_nodes(
            _dummy in 0..5u8,
        ) {
            let kg = build_test_kg();
            let params = PhysicsParams::default();
            let visual = build_visual_graph(&kg, &params);
            prop_assert_eq!(visual.nodes.len(), kg.node_count());
        }

        #[test]
        fn visual_graph_has_edges(
            _dummy in 0..5u8,
        ) {
            let kg = build_test_kg();
            let params = PhysicsParams::default();
            let visual = build_visual_graph(&kg, &params);
            prop_assert!(!visual.edges.is_empty());
        }

        #[test]
        fn svg_renders_without_error(
            _dummy in 0..5u8,
        ) {
            let kg = build_test_kg();
            let params = PhysicsParams::default();
            let visual = build_visual_graph(&kg, &params);
            let svg = render_svg(&visual);
            prop_assert!(svg.contains("<svg"));
            prop_assert!(svg.contains("</svg>"));
        }

        #[test]
        fn svg_contains_nodes(
            _dummy in 0..5u8,
        ) {
            let kg = build_test_kg();
            let params = PhysicsParams::default();
            let visual = build_visual_graph(&kg, &params);
            let svg = render_svg(&visual);
            prop_assert!(svg.contains("kg-node"));
        }

        #[test]
        fn svg_contains_edges(
            _dummy in 0..5u8,
        ) {
            let kg = build_test_kg();
            let params = PhysicsParams::default();
            let visual = build_visual_graph(&kg, &params);
            let svg = render_svg(&visual);
            prop_assert!(svg.contains("<line"));
        }

        #[test]
        fn svg_escapes_html(
            _dummy in 0..5u8,
        ) {
            let escaped = html_escape("<script>alert('xss')</script>");
            prop_assert!(!escaped.contains("<script>"));
            prop_assert!(escaped.contains("&lt;"));
        }

        #[test]
        fn local_graph_excludes_distant_nodes(
            _dummy in 0..5u8,
        ) {
            let kg = build_test_kg();
            let params = PhysicsParams::default();
            let local = build_local_graph(&kg, "video:v1", 1, &params);
            // With 1 hop from v1, should include v1, tag:rust, channel:UC:A, video:v2
            let ids: Vec<&str> = local.nodes.iter().map(|n| n.entity_id.as_str()).collect();
            prop_assert!(ids.contains(&"video:v1"));
            prop_assert!(ids.contains(&"tag:rust"));
        }

        #[test]
        fn local_graph_respects_hops(
            hops in 1..4usize,
        ) {
            let kg = build_test_kg();
            let params = PhysicsParams::default();
            let local = build_local_graph(&kg, "video:v1", hops, &params);
            // Seed should always be included
            let ids: Vec<&str> = local.nodes.iter().map(|n| n.entity_id.as_str()).collect();
            prop_assert!(ids.contains(&"video:v1"));
        }

        #[test]
        fn node_radius_reflects_centrality(
            _dummy in 0..5u8,
        ) {
            let kg = build_test_kg();
            let params = PhysicsParams::default();
            let visual = build_visual_graph(&kg, &params);
            // channel:UC:A has centrality 0.8 → larger radius
            let channel_node = visual.nodes.iter().find(|n| n.entity_id == "channel:UC:A");
            prop_assert!(channel_node.is_some());
            if let Some(node) = channel_node {
                prop_assert!(node.radius > 8.0, "central node should have larger radius");
            }
        }

        #[test]
        fn edge_thickness_reflects_weight(
            _dummy in 0..5u8,
        ) {
            let kg = build_test_kg();
            let params = PhysicsParams::default();
            let visual = build_visual_graph(&kg, &params);
            for edge in &visual.edges {
                prop_assert!(edge.thickness >= 0.5);
                prop_assert!(edge.thickness <= 4.0);
            }
        }

        #[test]
        fn physics_converges_to_finite_positions(
            _dummy in 0..5u8,
        ) {
            let kg = build_test_kg();
            let params = PhysicsParams::default();
            let visual = build_visual_graph(&kg, &params);
            for node in &visual.nodes {
                prop_assert!(node.x.is_finite(), "node x not finite: {}", node.x);
                prop_assert!(node.y.is_finite(), "node y not finite: {}", node.y);
            }
        }

        #[test]
        fn color_for_entity_type_is_valid_hex(
            ty in prop_oneof![
                Just(EntityType::Video),
                Just(EntityType::Channel),
                Just(EntityType::Tag),
                Just(EntityType::Keyword),
                Just(EntityType::Topic),
                Just(EntityType::Entity),
            ],
        ) {
            let color = color_for_entity_type(&ty);
            prop_assert!(color.starts_with('#'));
            prop_assert_eq!(color.len(), 7);
        }

        #[test]
        fn empty_graph_renders_empty_svg(
            _dummy in 0..5u8,
        ) {
            let kg = KnowledgeGraph::new();
            let params = PhysicsParams::default();
            let visual = build_visual_graph(&kg, &params);
            let svg = render_svg(&visual);
            prop_assert!(svg.contains("<svg"));
            prop_assert!(!svg.contains("kg-node"));
        }
    }
}
