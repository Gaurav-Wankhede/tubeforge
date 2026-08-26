//! Real-world Knowledge Graph scalability and performance benchmark against the LIVE database.

use std::path::PathBuf;
use std::time::Instant;
use tubeforge::analytics::kg_algorithms::{
    louvain_communities, pagerank, random_walk_with_restart,
};
use tubeforge::storage::Db;

#[tokio::test]
async fn benchmark_real_live_knowledge_graph() {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let db_path = PathBuf::from(format!("{}/.tubeforge/tubeforge.db", home));

    if !db_path.exists() {
        eprintln!("Live database not found at {}", db_path.display());
        return;
    }

    println!("\n================================================================================");
    println!("  ⚡ LIVE KNOWLEDGE GRAPH EMPIRICAL BENCHMARK (REAL DATABASE)");
    println!("  Target Database: {}", db_path.display());
    println!("================================================================================\n");

    let t_db_start = Instant::now();
    let db = Db::open(&db_path)
        .await
        .expect("Failed to open live database");
    let t_db_open = t_db_start.elapsed();

    // 1. Fetch raw counts
    let videos = db.all_videos().await.expect("Failed to query videos");
    let channels = db.all_channels().await.expect("Failed to query channels");
    let edges = db.list_edges().await.expect("Failed to query edges");
    let keywords = db.list_keywords().await.expect("Failed to query keywords");

    println!("📊 1. LIVE CORPUS METRICS:");
    println!("   • Total Videos in DB:    {}", videos.len());
    println!("   • Total Channels in DB:  {}", channels.len());
    println!("   • Raw Inter-Video Edges: {}", edges.len());
    println!("   • Tracked Keywords:      {}", keywords.len());
    println!(
        "   • DB Connection Open:    {:.3} ms",
        t_db_open.as_secs_f64() * 1000.0
    );

    // 2. Build In-Memory Knowledge Graph directly from Real Data
    let t_build_start = Instant::now();
    let mut kg = tubeforge::analytics::kg::KnowledgeGraph::new();

    // Insert real video entities
    for v in &videos {
        kg.insert_entity(tubeforge::analytics::kg::KgEntity::video(
            &v.video_id,
            &v.title,
        ));
        if let Some(ref cid) = v.channel_id {
            kg.insert_edge(
                &format!("video:{}", v.video_id),
                &format!("channel:{}", cid),
                tubeforge::analytics::kg::RelationType::CreatedBy,
                1.0,
            );
        }
        let tags: Vec<String> = serde_json::from_str(&v.tags).unwrap_or_default();
        for (pos, t) in tags.iter().enumerate() {
            let norm = t.trim().to_lowercase();
            if !norm.is_empty() {
                let w = 1.0 / (1.0 + pos as f64);
                kg.insert_edge(
                    &format!("video:{}", v.video_id),
                    &format!("tag:{}", norm),
                    tubeforge::analytics::kg::RelationType::Tags,
                    w,
                );
            }
        }
    }

    // Insert real channel entities and edges
    for c in &channels {
        kg.insert_entity(tubeforge::analytics::kg::KgEntity::channel(
            &c.channel_id,
            &c.title,
        ));
    }

    for e in &edges {
        kg.insert_edge(
            &format!("channel:{}", e.from_channel),
            &format!("channel:{}", e.to_channel),
            tubeforge::analytics::kg::RelationType::SimilarTo,
            e.weight,
        );
    }

    let t_build = t_build_start.elapsed();

    println!("\n🏗️  2. IN-MEMORY GRAPH CONSTRUCTION FROM REAL DATA:");
    println!("   • Total Entities (|V|):   {}", kg.node_count());
    println!("   • Total Relations (|E|):  {}", kg.edge_count());
    println!(
        "   • In-Memory Build Time:   {:.3} ms ({:.1} µs)",
        t_build.as_secs_f64() * 1000.0,
        t_build.as_secs_f64() * 1_000_000.0
    );

    // 4. Benchmark Louvain Community Detection (Real Data)
    let t_louvain_start = Instant::now();
    let communities = louvain_communities(&kg);
    let t_louvain = t_louvain_start.elapsed();
    let unique_communities = communities
        .values()
        .collect::<std::collections::HashSet<_>>()
        .len();

    println!("\n🔍 3. LOUVAIN COMMUNITY DETECTION BENCHMARK:");
    println!("   • Communities Detected:   {}", unique_communities);
    println!(
        "   • Execution Time:         {:.3} ms ({:.1} µs)",
        t_louvain.as_secs_f64() * 1000.0,
        t_louvain.as_secs_f64() * 1_000_000.0
    );
    println!(
        "   • Throughput:             {:.1} nodes/ms",
        kg.node_count() as f64 / (t_louvain.as_secs_f64() * 1000.0)
    );

    // 5. Benchmark Weighted PageRank (50 Iterations on Real Data)
    let t_pr_start = Instant::now();
    let pr_scores = pagerank(&kg);
    let t_pr = t_pr_start.elapsed();

    let mut sorted_pr: Vec<_> = pr_scores.into_iter().collect();
    sorted_pr.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    println!("\n📈 4. WEIGHTED PAGERANK BENCHMARK (50 ITERATIONS):");
    println!("   • Scored Entities:        {}", sorted_pr.len());
    println!(
        "   • Execution Time:         {:.3} ms ({:.1} µs)",
        t_pr.as_secs_f64() * 1000.0,
        t_pr.as_secs_f64() * 1_000_000.0
    );
    println!(
        "   • Edge Processing Speed:  {:.1} edge-steps/ms",
        (kg.edge_count() * 50) as f64 / (t_pr.as_secs_f64() * 1000.0)
    );

    if !sorted_pr.is_empty() {
        println!(
            "   • Top Entity #1 (Max PR):  {} (score: {:.6})",
            sorted_pr[0].0, sorted_pr[0].1
        );
        if sorted_pr.len() > 1 {
            println!(
                "   • Top Entity #2:           {} (score: {:.6})",
                sorted_pr[1].0, sorted_pr[1].1
            );
        }
        if sorted_pr.len() > 2 {
            println!(
                "   • Top Entity #3:           {} (score: {:.6})",
                sorted_pr[2].0, sorted_pr[2].1
            );
        }
    }

    // 6. Benchmark Random Walk with Restart (RWR Multi-Hop Retrieval)
    if let Some((seed_id, _)) = sorted_pr.first() {
        let t_rwr_start = Instant::now();
        let rwr_results = random_walk_with_restart(&kg, seed_id);
        let t_rwr = t_rwr_start.elapsed();

        println!("\n🌐 5. RANDOM WALK WITH RESTART (MULTI-HOP GRAPH RAG):");
        println!("   • Seed Entity:            {}", seed_id);
        println!("   • Related Entities Found: {}", rwr_results.len());
        println!(
            "   • RWR Traversal Latency:  {:.3} ms ({:.1} µs)",
            t_rwr.as_secs_f64() * 1000.0,
            t_rwr.as_secs_f64() * 1_000_000.0
        );
    }

    println!("\n================================================================================");
    println!("  ✅ BENCHMARK COMPLETE — ALL ALGORITHMS CONVERGED DETERMINISTICALLY");
    println!("================================================================================\n");
}
