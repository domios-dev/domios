use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
use crate::service_plan::{NoticeDeliveryTargetRead, PlanError, PlanIdentityRead};
use rusqlite::OptionalExtension;

impl PlanDatabase {
    pub(in crate::service_plan) fn notice_delivery_target_projection(
        &self,
        id: &str,
    ) -> Result<NoticeDeliveryTargetRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let encoded: String = connection.query_row(
            "SELECT json_object('owner_tab_id',CASE WHEN json_type(header_json,'$.owner')='object'
                THEN json_extract(header_json,'$.owner.tab_id') ELSE json('{}') END,
                'mode',CASE WHEN json_type(header_json,'$.mode') IS NULL THEN 'planning'
                ELSE json_extract(header_json,'$.mode') END) FROM plans WHERE id=?1",
            [id], |row| column(row,0)).optional().map_err(db_error)?.ok_or(PlanError::NotFound)?;
        decode(id, &encoded)
    }

    pub(in crate::service_plan) fn plan_identity_projection(
        &self,
        id: &str,
    ) -> Result<PlanIdentityRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let encoded: String = connection
            .query_row(
                "SELECT json_object('owner',json_extract(header_json,'$.owner'),'team_id',team_id)
             FROM plans WHERE id=?1",
                [id],
                |row| column(row, 0),
            )
            .optional()
            .map_err(db_error)?
            .ok_or(PlanError::NotFound)?;
        decode(id, &encoded)
    }
}
