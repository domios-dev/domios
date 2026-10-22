use super::records::{TabTodoAmendmentRecord, TabTodoChecklistRecord, TabTodoOwnWriteRecord};
use super::{mutations, own_write, queries, TabTodoRepository};
use crate::{
    repositories::{Committed, Completion},
    StorageError,
};
use rusqlite::params;

impl TabTodoRepository {
    pub fn review_proposal(
        &self,
        tab_id: String,
        generation: u64,
        revision: u64,
        proposal_json: String,
        amendments: Vec<(String, u64, TabTodoAmendmentRecord)>,
        write: Option<TabTodoOwnWriteRecord>,
        actor: Option<String>,
        at: u64,
    ) -> Completion<Committed<TabTodoChecklistRecord>> {
        let cost = proposal_json.len()
            + amendments
                .iter()
                .map(|(_, _, item)| item.cost())
                .sum::<usize>()
            + write.as_ref().map_or(0, TabTodoOwnWriteRecord::cost)
            + 256;
        self.storage.submit_mutation(cost, move |cx| {
            if proposal_json.len() > 4096 {
                return Err(StorageError::invalid("checklist proposal", "exceeds 4096 bytes"));
            }
            let current = queries::load(cx.conn, &tab_id)?
                .ok_or_else(|| StorageError::invalid("checklist", "not found"))?;
            if current.generation != generation || current.revision != revision {
                return Err(StorageError::invalid(
                    "checklist", "changed; read the current checklist before reviewing",
                ));
            }
            for (id, expected, amendment) in amendments {
                let item = current.items.iter()
                    .find(|item| item.id == id)
                    .ok_or_else(|| StorageError::invalid("checklist item", "not found"))?;
                if item.locked {
                    return Err(StorageError::invalid(
                        "checklist criterion", "seeded criteria stay locked",
                    ));
                }
                mutations::amend(
                    cx, tab_id.clone(), generation, id, expected, amendment, actor.clone(), at,
                )?;
            }
            let generation = if let Some(write) = write {
                own_write::write_as(
                    cx, tab_id.clone(), current.workspace_id, current.team_id, at, write, actor,
                )?.generation
            } else {
                generation
            };
            cx.write("UPDATE tab_todo_generations SET proposal_json = ?3, revision = revision + 1 WHERE tab_id = ?1 AND generation = ?2",
                params![tab_id, generation, proposal_json])?;
            queries::load(cx.conn, &tab_id)?
                .ok_or_else(|| StorageError::invalid("checklist", "not found after review"))
        })
    }
}
