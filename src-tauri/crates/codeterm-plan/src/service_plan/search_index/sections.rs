use super::*;

pub(in crate::service_plan) struct SectionIndexDelta {
    pub plan_id: String,
    pub title: String,
    pub status: String,
    pub project_root: PathBuf,
    pub updated_at: u64,
    pub section: Option<Section>,
    pub removed: Vec<String>,
    pub ordinal: usize,
    pub total: usize,
}

pub(in crate::service_plan) fn enqueue_section_index(
    _turn: &MutationWriterTurn<'_>,
    delta: SectionIndexDelta,
) {
    let scope = project_scope(&delta.project_root);
    let parent = unit_inputs::UnitParent {
        id: &delta.plan_id,
        title: &delta.title,
        scope: &scope,
        status: &delta.status,
    };
    index_queue().enqueue(IndexJob::Unit(
        delta.project_root.clone(),
        parent.header("", delta.updated_at),
    ));
    for id in delta.removed {
        index_queue().enqueue(IndexJob::DeleteUnit(
            delta.project_root.clone(),
            stable_id(&delta.plan_id, "plan_section", &id),
        ));
    }
    if let Some(section) = delta.section {
        let mut ordinal = delta.ordinal;
        enqueue_tree(
            &delta.project_root,
            &parent,
            &section,
            &mut ordinal,
            delta.total,
            delta.updated_at,
        );
    }
}

fn enqueue_tree(
    root: &Path,
    parent: &unit_inputs::UnitParent<'_>,
    section: &Section,
    ordinal: &mut usize,
    total: usize,
    at: u64,
) {
    index_queue().enqueue(IndexJob::Unit(
        root.to_path_buf(),
        parent.section(
            &section.id,
            &section.title,
            &section.body,
            *ordinal,
            total,
            at,
        ),
    ));
    *ordinal += 1;
    for child in &section.subsections {
        enqueue_tree(root, parent, child, ordinal, total, at);
    }
}
