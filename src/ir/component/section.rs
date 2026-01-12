//! Enums the represent a section of a Module or a Component

use crate::ir::component::idx_spaces::{IndexSpaceOf, SpaceId, StoreHandle};
use crate::{Component, Module};
use wasmparser::{
    ComponentType, ComponentTypeDeclaration, CoreType, InstanceTypeDeclaration,
    ModuleTypeDeclaration,
};

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
            ComponentSection::CoreType(id) | ComponentSection::ComponentType(id) => *id,
            ComponentSection::Module
            | ComponentSection::Alias
            | ComponentSection::ComponentImport
            | ComponentSection::ComponentExport
            | ComponentSection::CoreInstance
            | ComponentSection::ComponentInstance
            | ComponentSection::Canon
            | ComponentSection::CustomSection
            | ComponentSection::ComponentStartSection => None,
        }
    }
}

// =============================================================
// ==== Helper Functions for Section Index Space Population ====
// =============================================================

pub(crate) fn populate_space_for_comp_ty(
    ty: &ComponentType,
    handle: StoreHandle,
) -> ComponentSection {
    match ty {
        ComponentType::Component(decls) => {
            let space_id = handle.borrow_mut().new_scope();
            let section = ComponentSection::ComponentType(Some(space_id.clone()));
            for (idx, decl) in decls.iter().enumerate() {
                populate_space_for_comp_ty_comp_decl(
                    idx,
                    &space_id,
                    decl,
                    &section,
                    handle.clone(),
                );
            }

            section
        }
        ComponentType::Instance(decls) => {
            let space_id = handle.borrow_mut().new_scope();
            let section = ComponentSection::ComponentType(Some(space_id.clone()));
            for (idx, decl) in decls.iter().enumerate() {
                populate_space_for_comp_ty_inst_decl(
                    idx,
                    &space_id,
                    decl,
                    &section,
                    handle.clone(),
                );
            }

            section
        }
        _ => ComponentSection::ComponentType(None),
    }
}

fn populate_space_for_comp_ty_comp_decl(
    idx: usize,
    space_id: &SpaceId,
    decl: &ComponentTypeDeclaration,
    section: &ComponentSection,
    handle: StoreHandle,
) {
    let space = decl.index_space_of();
    handle
        .borrow_mut()
        .assign_assumed_id(space_id, &space, section, idx);
}

fn populate_space_for_comp_ty_inst_decl(
    idx: usize,
    space_id: &SpaceId,
    decl: &InstanceTypeDeclaration,
    section: &ComponentSection,
    handle: StoreHandle,
) {
    let space = decl.index_space_of();
    handle
        .borrow_mut()
        .assign_assumed_id(space_id, &space, section, idx);
}

pub(crate) fn populate_space_for_core_ty(ty: &CoreType, handle: StoreHandle) -> ComponentSection {
    match ty {
        CoreType::Module(decls) => {
            let space_id = handle.borrow_mut().new_scope();
            let section = ComponentSection::CoreType(Some(space_id.clone()));
            for (idx, decl) in decls.iter().enumerate() {
                populate_space_for_core_module_decl(idx, &space_id, decl, &section, handle.clone());
            }

            section
        }
        _ => ComponentSection::CoreType(None),
    }
}

fn populate_space_for_core_module_decl(
    idx: usize,
    space_id: &SpaceId,
    decl: &ModuleTypeDeclaration,
    section: &ComponentSection,
    handle: StoreHandle,
) {
    let space = decl.index_space_of();
    handle
        .borrow_mut()
        .assign_assumed_id(space_id, &space, section, idx);
}
