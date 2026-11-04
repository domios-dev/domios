use super::{column, decode, record_query};
use crate::plan::{PlanMode, PlanningState};
use crate::service_plan::PlanError;
use rusqlite::{Connection, OptionalExtension};
use std::rc::Rc;

pub struct ReminderCompositionRead {
    pub id: String,
    pub mode: PlanMode,
    pub planning_state: PlanningState,
    pub owner_tab_id: Option<String>,
    pub team_id: Option<String>,
    pub(super) connection: Rc<Connection>,
}

impl ReminderCompositionRead {
    pub fn digest_input(
        &self,
        now: u64,
        targets: &crate::service_plan::NoticeInventoryTargets,
    ) -> Result<crate::service_plan::NoticeDigestInput, PlanError> {
        super::super::reminder_digests::notice_digest_input(
            &self.connection,
            &self.id,
            now,
            targets,
        )
    }
    pub fn compose_worker(&self, tab: &str, text: &str) -> Result<String, PlanError> {
        let read = super::super::super::charter_queries::worker_assignment(&self.connection, tab)?;
        crate::service_plan::worker_charter::prepend_assignment(read, text)
    }
    pub fn compose_owner(
        &self,
        directive: &str,
        digests: &[String],
        selection: Option<&[String]>,
        plain: bool,
        now: u64,
    ) -> Result<String, PlanError> {
        super::super::super::curator_notice_queries::bounded_reminder(
            &self.connection,
            &self.id,
            now,
            selection,
            digests,
            directive,
            plain,
        )
        .map(|read| read.body)
    }

    pub fn planner_resolution(
        &self,
        probe: impl Fn(&str) -> crate::plan::reconcile::TabProbe,
    ) -> Result<crate::plan::peer::PeerResolution, PlanError> {
        record_query();
        let raw:String=self.connection.query_row(
            "SELECT json_quote(CASE json_extract(header_json,'$.planner.kind') WHEN 'tab' THEN json_object('kind','tab','tab_id',json_extract(header_json,'$.planner.tab_id'),'session_id',json_extract(header_json,'$.planner.session_id')) WHEN 'waived' THEN CASE WHEN json_type(header_json,'$.planner.note')='text' THEN json_object('kind','waived','note','') ELSE json('invalid planner waiver') END ELSE CASE WHEN json_type(header_json,'$.planner') IS NULL OR json_type(header_json,'$.planner')='null' THEN NULL ELSE json('invalid planner kind') END END) FROM plans WHERE id=?1",
            [&self.id],|row|column(row,0)).map_err(super::super::super::db_error)?;
        let planner: Option<crate::plan::PlannerRef> = decode(&self.id, &raw)?;
        Ok(crate::plan::peer::resolve_planner_ref(
            &planner,
            &crate::plan::OrchestratorRef::default(),
            probe,
        ))
    }
}

pub(super) fn read(
    connection: Rc<Connection>,
    id: &str,
) -> Result<Option<ReminderCompositionRead>, PlanError> {
    #[derive(serde::Deserialize)]
    struct Header {
        mode: PlanMode,
        planning_state: PlanningState,
        owner_tab_id: Option<String>,
        team_id: Option<String>,
    }
    record_query();
    let raw:Option<String>=connection.query_row("SELECT json_object('mode',coalesce(json_extract(header_json,'$.mode'),'planning'),'planning_state',coalesce(json_extract(header_json,'$.planning_state'),'drafting'),'owner_tab_id',owner_tab_id,'team_id',json_extract(header_json,'$.team_id')) FROM plans WHERE id=?1",[id],|row|column(row,0)).optional().map_err(super::super::super::db_error)?;
    let Some(raw) = raw else { return Ok(None) };
    let header: Header = decode(id, &raw)?;
    Ok(Some(ReminderCompositionRead {
        id: id.into(),
        mode: header.mode,
        planning_state: header.planning_state,
        owner_tab_id: header.owner_tab_id,
        team_id: header.team_id,
        connection,
    }))
}
