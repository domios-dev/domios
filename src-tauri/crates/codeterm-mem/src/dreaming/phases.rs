use super::DreamReport;
use crate::MemManager;
pub async fn light_phase(mem: &MemManager) -> Result<DreamReport, String> {
    let _permit = mem.acquire_heavy_permit().await;

    const BATCH: usize = 8;

    let now_ts = chrono::Utc::now();
    let one_day_ago = (now_ts - chrono::Duration::days(1)).to_rfc3339();

    let candidates = mem.store.dream_candidates(&one_day_ago)?;

    let mut considered = 0u64;
    let mut promoted = 0u64;

    for (batch_idx, chunk) in candidates.chunks(BATCH).enumerate() {
        for cand in chunk {
            considered += 1;
            let promote = mem.store.dream_promote(
                &crate::backend::WriteContext::system("dreaming:rem"),
                cand,
                now_ts,
            )?;
            if promote {
                promoted += 1;
            }
        }

        if batch_idx + 1 < candidates.chunks(BATCH).len() {
            tokio::task::yield_now().await;
        }
    }

    Ok(DreamReport {
        considered,
        promoted,
        dropped: 0,
        phase: "light".into(),
    })
}

pub async fn deep_phase(mem: &MemManager) -> Result<DreamReport, String> {
    let _permit = mem.acquire_heavy_permit().await;

    let tiers = codeterm_mem_store::maintenance::DEEP_TIERS;
    let mut total_expired = 0u64;

    for (idx, (kind, days)) in tiers.iter().enumerate() {
        let expired = mem.store.dream_expire(
            &crate::backend::WriteContext::system("dreaming:deep"),
            kind,
            *days,
        )?;
        total_expired += expired;

        if idx + 1 < tiers.len() {
            tokio::task::yield_now().await;
        }
    }

    Ok(DreamReport {
        considered: total_expired,
        promoted: 0,
        dropped: total_expired,
        phase: "deep".into(),
    })
}

pub async fn normalize_phase(mem: &MemManager) -> Result<DreamReport, String> {
    let _permit = mem.acquire_heavy_permit().await;
    let r = mem
        .store
        .dream_normalize(&crate::backend::WriteContext::system("dreaming:normalize"))?;
    Ok(DreamReport {
        considered: r["flagged_entity_pairs"]
            .as_array()
            .map_or(0, |pairs| pairs.len() as u64),
        promoted: r["merged"].as_u64().unwrap_or(0),
        dropped: r["orphans_deleted"].as_u64().unwrap_or(0)
            + r["broken_relations_invalidated"].as_u64().unwrap_or(0),
        phase: "normalize".into(),
    })
}
