//! Enums the represent a section of a Module or a Component

use crate::{Component, Module};
use crate::ir::component::idx_spaces::SpaceId;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
/// Represents a Section in a Component
pub enum ComponentSection {
    Module,
    Alias,
    CoreType(Option<SpaceId>),
    ComponentType(Option<SpaceId>),
    ComponentImport,
    ComponentExport,
    CoreInstance,
    ComponentInstance,
    Canon,
    CustomSection,
    Component(SpaceId),
    ComponentStartSection,
}
impl ComponentSection {
    pub fn space_id(&self) -> Option<SpaceId> {
        match self {
            ComponentSection::Component(id) => Some(*id),
            ComponentSection::CoreType(id)
            | ComponentSection::ComponentType(id) => *id,
            ComponentSection::Module
            | ComponentSection::Alias
            | ComponentSection::ComponentImport
            | ComponentSection::ComponentExport
            | ComponentSection::CoreInstance
            | ComponentSection::ComponentInstance
            | ComponentSection::Canon
            | ComponentSection::CustomSection
            | ComponentSection::ComponentStartSection => None
        }
    }
}
