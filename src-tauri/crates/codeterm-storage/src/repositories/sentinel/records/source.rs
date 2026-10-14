use codeterm_blueprint_types::sentinel_input::InputSpec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObserveKind {
    Tab,
    CronRun,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObserveSource {
    pub kind: ObserveKind,
    pub reference: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SourceKind {
    Probe,
    Observe,
    Input,
    Terminal,
    Pipeline,
}

impl SourceKind {
    pub const ALL: [SourceKind; 5] = [
        SourceKind::Probe,
        SourceKind::Observe,
        SourceKind::Input,
        SourceKind::Terminal,
        SourceKind::Pipeline,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::Probe => "probe",
            SourceKind::Observe => "observe",
            SourceKind::Input => "input",
            SourceKind::Terminal => "terminal",
            SourceKind::Pipeline => "pipeline",
        }
    }

    pub fn parse(value: &str) -> Option<SourceKind> {
        SourceKind::ALL
            .into_iter()
            .find(|kind| kind.as_str() == value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageSource {
    Probe(String),
    Observe(ObserveSource),
    Input(InputSpec),
    Terminal {
        binding: ObserveSource,
        config: codeterm_blueprint_types::blueprint_terminal::Config,
    },
    Pipeline(codeterm_blueprint_types::blueprint_pipeline::BlueprintSpec),
}

impl StageSource {
    pub fn kind(&self) -> SourceKind {
        match self {
            StageSource::Probe(_) => SourceKind::Probe,
            StageSource::Observe(_) => SourceKind::Observe,
            StageSource::Input(_) => SourceKind::Input,
            StageSource::Terminal { .. } => SourceKind::Terminal,
            StageSource::Pipeline(_) => SourceKind::Pipeline,
        }
    }
}
