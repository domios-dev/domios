use super::{
    EventSourceDecl, GraphNodesCap, NodeProperty, NodePropertySchema, NodeScalarSchema,
    NodeScalarType, NodeSchema,
};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct GraphNodesManifestCapability {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub converters: Option<Vec<crate::converter::Conversion>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub event_sources: Option<Vec<EventSourceDeclaration>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub steps: Option<Vec<crate::blueprint_step::StepDescriptor>>,
}

#[derive(Serialize, ts_rs::TS)]
pub(super) struct EventSourceDeclaration {
    pub kind: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub params: Option<AuthoredNodeSchema>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub payload: Option<AuthoredNodeSchema>,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub(super) enum NodeObjectType {
    Object,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub(super) enum NodeArrayType {
    Array,
}

#[derive(Serialize, ts_rs::TS)]
pub(super) struct AuthoredNodeSchema {
    #[serde(rename = "type")]
    pub kind: NodeObjectType,
    pub properties: BTreeMap<String, AuthoredNodePropertySchema>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub required: Option<Vec<String>>,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(untagged)]
pub(super) enum AuthoredNodePropertySchema {
    Scalar(AuthoredNodeScalarSchema),
    Array(AuthoredNodeArraySchema),
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct AuthoredNodeArraySchema {
    #[serde(rename = "type")]
    pub kind: NodeArrayType,
    pub items: Box<AuthoredNodeScalarSchema>,
    pub max_items: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct AuthoredNodeScalarSchema {
    #[serde(rename = "type")]
    pub kind: NodeScalarType,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub enum_values: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub max_length: Option<u32>,
}

impl GraphNodesManifestCapability {
    pub(super) fn lower(self) -> GraphNodesCap {
        GraphNodesCap {
            converters: self.converters.unwrap_or_default(),
            event_sources: self
                .event_sources
                .unwrap_or_default()
                .into_iter()
                .map(EventSourceDeclaration::lower)
                .collect(),
            steps: self.steps.unwrap_or_default(),
        }
    }
}

impl EventSourceDeclaration {
    pub(super) fn lower(self) -> EventSourceDecl {
        EventSourceDecl {
            kind: self.kind,
            title: self.title,
            params: self.params.map(AuthoredNodeSchema::lower),
            payload: self.payload.map(AuthoredNodeSchema::lower),
        }
    }
    pub(super) fn from_internal(value: &EventSourceDecl) -> Self {
        Self {
            kind: value.kind.clone(),
            title: value.title.clone(),
            params: value.params.as_ref().map(AuthoredNodeSchema::from_internal),
            payload: value
                .payload
                .as_ref()
                .map(AuthoredNodeSchema::from_internal),
        }
    }
}

impl AuthoredNodeSchema {
    pub(super) fn lower(self) -> NodeSchema {
        NodeSchema {
            properties: self
                .properties
                .into_iter()
                .map(|(name, schema)| NodeProperty {
                    name,
                    schema: schema.lower(),
                })
                .collect(),
            required: self.required.unwrap_or_default(),
        }
    }
    pub(super) fn from_internal(value: &NodeSchema) -> Self {
        Self {
            kind: NodeObjectType::Object,
            properties: value
                .properties
                .iter()
                .map(|p| {
                    (
                        p.name.clone(),
                        AuthoredNodePropertySchema::from_internal(&p.schema),
                    )
                })
                .collect(),
            required: (!value.required.is_empty()).then(|| value.required.clone()),
        }
    }
}

impl AuthoredNodePropertySchema {
    fn lower(self) -> NodePropertySchema {
        match self {
            Self::Scalar(value) => NodePropertySchema::Scalar(value.lower()),
            Self::Array(value) => NodePropertySchema::Array {
                description: value.description,
                items: Box::new(value.items.lower()),
                max_items: value.max_items,
            },
        }
    }
    pub(super) fn from_internal(value: &NodePropertySchema) -> Self {
        match value {
            NodePropertySchema::Scalar(value) => {
                Self::Scalar(AuthoredNodeScalarSchema::from_internal(value))
            }
            NodePropertySchema::Array {
                description,
                items,
                max_items,
            } => Self::Array(AuthoredNodeArraySchema {
                kind: NodeArrayType::Array,
                description: description.clone(),
                items: Box::new(AuthoredNodeScalarSchema::from_internal(items)),
                max_items: *max_items,
            }),
        }
    }
}

impl AuthoredNodeScalarSchema {
    fn lower(self) -> NodeScalarSchema {
        NodeScalarSchema {
            kind: self.kind,
            description: self.description,
            enum_values: self.enum_values,
            max_length: self.max_length,
        }
    }
    pub(super) fn from_internal(value: &NodeScalarSchema) -> Self {
        Self {
            kind: value.kind,
            description: value.description.clone(),
            enum_values: value.enum_values.clone(),
            max_length: value.max_length,
        }
    }
}
