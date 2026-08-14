# PRD: TubeForge Knowledge Graph Engine

**Document Version:** 3.0 | **Date:** August 8, 2026 | **Status:** Approved for Implementation
**Author:** TubeForge Architecture Team | **Companion Documents:** HLD v1.2, LLD v1.5, Schema Audit (this session)

> **⚠️ ARCHITECTURE DECISION (v3.0):** The Knowledge Graph is an **internal-only enhancement** to existing APIs. There are **NO** separate `/api/kg/*` endpoints. All KG processing happens internally within existing handlers. The frontend consumes KG signals through enhanced existing endpoints (e.g., `GET /api/scores/{id}` returns a `graph_scores` field). This is a YAGNI-driven simplification — the KG adds value without expanding the API surface.

---

## 1. Working Backwards Press Release

### Headline

**TubeForge Launches Knowledge Graph Engine: The First Local-First YouTube Growth Intelligence System That Thinks in Connections, Not Just Keywords**

### Sub-Headline

TubeForge v2 introduces a unified Knowledge Graph that transforms scattered YouTube data into an interconnected intelligence network — enabling creators to see exactly how videos, channels, tags, keywords, and topics relate to each other, and to retrieve precise insights without context loss.

### Problem

YouTube creators face a fundamental information problem: the signals that drive recommendation are interconnected (videos relate to channels, channels compete in topics, tags connect to keywords, keywords cluster into themes), but every existing tool treats these signals in isolation.

Current tools fall into two camps:
- **Spreadsheet tools** (vidIQ, TubeBuddy): Give you scores and lists but no understanding of *why* things connect
- **AI chatbots**: Hallucinate relationships because they have no structured knowledge

The result: creators make decisions based on fragmented data, miss hidden opportunities, and waste time connecting dots manually. Context "rots" between tools — you see a keyword score but not its competitive landscape, a tag suggestion but not its authority, a topic idea but not its relationship to your existing content.

### Solution

TubeForge v2 introduces an **internal Knowledge Graph Engine** that enhances existing APIs without expanding the public surface. All KG processing happens inside existing handlers:

1. **Unified Entity Graph**: Videos, channels, tags, keywords, topics, and extracted entities become nodes in a single graph
2. **Typed Relationships**: Every connection has a type (tags, competes_in, dominates, similar_to, mentioned_in) and weight
3. **Hybrid Retrieval**: Combines BM25 text search + vector similarity + graph traversal — no context loss
4. **Community Detection**: Automatically discovers topic clusters using Louvain algorithm
5. **Graph Visualization**: Obsidian-style force-directed graph for visual exploration (rendered as SVG, served via existing dashboard page)
6. **Context-Preserving Queries**: Every result carries its full provenance chain — you see *why* something was retrieved
7. **Internal-Only API**: **NO** separate `/api/kg/*` endpoints. KG signals are returned as additional fields on existing endpoints (e.g., `graph_scores` on `GET /api/scores/{id}`).

### Customer Quotes

> *"I used to jump between vidIQ, Ahrefs, and a spreadsheet to understand my niche. Now I open TubeForge's graph view and literally see the opportunity gaps — topics where I have no coverage but my competitors are weak."*
> — **Technical Education Creator, 50K subscribers**

> *"The hybrid retriever found a keyword I'd never have discovered: 'rust async patterns' — it's related to my best-performing video but has 3x less competition. The graph showed me the connection my old tools missed."*
> — **Rust Programming Educator, 120K subscribers**

> *"I can finally see WHY TubeForge suggests a tag. It's not just a score — it's the tag's authority (used by high-centrality channels), its relationship to my other tags, and its competitive density. That's the difference between guessing and knowing."*
> — **System Design Educator, 200K subscribers**

### Getting Started

```bash
# 1. Install TubeForge v2
curl -sSfL https://get.tubeforge.tech/install.sh | sh

# 2. Initialize (KG is built automatically on first query)
tubeforge init

# 3. Ingest your competitors (RSS + SERP discovery)
tubeforge ingest channels @competitor1 @competitor2 @competitor3
tubeforge keywords discover "rust programming" --enrich --register-competitors

# 4. (KG is built lazily on first graph-aware query — no separate command)
tubeforge serve  # graph_scores / centrality appear on existing endpoints

# 5. Get graph-aware scores (existing score endpoint returns graph_scores field)
tubeforge score --draft-title "Rust Async Patterns Complete Guide" --keywords "rust async" --graph-aware
```

---

## 2. Customer Problem Statement

### Detailed Problem

YouTube's recommendation algorithm evaluates videos within a rich context of relationships:
- **Video ↔ Channel**: Authority inheritance (a strong channel boosts new videos)
- **Video ↔ Tag**: Categorization and discoverability
- **Tag ↔ Tag**: Semantic clustering (tags that co-occur form topics)
- **Video ↔ Keyword**: Search ranking competition
- **Channel ↔ Topic**: Dominance (some channels own certain topics)
- **Keyword ↔ Keyword**: Intent clustering (related keywords form content themes)

Current tools flatten these relationships into independent scores. A creator sees:
- "Keyword X has 85 opportunity score" — but not *who* dominates it or *why*
- "Tag Y is suggested" — but not its authority or relationship to existing tags
- "Topic Z is trending" — but not its competitive landscape or their position in it

This fragmentation causes three specific failures:

1. **Context Loss**: Information gathered in one tool doesn't transfer to another. The creator must manually reconstruct relationships.
2. **Opportunity Blindness**: Hidden gaps (underserved topics, low-competition keywords, authority-building tags) are invisible without graph traversal.
3. **Decision Paralysis**: Without understanding *why* a suggestion exists, creators can't evaluate its merit — they either ignore it or follow it blindly.

### Current Limitations (Pre-KG TubeForge)

| Limitation | Impact | KG Solution |
|---|---|---|
| Scoring is structural only (15 SEO + 7 GEO components) | Misses graph signals like tag authority, topic dominance | 3 new graph-based components integrated into scoring |
| BM25-only retrieval (own engine) | Misses semantic similarity and relationship context | Hybrid retrieval: BM25 + graph traversal (vector deferred) |
| Edges table is channel→channel only | No video-level, tag-level, or keyword-level graph | Unified kg_entities + kg_relations tables |
| No entity extraction from transcripts/comments | Misses the richest source of entity relationships | NLP pipeline feeds entity nodes into KG |
| No visualization | Creators can't *see* the knowledge structure | Obsidian-style force-directed graph |
| Ideas are BM25-neighborhood only | Misses graph-based gap detection | Community-aware idea generation |
| No provenance tracking | Creators can't understand *why* something was suggested | Full provenance chain on every retrieval |

### Customer Impact

**Without KG:**
- 40% of keyword opportunities missed (isolated scoring misses graph-revealed connections)
- 3x longer research time (manual correlation across tools)
- Tag strategy is guesswork (no authority or relationship data)
- Content gaps remain invisible (no community coverage analysis)

**With KG:**
- Graph expansion reveals 2-5x more relevant opportunities per query
- Single-tool research (all relationships visible in one view)
- Tag strategy is data-driven (authority scores + relationship mapping)
- Content gaps are visually obvious (community coverage heatmap)

---

## 3. Success Metrics

### Customer Experience Metrics

| Metric | Baseline (v1) | Target (v2) | Measurement |
|---|---|---|---|
| **Query relevance** (precision@5) | 0.62 (BM25 only) | 0.85 (hybrid) | Human-evaluated query results |
| **Context preservation** (provenance coverage) | 0% (no provenance) | 100% (every result has chain) | Automated audit |
| **Opportunity discovery rate** | 12 keywords/query | 35 keywords/query (graph expansion) | Query result count |
| **Time to insight** | 45 min (multi-tool) | 8 min (single graph view) | User timing study |
| **Visualization interactivity** | N/A | <16ms frame rate (60fps) | Browser perf API |
| **Graph build time** | N/A | <2s for 10k videos | Benchmark |

### Business Metrics

| Metric | Baseline (v1) | Target (v2) | Measurement |
|---|---|---|---|
| **SEO score >70 achievement rate** | 35% of drafts | 75% of drafts (graph-aware scoring) | Score distribution |
| **User retention (30-day)** | 60% | 85% (KG is sticky) | Analytics |
| **Query depth** (avg hops) | 1.0 (flat) | 2.3 (graph traversal) | Query logs |
| **Feature adoption** (KG queries/total) | 0% | 60% within 30 days | Usage tracking |

### Long-Term Indicators

| Indicator | Target | Timeline |
|---|---|---|
| **Knowledge Graph density** (edges/node) | >5.0 (well-connected) | 6 months |
| **Community detection quality** (modularity) | >0.6 (clear clusters) | 6 months |
| **Entity extraction coverage** | >80% of videos have extracted entities | 12 months |
| **Graph-aware scoring correlation with views** | r > 0.7 | 12 months |

---

## 4. Requirements

### 4.1 Functional Requirements

#### FR-1: Knowledge Graph Construction
- **FR-1.1**: System SHALL create `kg_entities` nodes from all existing data sources (videos, channels, tags, keywords, topics)
- **FR-1.2**: System SHALL create `kg_relations` edges from all relationship sources (video_tags, keyword_rankings, competitor_tags, edges, topic_categories)
- **FR-1.3**: System SHALL support entity types: video, channel, tag, keyword, topic, entity (NLP-extracted), community
- **FR-1.4**: System SHALL support relation types: tags, created_by, about_topic, competes_in, dominates, related_to, similar_to, mentioned_in, contains
- **FR-1.5**: System SHALL auto-build the KG on first query if not explicitly built
- **FR-1.6**: System SHALL support both **full rebuild** (clear all, rebuild from scratch) and **incremental update** (only process changed entities)
- **FR-1.7**: System SHALL cache the in-memory KG in `meta` table for fast startup
- **FR-1.8**: KG build SHALL be **idempotent** — running twice produces identical state (no duplicates, no orphans)

#### FR-2: Hybrid Retrieval Engine
- **FR-2.1**: System SHALL support three retrieval modes: `local` (entity + 1-2 hops), `global` (community summaries), `mix` (both combined, default)
- **FR-2.2**: System SHALL combine BM25 text search, vector similarity, and graph traversal in a single query
- **FR-2.3**: System SHALL return results with full provenance chains (why each result was retrieved)
- **FR-2.4**: System SHALL support configurable graph depth (1-5 hops)
- **FR-2.5**: System SHALL support filtering by entity type, relation type, date range, score range, and duration
- **FR-2.6**: System SHALL support sorting by relevance (hybrid score), newest, most viewed, best SEO, best total
- **FR-2.7**: System SHALL implement random walk with restart for graph-based similarity
- **FR-2.8**: System SHALL implement context window management to prevent context ROT (max 4096 tokens per retrieval packet)

#### FR-3: Graph Analytics
- **FR-3.1**: System SHALL compute PageRank centrality for all entities
- **FR-3.2**: System SHALL perform Louvain community detection to find topic clusters
- **FR-3.3**: System SHALL compute tag authority scores (weighted by channel centrality)
- **FR-3.4**: System SHALL compute topic dominance scores (channel's share of topic cluster)
- **FR-3.5**: System SHALL compute keyword competition scores (incumbent authority)
- **FR-3.6**: System SHALL find shortest paths between any two entities
- **FR-3.7**: System SHALL detect bridge entities (high betweenness centrality)
- **FR-3.8**: System SHALL compute connected components for graph health monitoring

#### FR-4: Graph-Aware Scoring
- **FR-4.1**: System SHALL add three new SEO components: `tag_authority`, `topic_dominance`, `keyword_competition`
- **FR-4.2**: Graph-based components SHALL integrate into the existing weighted scoring pipeline
- **FR-4.3**: Graph-based components SHALL default to 0 when no graph data is available (backward compatible)
- **FR-4.4**: System SHALL re-score all videos when the KG is rebuilt
- **FR-4.5**: System SHALL expose graph component breakdown in the score envelope

#### FR-5: Graph Visualization
- **FR-5.1**: System SHALL render an interactive force-directed graph (Obsidian-style)
- **FR-5.2**: Graph SHALL support physics simulation (Hooke's law springs + Coulomb's repulsion)
- **FR-5.3**: Graph SHALL support node grouping by entity type, community, or custom query
- **FR-5.4**: Graph SHALL support local graph view (N-hop neighborhood of selected node)
- **FR-5.5**: Graph SHALL support filtering by entity type, relation type, and properties
- **FR-5.6**: Graph SHALL support node dragging with physics response
- **FR-5.7**: Graph SHALL support zoom, pan, and focus operations
- **FR-5.8**: Graph SHALL display edge weights as line thickness
- **FR-5.9**: Graph SHALL display node centrality as node size
- **FR-5.10**: Graph SHALL be rendered using Canvas 2D (no external JS library)

#### FR-6: Internal Query Integration
- **FR-6.1**: System SHALL support structured queries: text, channel_id, date range, score range, duration, tags, topic
- **FR-6.2**: System SHALL support graph queries: similar_videos, neighborhood, topic_cluster, keyword_best, topic_outliers
- **FR-6.3**: System SHALL support hybrid queries combining structured filters + graph traversal
- **FR-6.4**: System SHALL expose query capability via CLI (`tubeforge query`) and through **enhanced existing dashboard pages** (NOT a new `/api/kg/query` endpoint)
- **FR-6.5**: System SHALL return query results with provenance metadata
- **FR-6.6**: **NO separate `/api/kg/*` endpoints** — all KG query results are returned through enhanced existing endpoints or CLI output

#### FR-7: Idempotency & Data Integrity
- **FR-7.1**: KG entities SHALL be uniquely identified by `entity_id` (format: `{type}:{canonical}` e.g., `video:abc123`, `tag:rust`)
- **FR-7.2**: KG relations SHALL be uniquely identified by `(from_entity, to_entity, relation_type)`
- **FR-7.3**: Full rebuild SHALL clear all KG tables before repopulating (no stale data)
- **FR-7.4**: Incremental update SHALL only process entities where `source_ref.updated_at > since`
- **FR-7.5**: KG build SHALL be transactional — all-or-nothing (no partial state on failure)
- **FR-7.6**: Communities SHALL be fully recomputed on every build (derived data, not incremental)
- **FR-7.7**: Centrality SHALL be fully recomputed on every build (derived from current graph)
- **FR-7.8**: System SHALL track `source_ref` on every entity for incremental update support

### 4.2 Technical Requirements

#### TR-1: Storage
- **TR-1.1**: KG tables SHALL be stored in the existing `tfdb` engine (`kg_entities`, `kg_relations`, `kg_communities` — no separate DB)
- **TR-1.2**: KG tables SHALL be safe for concurrent readers (single-writer engine)
- **TR-1.3**: KG indexes SHALL optimize for: entity lookup by ID, neighbor lookup by entity, filter by type, filter by community
- **TR-1.4**: KG cache SHALL be serialized to `meta` table (`kg_cache_json`) for sub-second startup
- **TR-1.5**: Embedding vectors SHALL use the existing `videos.embedding` BLOB column (reserved in v1; unused until the vector pipeline ships)

#### TR-2: Performance
- **TR-2.1**: KG full build SHALL complete in <2s for 10k videos on M4
- **TR-2.2**: PageRank SHALL complete in <200ms for 10k nodes
- **TR-2.3**: Louvain community detection SHALL complete in <500ms for 10k nodes
- **TR-2.4**: Hybrid retrieval SHALL complete in <100ms for 10k videos
- **TR-2.5**: Graph visualization SHALL render at 60fps for <500 nodes
- **TR-2.6**: KG incremental update SHALL complete in <100ms per new video

#### TR-3: Data Structures
- **TR-3.1**: In-memory KG SHALL use `HashMap<String, KgEntity>` for O(1) entity lookup
- **TR-3.2**: In-memory KG SHALL use adjacency list `HashMap<String, Vec<(String, RelationType, f64)>>` for O(1) neighbor access
- **TR-3.3**: In-memory KG SHALL maintain reverse adjacency for bidirectional traversal
- **TR-3.4**: In-memory KG SHALL maintain type index `HashMap<EntityType, Vec<String>>` for filtered queries
- **TR-3.5**: Vector index SHALL use the HNSW module (`src/tfdb/hnsw.rs`); **deferred** — no embeddings are generated until the vector pipeline ships (BM25 lexical retrieval is the shipped path)

#### TR-4: Algorithms
- **TR-4.1**: PageRank SHALL use damped (0.85) iteration (50 iterations, converges at this scale)
- **TR-4.2**: Louvain SHALL use standard modularity optimization
- **TR-4.3**: Random walk with restart SHALL use restart probability 0.3, max 50 iterations
- **TR-4.4**: Community detection SHALL run on the full KG (all entity types)
- **TR-4.5**: Hybrid fusion SHALL use configurable weights: `(1-w)*bm25 + w*graph`

#### TR-5: Integration (Internal-Only — No New Endpoints)
- **TR-5.1**: KG builder SHALL read from all existing tables (videos, channels, tags, keywords, edges, etc.)
- **TR-5.2**: KG builder SHALL write to kg_entities, kg_relations, kg_communities
- **TR-5.3**: Scoring pipeline SHALL read graph signals from KG (tag authority, topic dominance, keyword competition) — returned as `graph_scores` field on existing score endpoints
- **TR-5.4**: Ideas generator SHALL use graph-based gap detection (low-centrality communities) — returned via existing `GET /api/ideas/analyze`
- **TR-5.5**: Scorecard SHALL include channel centrality and community membership — returned via existing `GET /api/scorecard`
- **TR-5.6**: Tags analyzer SHALL include tag authority scores from KG — returned via existing `GET /api/tags/gaps`
- **TR-5.7**: Gaps analyzer SHALL include community coverage analysis — returned via existing `GET /api/gaps`
- **TR-5.8**: Research discover SHALL enrich SERP results with KG context — returned via existing analysis endpoints
- **TR-5.9**: **NO new HTTP endpoints** — all KG output is returned as additional optional fields on existing endpoints; frontend handles absence gracefully (backward compatible)

### 4.3 UX Requirements

#### UX-1: Dashboard Graph View (Internal Enhancement)
- **UX-1.1**: Dashboard SHALL include a graph visualization within the **existing Analysis/Command Center page** (NOT a new route)
- **UX-1.2**: Graph view SHALL include controls: entity type filter, relation type filter, community filter, search box
- **UX-1.3**: Clicking a node SHALL show its detail panel (properties, relations, scores)
- **UX-1.4**: Graph SHALL support "Local Graph" mode (show only N-hop neighborhood)
- **UX-1.5**: Graph SHALL support "Global Graph" mode (show all communities)
- **UX-1.6**: Graph SVG is fetched via **enhanced existing endpoint** (e.g., `GET /api/analysis/graph`) — NOT a new `/api/kg/graph` route

#### UX-2: Query Interface (CLI-First, No New API)
- **UX-2.1**: KG query capability is exposed via enhanced existing endpoints and CLI outputs (internal KG; no separate query command)
- **UX-2.2**: Query results SHALL show provenance chain (expandable)
- **UX-2.3**: Query results SHALL link to graph view (show result in graph context)
- **UX-2.4**: Query SHALL support saved queries (bookmark frequent searches)

#### UX-3: Score Explanation (Enhanced Existing Endpoints)
- **UX-3.1**: Score detail (`GET /api/scores/{id}`) SHALL include `graph_scores` field with tag authority, topic dominance, keyword competition
- **UX-3.2**: Score recommendations SHALL reference graph signals ("Your tag 'rust' has low authority — channels using it have low centrality")
- **UX-3.3**: Score comparison SHALL show graph position relative to competitors (via enhanced `GET /api/scorecard`)

---

## 5. Schema Design

### 5.1 KG Tables (Migration 009)

```sql
-- Core entity table: every node in the knowledge graph
CREATE TABLE kg_entities (
    entity_id       TEXT PRIMARY KEY,           -- "video:abc123", "tag:rust", "channel:UC..."
    entity_type     TEXT NOT NULL,              -- video|channel|tag|keyword|topic|entity
    canonical_name  TEXT NOT NULL,              -- normalized lookup key
    display_name    TEXT NOT NULL,              -- human-readable label
    properties      TEXT NOT NULL DEFAULT '{}', -- JSON: flexible per-type metadata
    embedding       BLOB,                       -- vector embedding (when available)
    centrality      REAL,                       -- cached PageRank score
    community_id    INTEGER,                    -- Louvain community assignment
    source          TEXT NOT NULL DEFAULT 'system', -- system|nlp|manual|import
    source_ref      TEXT,                       -- "videos:abc123", "tags:rust" (for incremental)
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE INDEX kg_entities_type ON kg_entities(entity_type);
CREATE INDEX kg_entities_community ON kg_entities(community_id);
CREATE INDEX kg_entities_centrality ON kg_entities(centrality DESC);
CREATE INDEX kg_entities_source_ref ON kg_entities(source_ref);

-- Core relation table: every edge in the knowledge graph
CREATE TABLE kg_relations (
    relation_id     INTEGER PRIMARY KEY AUTOINCREMENT,
    from_entity     TEXT NOT NULL REFERENCES kg_entities(entity_id) ON DELETE CASCADE,
    to_entity       TEXT NOT NULL REFERENCES kg_entities(entity_id) ON DELETE CASCADE,
    relation_type   TEXT NOT NULL,              -- tags|created_by|about_topic|competes_in|dominates|related_to|similar_to|mentioned_in|contains
    weight          REAL NOT NULL DEFAULT 1.0,
    source          TEXT NOT NULL DEFAULT 'system',
    created_at      TEXT NOT NULL,
    UNIQUE(from_entity, to_entity, relation_type)
);

CREATE INDEX kg_relations_from ON kg_relations(from_entity);
CREATE INDEX kg_relations_to ON kg_relations(to_entity);
CREATE INDEX kg_relations_type ON kg_relations(relation_type);

-- Community table: Louvain algorithm output
CREATE TABLE kg_communities (
    community_id    INTEGER PRIMARY KEY AUTOINCREMENT,
    community_type  TEXT NOT NULL,              -- topic_cluster|channel_group|tag_cluster
    summary         TEXT,                       -- auto-generated summary
    member_count    INTEGER NOT NULL DEFAULT 0,
    mean_views      REAL,
    mean_seo_score  REAL,
    top_entities    TEXT NOT NULL DEFAULT '[]', -- JSON array of entity_ids
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);
```

### 5.2 Entity ID Convention

| Type | Format | Example |
|---|---|---|
| video | `video:{video_id}` | `video:abc123def45` |
| channel | `channel:{channel_id}` | `channel:UC_x5XG1OV2P6uZZ5FSM9Ttw` |
| tag | `tag:{normalized_name}` | `tag:rust` |
| keyword | `keyword:{normalized_name}` | `keyword:rust-programming` |
| topic | `topic:{url_segment}` | `topic:rust_programming_language` |
| entity | `entity:{normalized_name}` | `entity:async-await` |

### 5.3 Entity Types & Sources

| KG Entity Type | Source Table | Extraction Method | Est. Count |
|---|---|---|---|
| `video` | `videos` | Direct mapping | 1-10k |
| `channel` | `channels` | Direct mapping | 50-500 |
| `tag` | `tags` + `video_tags` | Direct mapping | 500-5k |
| `keyword` | `keywords` | Direct mapping | 100-1k |
| `topic` | `videos.topic_categories` | URL segment parsing | 50-500 |
| `entity` | `transcripts`, `titles` | NLP extraction (Phase 2) | 1k-50k |
| `community` | `kg_communities` | Louvain algorithm | 10-100 |

### 5.4 Relation Types & Sources

| Relation | Source | Weight Basis |
|---|---|---|
| `(video)-[:tags]->(tag)` | `video_tags` | 1.0 / position |
| `(video)-[:created_by]->(channel)` | `videos.channel_id` | 1.0 |
| `(video)-[:about_topic]->(topic)` | `videos.topic_categories` | 1.0 |
| `(channel)-[:competes_in]->(keyword)` | `keyword_rankings` | 1.0 / position |
| `(channel)-[:dominates]->(topic)` | `competitor_tags` | video_count × avg_views |
| `(tag)-[:related_to]->(tag)` | Co-occurrence in `video_tags` | Jaccard similarity |
| `(keyword)-[:related_to]->(keyword)` | `keyword_research.related_keywords` | 1.0 |
| `(video)-[:similar_to]->(video)` | Embedding cosine / title overlap | Similarity score |
| `(channel)-[:competes_with]->(channel)` | `edges` (overlap) | Jaccard weight |
| `(entity)-[:mentioned_in]->(video)` | NLP extraction (Phase 2) | TF-IDF |
| `(community)-[:contains]->(entity)` | Louvain output | 1.0 |

---

## 6. Idempotency Design

### 6.1 Problem Statement

KG build must handle two scenarios correctly:
1. **Fresh build**: Empty KG tables, populate from source data
2. **Rebuild** (new videos, new keywords, schema migration): KG tables have data, update without duplicates

### 6.2 Idempotency Strategies

| Table | Unique Key | Strategy |
|---|---|---|
| `kg_entities` | `entity_id` PK | `INSERT OR REPLACE` — same entity updates properties/centrality |
| `kg_relations` | `UNIQUE(from, to, type)` | `INSERT OR REPLACE` — same edge updates weight |
| `kg_communities` | `community_id` PK | **Full clear + recompute** — communities are derived outputs |

### 6.3 Build Modes

#### Full Rebuild (default, <2s for 10k videos)
```
1. BEGIN TRANSACTION
2. DELETE FROM kg_relations
3. DELETE FROM kg_entities
4. DELETE FROM kg_communities
5. INSERT all entities (INSERT OR REPLACE for safety)
6. INSERT all relations (INSERT OR REPLACE for safety)
7. Run Louvain → INSERT communities
8. UPDATE entities SET community_id = ...
9. Run PageRank → UPDATE entities SET centrality = ...
10. COMMIT
```

#### Incremental Update (for large corpora, <100ms per video)
```
1. BEGIN TRANSACTION
2. Identify entities WHERE source_ref.updated_at > since
3. Delete affected relations (cascade)
4. Insert/update affected entities
5. Insert/update affected relations
6. Recompute affected communities only
7. Recompute centrality incrementally
8. COMMIT
```

### 6.4 Idempotent SQL Patterns

```sql
-- Entity: upsert (insert or replace all fields)
INSERT OR REPLACE INTO kg_entities
  (entity_id, entity_type, canonical_name, display_name, properties,
   embedding, centrality, community_id, source, source_ref, created_at, updated_at)
VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);

-- Relation: upsert weight on conflict
INSERT INTO kg_relations (from_entity, to_entity, relation_type, weight, source, created_at)
VALUES (?, ?, ?, ?, ?, ?)
ON CONFLICT(from_entity, to_entity, relation_type)
DO UPDATE SET weight = excluded.weight, source = excluded.source;

-- Communities: always full recompute (clear then insert)
DELETE FROM kg_communities;
INSERT INTO kg_communities (...) VALUES (...);
```

### 6.5 Safety Guarantees

| Concern | Guarantee |
|---|---|
| Duplicate entities | `entity_id` PK + `INSERT OR REPLACE` |
| Duplicate relations | `UNIQUE(from, to, type)` + `ON CONFLICT DO UPDATE` |
| Stale communities on rebuild | Full clear + recompute (derived data) |
| Stale centrality on rebuild | Full recompute (derived from graph) |
| Partial build failure | Transactional — all-or-nothing |
| Orphan relations | FK CASCADE on entity delete |
| Incremental update correctness | `source_ref` + `updated_at` filtering |

---

## 7. Data Structures

### 7.1 In-Memory Knowledge Graph

```rust
/// Central in-memory data structure for the Knowledge Graph.
/// Optimized for: O(1) entity lookup, O(1) neighbor access, O(E) traversal.
pub struct KnowledgeGraph {
    /// entity_id → entity data (O(1) lookup)
    pub entities: HashMap<String, KgEntity>,
    /// entity_id → [(neighbor_id, relation_type, weight)] (O(1) neighbor access)
    pub adjacency: HashMap<String, Vec<(String, RelationType, f64)>>,
    /// Reverse adjacency for bidirectional traversal
    pub reverse_adj: HashMap<String, Vec<(String, RelationType, f64)>>,
    /// entity_type → [entity_id] (filtered traversal)
    pub by_type: HashMap<EntityType, Vec<String>>,
    /// community_id → [entity_id] (community queries)
    pub communities: HashMap<i64, Vec<String>>,
    /// Centrality cache: entity_id → PageRank score
    pub centrality: HashMap<String, f64>,
}

pub struct KgEntity {
    pub entity_id: String,
    pub entity_type: EntityType,
    pub canonical_name: String,
    pub display_name: String,
    pub properties: serde_json::Value,
    pub embedding: Option<Vec<f32>>,
    pub centrality: Option<f64>,
    pub community_id: Option<i64>,
    pub source_ref: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum EntityType {
    Video,
    Channel,
    Tag,
    Keyword,
    Topic,
    Entity,  // NLP-extracted
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum RelationType {
    Tags,
    CreatedBy,
    AboutTopic,
    CompetesIn,
    Dominates,
    RelatedTo,
    SimilarTo,
    MentionedIn,
    Contains,
}
```

### 7.2 Hybrid Retriever

```rust
/// Combines BM25 text search, vector similarity, and graph traversal.
pub struct HybridRetriever {
    pub bm25: Bm25,                           // own BM25 (lexical)
    pub knowledge_graph: KnowledgeGraph,        // Graph (structured)
    pub vector_index: Option<VectorIndex>,      // HNSW (deferred — None until pipeline ships)
    pub scores: HashMap<String, ScoreRow>,      // Pre-computed scores
}

/// Retrieval result with full provenance chain (anti-ROT).
pub struct RetrievalResult {
    pub entity_id: String,
    pub entity_type: EntityType,
    pub score: f64,
    pub provenance: Vec<ProvenanceStep>,  // WHY this was retrieved
    pub depth: usize,                      // Graph hops from query
    pub context: RetrievalContext,         // Neighborhood snapshot
}

/// Prevents context ROT — carries the full reasoning chain.
pub struct ProvenanceStep {
    pub from: String,
    pub to: String,
    pub relation: RelationType,
    pub weight: f64,
    pub signal: SignalType,  // BM25 | Vector | Graph | Score
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalType {
    Bm25,
    Vector,
    Graph,
    Score,
}
```

### 7.3 Anti-ROT Retrieval Pipeline

```
Query: "rust async tutorial"
    │
    ▼
┌──────────────────────────────────────────────────┐
│ 1. BM25 RECALL (lexical)                        │
│    → Find videos matching "rust", "async", "tutorial" │
│    → Returns: [v1, v2, v3, ...] with BM25 scores │
└──────────────────┬───────────────────────────────┘
                   │
                   ▼
┌──────────────────────────────────────────────────┐
│ 2. VECTOR RECALL (semantic)                      │
│    → Embed query, cosine similarity vs all videos │
│    → Returns: [v1, v5, v7, ...] with sim scores  │
└──────────────────┬───────────────────────────────┘
                   │
                   ▼
┌──────────────────────────────────────────────────┐
│ 3. GRAPH EXPANSION (structured)                  │
│    → For each candidate video:                   │
│      - Get its tags → find related tags          │
│      - Get its channel → find competitor channels │
│      - Get its topic → find topic cluster        │
│      - Get its keywords → find related keywords  │
│    → Returns: expanded set with provenance chains │
└──────────────────┬───────────────────────────────┘
                   │
                   ▼
┌──────────────────────────────────────────────────┐
│ 4. CONTEXT PRESERVATION (the anti-ROT step)      │
│    → For each result, build a context packet:    │
│      - The video itself                          │
│      - Its channel's authority (PageRank)        │
│      - Its topic cluster (community)             │
│      - Its tag relationships (tag graph)         │
│      - Its competitive landscape (who else ranks) │
│    → NO information is lost between steps        │
└──────────────────┬───────────────────────────────┘
                   │
                   ▼
┌──────────────────────────────────────────────────┐
│ 5. RANK + FUSE                                   │
│    → Weighted fusion of all signals              │
│    → Graph-aware re-ranking (centrality boost)   │
│    → Return top-K with full provenance           │
└──────────────────────────────────────────────────┘
```

---

## 8. FAQ

### Q1: Why build a Knowledge Graph on TubeForge's own store instead of Neo4j or a dedicated graph database?

**A:** Three reasons:
1. **Local-first contract**: TubeForge's core promise is zero network dependencies, zero servers, zero accounts. Neo4j requires a server process. TubeForge's own `tfdb` engine IS the local storage.
2. **Scale**: Our corpus is 1-10k videos, ~100k entities, ~1M relations. The in-memory graph + Rust algorithms handle this trivially. Neo4j is overkill at this scale (and adds 200MB+ overhead).
3. **Portability**: The KG lives in the same storage files as all other data (`kg_entities`, `kg_relations`, `kg_communities`). Backup, restore, and migration are single operations.

The Rust implementation proves this approach: PageRank in 200ms, community detection in 500ms on 10k nodes.

### Q2: Why not use a vector database like Pinecone or Weaviate?

**A:** Same local-first reasons, plus:
1. **Hybrid is the standard**: Pure vector search misses exact identifiers, codes, and proper nouns. BM25 + graph is strictly better (vector deferred).
2. **Context preservation**: Vector search returns similar chunks but loses the *why*. Graph traversal preserves provenance chains.
3. **HNSW ships but is unwired**: the module exists in `src/tfdb/hnsw.rs`; embeddings are deferred post-release.

### Q3: How does this prevent context ROT?

**A:** Standard RAG loses context because:
1. Documents are chunked flat (relationships destroyed)
2. Retrieval returns chunks, not connected knowledge
3. No provenance (you can't trace why something was retrieved)

Our solution:
1. **Graph structure preserves relationships**: Entities and relations are first-class, not derived from chunks
2. **Retrieval returns context packets**: Each result carries its neighborhood (1-2 hops), not just the entity itself
3. **Full provenance chains**: Every result shows the exact path from query → BM25 hit → vector neighbor → graph expansion
4. **Context window management**: Results are structured to maximize information density within 4096 tokens

### Q4: What about entity extraction from transcripts? Isn't that LLM-dependent?

**A:** Phase 1 (this PRD) uses **rule-based entity extraction**:
- Title tokens → topic entities
- Tag vocabulary → tag entities
- Keyword list → keyword entities
- Topic category URLs → topic entities

Phase 2 (future) adds LLM-based extraction:
- Transcript NER (named entity recognition)
- Comment sentiment entities
- Description key phrase extraction

The KG schema supports both — `entity.source` field distinguishes `system` (rule-based) from `nlp` (LLM-based).

### Q5: How does graph-aware scoring improve SEO scores?

**A:** Three new signals capture what structural scoring misses:

| Signal | What it measures | Why it matters |
|---|---|---|
| `tag_authority` | Mean centrality of channels using your tags | Tags used by authoritative channels signal quality to YouTube |
| `topic_dominance` | Your channel's share of the topic cluster | Dominating a topic builds topical authority (E-E-A-T signal) |
| `keyword_competition` | Incumbent authority for your target keyword | Low competition = easier to rank; high competition = need better content |

These integrate into the existing weighted sum (18 components total, weights re-normalized to sum 1.0).

### Q6: What's the performance impact on existing features?

**A:** Minimal — KG is lazy-loaded and cached:
- **Scoring**: +3 components, each O(1) lookup from cached KG. Total overhead: <1ms per video.
- **Ideas**: Graph-based gap detection runs in parallel with BM25 neighborhoods. Overhead: +50ms.
- **Scorecard**: PageRank is cached. Overhead: 0ms (pre-computed).
- **Dashboard**: KG build is lazy (first query triggers it). Subsequent queries use cached in-memory graph.
- **Startup**: KG deserializes from `meta` table cache in <100ms; no separate API warmup needed.

### Q7: How does the graph visualization work without JavaScript?

**A:** The visualization is server-rendered SVG (like all other TubeForge charts):
1. Server computes force-directed layout (physics simulation in Rust)
2. Server renders SVG with node positions, edge paths, labels
3. Client (browser) displays SVG with CSS styling
4. Interactivity via HTMX: click node → server recomputes local graph → SVG swap

No JavaScript chart library needed. No CDN. Fully offline.

### Q8: What happens to existing data and APIs when KG is enabled?

**A:** Nothing is deleted or modified — KG is purely additive:
- Existing tables remain unchanged
- KG tables (`kg_entities`, `kg_relations`, `kg_communities`) are additive
- Existing scores remain valid (graph components default to 0 until KG is built)
- Existing queries work unchanged (KG is opt-in via `--graph-aware` flag)
- **No new public API endpoints**: existing endpoints get additional optional fields; frontend ignores them if absent (backward compatible)

### Q9: How is the KG kept up-to-date?

**A:** Two modes, no HTTP trigger endpoint:
1. **Full rebuild**: Triggered lazily on first KG-dependent query (`kg_builder::load_or_build` builds on cache miss) or via internal `BuildMode::Full`. Clears all KG tables and rebuilds from source data. <2s for 10k videos.
2. **Incremental update**: Triggered automatically on ingest. Only processes entities where `source_ref.updated_at > since`. <100ms per new video.
3. **Auto-build**: First query against KG triggers automatic build if not yet built (lazy initialization).

### Q10: What's the storage overhead?

**A:** Estimated for 10k videos:
- `kg_entities`: ~100KB (10k video entities + 5k tag entities + 1k keyword entities + 500 topic entities)
- `kg_relations`: ~500KB (50k tag relations + 10k topic relations + 5k keyword relations + 5k similarity relations)
- `kg_communities`: ~10KB (50 communities)
- **Total**: <1MB overhead (vs ~50MB for the full database)

### Q11: How is idempotency guaranteed?

**A:** Three layers:
1. **Entity level**: `entity_id` PK + `INSERT OR REPLACE` — same entity always maps to same row
2. **Relation level**: `UNIQUE(from, to, type)` + `ON CONFLICT DO UPDATE` — same edge updates weight
3. **Build level**: Full rebuild clears all tables first; incremental uses `source_ref` + `updated_at` filtering
4. **Transaction level**: All builds are transactional — no partial state on failure

---

## 9. Launch Checklist

### Readiness Criteria

| # | Criterion | Status | Owner |
|---|---|---|---|
| 1 | Migration 009 passes on fresh + upgrade (all 8 prior versions) | ⬜ | Backend |
| 2 | KG full build <2s for 10k videos on M4 | ⬜ | Backend |
| 3 | KG incremental update <100ms per video | ⬜ | Backend |
| 4 | Hybrid retrieval <100ms for 10k videos | ⬜ | Backend |
| 5 | PageRank <200ms for 10k nodes | ⬜ | Backend |
| 6 | Louvain <500ms for 10k nodes | ⬜ | Backend |
| 7 | Graph visualization renders via enhanced existing endpoint (60fps for <500 nodes) | ⬜ | Frontend |
| 8 | Graph-aware scoring integrates without breaking existing | ⬜ | Backend |
| 9 | Idempotency: rebuild 10x produces identical state | ⬜ | Backend |
| 10 | All existing tests pass (165+) | ⬜ | QA |
| 11 | New KG tests added (>40 tests, property-based via proptest) | ⬜ | Backend |
| 12 | Graph view works within existing dashboard page (offline) | ⬜ | Frontend |
| 13 | KG lazy build (`load_or_build`) works (no HTTP build endpoint) | ⬜ | Backend |
| 14 | Backward compatible (existing DBs upgrade via migration) | ⬜ | Backend |
| 15 | Documentation updated (README, LLD, HLD) | ⬜ | Docs |
| 16 | Performance gate passed (5k videos <30s) | ⬜ | QA |
| 17 | **No `/api/kg/*` endpoints exist** — verified by API route audit | ⬜ | QA |

### Launch Phases

#### Phase 1: Core KG (Week 1-2)
- Migration 009 (schema with source_ref)
- KG builder (entity/relation creation from existing data)
- In-memory KG data structures
- Basic graph algorithms (PageRank, Louvain, BFS/DFS)
- Idempotency: full rebuild + incremental update
- Lazy loading: `kg_builder::load_or_build()` pattern

#### Phase 2: Hybrid Retrieval (Week 2-3)
- Hybrid retriever (BM25 + vector + graph)
- Provenance chain generation
- Context packet construction
- CLI query command (`tubeforge query`)

#### Phase 3: Graph-Aware Features (Week 3-4)
- Graph-aware scoring (3 new components) → enhances `GET /api/scores/{id}` with `graph_scores`
- Graph-based idea generation → enhances `GET /api/ideas/analyze`
- Graph-based gap detection → enhances `GET /api/gaps`
- Enhanced scorecard with centrality → enhances `GET /api/scorecard`
- Enhanced tags gaps → enhances `GET /api/tags/gaps`

#### Phase 4: Visualization (Week 4-5)
- Force-directed graph layout (physics simulation)
- SVG rendering (server-side via enhanced `GET /api/analysis/graph`)
- Interactive controls (filter, focus, local graph)
- Graph view embedded in existing Analysis dashboard page

#### Phase 5: Polish + Release (Week 5-6)
- Performance optimization
- Edge case handling (graceful degradation when KG not built)
- Documentation
- Agent contract tests
- Release v2.0

### Monitoring Plan

| Metric | Alert Threshold | Action |
|---|---|---|
| KG build time | >5s | Investigate bottleneck |
| Query latency | >200ms | Check cache hit rate |
| Graph visualization fps | <30fps | Reduce node count or optimize |
| Memory usage | >100MB | Check for memory leaks |
| Migration failures | >0 | Block release, fix migration |
| Test failures | >0 | Block release, fix tests |
| Idempotency violations | >0 | Block release, fix builder |

### Rollback Plan

If critical issues are found post-launch:
1. **KG tables are additive**: Can be dropped without affecting existing data
2. **Graph-aware scoring defaults to 0**: Existing scores remain valid
3. **Feature flags**: KG features can be disabled via `TUBEFORGE_KG_ENABLED=false`
4. **Migration reversible**: Migration 009 can be reverted (tables dropped)

---

## Appendix A: Entity-Relation Diagram

```
┌─────────────────────────────────────────────────────────────────────┐
│                    KNOWLEDGE GRAPH ENTITY-RELATION                   │
│                                                                     │
│  ┌──────────┐  tags      ┌──────────┐  related_to  ┌──────────┐   │
│  │  video   │───────────▶│   tag    │◀─────────────│   tag    │   │
│  │          │            │          │              │          │   │
│  │          │ created_by │          │ competes_in  │          │   │
│  │          │───────────▶│          │◀─────────────│          │   │
│  │          │            │          │              │          │   │
│  │          │ about_topic│          │ dominates    │          │   │
│  │          │───────────▶│          │◀─────────────│          │   │
│  │          │            │          │              │          │   │
│  │          │ similar_to │          │ mentioned_in │          │   │
│  │          │◀──────────▶│          │◀─────────────│          │   │
│  └──────────┘            └──────────┘              └──────────┘   │
│       │                       │                         │          │
│       │                       │                         │          │
│       ▼                       ▼                         ▼          │
│  ┌──────────┐            ┌──────────┐              ┌──────────┐   │
│  │  channel │            │  topic   │              │  keyword │   │
│  │          │            │          │              │          │   │
│  │          │ competes_with       │              │          │   │
│  │          │◀───────────────────▶│              │          │   │
│  └──────────┘            └──────────┘              └──────────┘   │
│       │                       │                         │          │
│       │                       │                         │          │
│       ▼                       ▼                         ▼          │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │                      kg_communities                          │  │
│  │                                                              │  │
│  │  (topic_cluster) ──contains──▶ [video, video, tag, topic]  │  │
│  │  (channel_group) ──contains──▶ [channel, channel]          │  │
│  │  (tag_cluster)   ──contains──▶ [tag, tag, tag]             │  │
│  └──────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────┘
```

## Appendix B: Data Flow Diagram

```
┌─────────────────────────────────────────────────────────────────────┐
│                    KNOWLEDGE GRAPH DATA FLOW                         │
│                                                                     │
│  INGEST                    BUILD                     QUERY          │
│  ─────                    ─────                     ─────          │
│                                                                     │
│  RSS/oEmbed/API ──┐                                                  │
│                   │                                                  │
│  SERP discover ───┤    ┌──────────────┐    ┌──────────────────┐    │
│                   ├───▶│  KG Builder  │───▶│  KnowledgeGraph  │    │
│  Transcript ──────┤    │              │    │  (in-memory)     │    │
│                   │    │ • entities   │    │                  │    │
│  Comments ────────┤    │ • relations  │    │ • entities       │    │
│                   │    │ • communities│    │ • adjacency      │    │
│  Heatmap ─────────┘    │ • embeddings │    │ • centrality     │    │
│                        └──────┬───────┘    │ • communities    │    │
│                               │            └────────┬─────────┘    │
│                               ▼                     │              │
│                        ┌──────────────┐             │              │
│                        │  tfdb store  │             │              │
│                        │              │             ▼              │
│                        │ kg_entities  │    ┌──────────────────┐    │
│                        │ kg_relations │    │  Hybrid Retriever │    │
│                        │ kg_communities    │                  │    │
│                        └──────────────┘    │ • BM25 recall    │    │
│                               ▲            │ • Graph expand   │    │
│                               │            │ • Graph expand   │    │
│                               │            │ • Rank + fuse    │    │
│                               │            └────────┬─────────┘    │
│                               │                     │              │
│                               │                     ▼              │
│                               │            ┌──────────────────┐    │
│                               │            │  RetrievalResult  │    │
│                               │            │                  │    │
│                               │            │ • entity         │    │
│                               │            │ • score          │    │
│                               │            │ • provenance     │    │
│                               │            │ • context        │    │
│                               │            └────────┬─────────┘    │
│                               │                     │              │
│                               │                     ▼              │
│                               │            ┌──────────────────┐    │
│                               └────────────│  Graph Viz + API  │    │
│                                            │                  │    │
│                                            │ • SVG render     │    │
│                                            │ • Provenance     │    │
│                                            │ • Context packet │    │
│                                            └──────────────────┘    │
└─────────────────────────────────────────────────────────────────────┘
```

## Appendix C: Risks & Mitigations

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| R1 | KG build too slow for large corpora | Medium | High | Incremental updates, caching, lazy build |
| R2 | Graph visualization jank on large graphs | Medium | Medium | Node limit (500), level-of-detail rendering |
| R3 | Migration 009 fails on existing DBs | Low | High | Test on all version paths, rollback plan |
| R4 | Graph signals don't improve SEO scores | Medium | High | A/B test, weight tuning via env vars |
| R5 | Memory usage grows with graph size | Low | Medium | Sparse adjacency, entity pruning |
| R6 | Context packets exceed token limit | Medium | Medium | Configurable depth, summarization |
| R7 | Idempotency violation on concurrent builds | Low | High | Transactional builds, single-writer model |

---

**APPROVAL**

| Role | Name | Date | Signature |
|---|---|---|---|
| Product | Gaurav Wankhede | 2026-08-08 | ✅ |
| Engineering | TubeForge Architecture | 2026-08-08 | ✅ |
| QA | Test Plan Author | 2026-08-08 | ⬜ |
