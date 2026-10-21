//! Hub report for `mem graph hubs`: the top-N nodes by active degree, each with
//! a deterministic grouping of its re-routable neighbours into suggested topics,
//! so the Dreamer can call `introduce-topic` straight from the JSON.

use std::collections::{BTreeSet, HashMap};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::error::MemStoreResult;

/// The by-design anchor every dream diary links to. Its degree is the diary
/// count, so it is reported as excluded rather than as a hub to decompose.
pub const DREAMS_ANCHOR_ID: &str = "project_dreams";
pub const DEFAULT_HUB_LIMIT: u32 = 5;
pub const MIN_CLUSTER_SIZE: usize = 2;
pub const MAX_CLUSTERS_PER_HUB: usize = 6;
const MIN_KEYWORD_LEN: usize = 4;
const PROVENANCE_RELATION: &str = "extracted_from";
const SUB_ANCHOR_PREFIXES: [&str; 2] = ["topic_", "role_"];
const ENTITY_KIND_PREFIX: &str = "entity:";
const NEW_TOPIC_TYPE: &str = "Topic";
const KEYWORD_STOPWORDS: [&str; 24] = [
    "with", "from", "that", "this", "into", "about", "over", "when", "what", "have", "uses",
    "used", "user", "does", "than", "then", "they", "them", "their", "there", "were", "been",
    "also", "only",
];

#[derive(Debug, Clone, Copy)]
pub struct HubQuery {
    pub threshold: u32,
    pub limit: u32,
    pub sample: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HubReport {
    pub threshold: u32,
    pub limit: u32,
    pub sample: u32,
    /// Hubs at or above the threshold before `limit` was applied.
    pub total_hubs: usize,
    pub hubs: Vec<Hub>,
    pub excluded: Vec<ExcludedHub>,
    pub existing_anchors: Vec<AnchorRef>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Hub {
    pub id: String,
    pub degree: i64,
    pub rel_dist: Vec<RelCount>,
    /// Every active edge is `extracted_from`: provenance, not structure.
    pub popular_source: bool,
    /// Inbound neighbours that already are topic/role anchors.
    pub sub_anchors: u32,
    /// Neighbours linked only by hub -> neighbour edges, which introduce-topic cannot re-route.
    pub outbound_only: u32,
    /// Re-routable neighbours no cluster claimed.
    pub unclustered: u32,
    pub clusters: Vec<HubCluster>,
    pub neighbors: Vec<HubNeighbor>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClusterBasis {
    Area,
    Keyword,
    Kind,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HubCluster {
    pub suggested_topic: String,
    pub entity_type: String,
    /// The suggested topic already exists as an entity, so the cluster reuses it.
    pub existing: bool,
    pub basis: ClusterBasis,
    pub key: String,
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RelCount {
    pub rel: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HubNeighbor {
    pub id: String,
    pub rel: String,
    pub dir: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ExcludedHub {
    pub id: String,
    pub degree: i64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AnchorRef {
    pub id: String,
    pub entity_type: String,
}

/// One re-routable neighbour as clustering sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct ClusterInput {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub area: Option<String>,
}

pub fn hub_report(conn: &Connection, query: HubQuery) -> MemStoreResult<HubReport> {
    let mut excluded = Vec::new();
    let mut candidates = Vec::new();
    for (id, degree) in hub_degrees(conn, query.threshold)? {
        if id == DREAMS_ANCHOR_ID {
            excluded.push(ExcludedHub {
                id,
                degree,
                reason: "dreams anchor".to_string(),
            });
        } else {
            candidates.push((id, degree));
        }
    }
    let total_hubs = candidates.len();
    let entities = entity_types(conn)?;
    let mut hubs = Vec::new();
    for (id, degree) in candidates.into_iter().take(query.limit as usize) {
        hubs.push(build_hub(conn, &entities, id, degree, query.sample)?);
    }
    let mut existing_anchors: Vec<AnchorRef> = entities
        .iter()
        .filter(|(id, _)| is_sub_anchor(id))
        .map(|(id, entity_type)| AnchorRef {
            id: id.clone(),
            entity_type: entity_type.clone(),
        })
        .collect();
    existing_anchors.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(HubReport {
        threshold: query.threshold,
        limit: query.limit,
        sample: query.sample,
        total_hubs,
        hubs,
        excluded,
        existing_anchors,
    })
}

/// Active relation-type histogram for one node, most frequent first.
pub fn relation_distribution(conn: &Connection, id: &str) -> MemStoreResult<Vec<RelCount>> {
    let mut stmt = conn.prepare(
        "SELECT relation_type, COUNT(*) FROM relations \
         WHERE valid_to IS NULL AND (from_id = ?1 OR to_id = ?1) \
         GROUP BY relation_type ORDER BY COUNT(*) DESC, relation_type",
    )?;
    let rows = stmt.query_map(params![id], |r| {
        Ok(RelCount {
            rel: r.get(0)?,
            count: r.get(1)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn hub_degrees(conn: &Connection, threshold: u32) -> MemStoreResult<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT node, COUNT(*) FROM (
            SELECT from_id AS node FROM relations WHERE valid_to IS NULL
            UNION ALL
            SELECT to_id AS node FROM relations WHERE valid_to IS NULL
         ) GROUP BY node HAVING COUNT(*) >= ?1 ORDER BY 2 DESC, node",
    )?;
    let rows = stmt.query_map(params![threshold as i64], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn entity_types(conn: &Connection) -> MemStoreResult<HashMap<String, String>> {
    let mut stmt = conn.prepare("SELECT id, entity_type FROM entities")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn is_sub_anchor(id: &str) -> bool {
    SUB_ANCHOR_PREFIXES
        .iter()
        .any(|prefix| id.starts_with(prefix))
}

fn build_hub(
    conn: &Connection,
    entities: &HashMap<String, String>,
    id: String,
    degree: i64,
    sample: u32,
) -> MemStoreResult<Hub> {
    let rel_dist = relation_distribution(conn, &id)?;
    let popular_source =
        !rel_dist.is_empty() && rel_dist.iter().all(|r| r.rel == PROVENANCE_RELATION);
    let inbound = distinct_ids(
        conn,
        "SELECT DISTINCT from_id FROM relations WHERE valid_to IS NULL AND to_id = ?1",
        &id,
    )?;
    let outbound = distinct_ids(
        conn,
        "SELECT DISTINCT to_id FROM relations WHERE valid_to IS NULL AND from_id = ?1",
        &id,
    )?;
    let outbound_only = outbound.difference(&inbound).count() as u32;

    let mut sub_anchors = 0u32;
    let mut inputs = Vec::new();
    for neighbor in &inbound {
        if is_sub_anchor(neighbor) && entities.contains_key(neighbor) {
            sub_anchors += 1;
        } else {
            inputs.push(cluster_input(conn, entities, neighbor)?);
        }
    }
    let (clusters, unclustered) = if popular_source {
        (Vec::new(), inputs.len())
    } else {
        cluster_neighbors(&id, &inputs, entities)
    };
    Ok(Hub {
        neighbors: neighbor_sample(conn, &id, sample)?,
        id,
        degree,
        rel_dist,
        popular_source,
        sub_anchors,
        outbound_only,
        unclustered: unclustered as u32,
        clusters,
    })
}

fn distinct_ids(conn: &Connection, sql: &str, id: &str) -> MemStoreResult<BTreeSet<String>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![id], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn cluster_input(
    conn: &Connection,
    entities: &HashMap<String, String>,
    id: &str,
) -> MemStoreResult<ClusterInput> {
    let item = conn
        .query_row(
            "SELECT kind, title, area FROM mem_items WHERE id = ?1",
            params![id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?;
    Ok(match item {
        Some((kind, title, area)) => ClusterInput {
            id: id.to_string(),
            kind,
            title,
            area,
        },
        None => ClusterInput {
            id: id.to_string(),
            kind: format!(
                "{ENTITY_KIND_PREFIX}{}",
                entities.get(id).map(String::as_str).unwrap_or("unknown")
            ),
            title: id.replace('_', " "),
            area: None,
        },
    })
}

fn neighbor_sample(conn: &Connection, id: &str, sample: u32) -> MemStoreResult<Vec<HubNeighbor>> {
    if sample == 0 {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT r.relation_type,
                CASE WHEN r.from_id = ?1 THEN r.to_id ELSE r.from_id END AS other,
                CASE WHEN r.from_id = ?1 THEN 'out' ELSE 'in' END AS dir,
                m.title
         FROM relations r
         LEFT JOIN mem_items m ON m.id = CASE WHEN r.from_id = ?1 THEN r.to_id ELSE r.from_id END
         WHERE r.valid_to IS NULL AND (r.from_id = ?1 OR r.to_id = ?1)
         ORDER BY r.relation_type, other LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![id, sample as i64], |r| {
        Ok(HubNeighbor {
            rel: r.get(0)?,
            id: r.get(1)?,
            dir: r.get(2)?,
            title: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Group re-routable neighbours by shared area, then title keyword, then kind.
/// A group must split the pool (2 <= size < remaining) and must not merely
/// repeat the hub's own name. Returns the clusters and the unclaimed count.
pub fn cluster_neighbors(
    hub_id: &str,
    neighbors: &[ClusterInput],
    entities: &HashMap<String, String>,
) -> (Vec<HubCluster>, usize) {
    let hub_tokens: BTreeSet<String> = tokens(hub_id).into_iter().collect();
    let mut pool: Vec<&ClusterInput> = neighbors.iter().collect();
    pool.sort_by(|a, b| a.id.cmp(&b.id));
    let mut clusters = ClusterSet {
        hub_id,
        entities,
        clusters: Vec::new(),
    };

    let area_groups = group_by(&pool, |n| {
        n.area
            .as_deref()
            .map(str::trim)
            .filter(|a| !a.is_empty() && !a.starts_with('_'))
            .filter(|a| !tokens(a).iter().all(|t| hub_tokens.contains(t)))
            .map(str::to_string)
    });
    clusters.take_groups(ClusterBasis::Area, area_groups, &mut pool);

    while !clusters.is_full() {
        let Some((keyword, members)) = best_keyword(&pool, &hub_tokens) else {
            break;
        };
        pool.retain(|n| !members.contains(&n.id));
        clusters.push(ClusterBasis::Keyword, keyword, members);
    }

    let kind_groups = group_by(&pool, |n| Some(n.kind.clone()));
    clusters.take_groups(ClusterBasis::Kind, kind_groups, &mut pool);
    (clusters.clusters, pool.len())
}

struct ClusterSet<'a> {
    hub_id: &'a str,
    entities: &'a HashMap<String, String>,
    clusters: Vec<HubCluster>,
}

impl ClusterSet<'_> {
    fn is_full(&self) -> bool {
        self.clusters.len() >= MAX_CLUSTERS_PER_HUB
    }

    fn take_groups(
        &mut self,
        basis: ClusterBasis,
        groups: Vec<(String, Vec<String>)>,
        pool: &mut Vec<&ClusterInput>,
    ) {
        for (key, members) in groups {
            if self.is_full() {
                return;
            }
            if members.len() >= MIN_CLUSTER_SIZE && members.len() < pool.len() {
                pool.retain(|n| !members.contains(&n.id));
                self.push(basis, key, members);
            }
        }
    }

    /// Two groups that resolve to the same topic become one cluster.
    fn push(&mut self, basis: ClusterBasis, key: String, members: Vec<String>) {
        let (suggested_topic, entity_type, existing) =
            suggest_topic(self.hub_id, &key, &members, self.entities);
        if let Some(same) = self
            .clusters
            .iter_mut()
            .find(|c| c.suggested_topic == suggested_topic)
        {
            same.members.extend(members);
            same.members.sort();
            return;
        }
        self.clusters.push(HubCluster {
            suggested_topic,
            entity_type,
            existing,
            basis,
            key,
            members,
        });
    }
}

/// Groups ordered by size descending, then key; members sorted by id.
fn group_by(
    pool: &[&ClusterInput],
    key_of: impl Fn(&ClusterInput) -> Option<String>,
) -> Vec<(String, Vec<String>)> {
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    for neighbor in pool {
        if let Some(key) = key_of(neighbor) {
            groups.entry(key).or_default().push(neighbor.id.clone());
        }
    }
    let mut groups: Vec<_> = groups.into_iter().collect();
    for (_, members) in groups.iter_mut() {
        members.sort();
    }
    groups.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(&b.0)));
    groups
}

/// The most shared title keyword that still splits the pool; ties go to the
/// alphabetically first keyword so the output is deterministic.
fn best_keyword(
    pool: &[&ClusterInput],
    hub_tokens: &BTreeSet<String>,
) -> Option<(String, Vec<String>)> {
    let mut frequency: HashMap<String, Vec<String>> = HashMap::new();
    for neighbor in pool {
        let distinct: BTreeSet<String> = tokens(&neighbor.title).into_iter().collect();
        for token in distinct {
            if is_keyword(&token) && !hub_tokens.contains(&token) {
                frequency
                    .entry(token)
                    .or_default()
                    .push(neighbor.id.clone());
            }
        }
    }
    frequency
        .into_iter()
        .filter(|(_, members)| members.len() >= MIN_CLUSTER_SIZE && members.len() < pool.len())
        .max_by(|a, b| a.1.len().cmp(&b.1.len()).then_with(|| b.0.cmp(&a.0)))
}

fn tokens(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

fn is_keyword(token: &str) -> bool {
    token.len() >= MIN_KEYWORD_LEN
        && !token.chars().all(|c| c.is_ascii_digit())
        && !KEYWORD_STOPWORDS.contains(&token)
}

fn slug(text: &str) -> String {
    tokens(text.trim_start_matches(ENTITY_KIND_PREFIX)).join("_")
}

/// Reuse an existing `topic_<key>` anchor when one exists; otherwise name a
/// new topic under the hub (`topic_<hub-name>_<key>`).
fn suggest_topic(
    hub_id: &str,
    key: &str,
    members: &[String],
    entities: &HashMap<String, String>,
) -> (String, String, bool) {
    let key_slug = slug(key);
    let usable = |id: &str| id != hub_id && !members.iter().any(|m| m == id);
    let shared = format!("topic_{key_slug}");
    if let Some(entity_type) = entities.get(&shared).filter(|_| usable(shared.as_str())) {
        return (shared, entity_type.clone(), true);
    }
    let hub_name = hub_id.split_once('_').map_or(hub_id, |(_, rest)| rest);
    let scoped = format!("topic_{}_{}", slug(hub_name), key_slug);
    match entities.get(&scoped).filter(|_| usable(scoped.as_str())) {
        Some(entity_type) => (scoped, entity_type.clone(), true),
        None => (scoped, NEW_TOPIC_TYPE.to_string(), false),
    }
}
