//! Enums the represent a section of a Module or a Component

use crate::assert_registered_with_id;
use crate::ir::component::idx_spaces::{IndexSpaceOf, SpaceId, StoreHandle};
use crate::ir::component::scopes::RegistryHandle;
use wasmparser::{
    ComponentType, ComponentTypeDeclaration, CoreType, InstanceTypeDeclaration,
    ModuleTypeDeclaration,
};

#[derive(Debug, Clone, Eq, PartialEq)]
/// Represents a Section in a Component
pub enum ComponentSection {
    Module,
    Alias,
    CoreType,
    ComponentType,
    ComponentImport,
    ComponentExport,
    CoreInstance,
    ComponentInstance,
    Canon,
    CustomSection,
    Component,
    ComponentStartSection,
}

pub(crate) fn get_sections_for_comp_ty(ty: &ComponentType) -> (ComponentSection, bool) {
    let section = ComponentSection::ComponentType;
    match ty {
        ComponentType::Component(_) | ComponentType::Instance(_) => (section, true),
        ComponentType::Defined(_) | ComponentType::Func(_) | ComponentType::Resource { .. } => {
            (section, false)
        }
    }
}

pub(crate) fn get_sections_for_core_ty(ty: &CoreType) -> (ComponentSection, bool) {
    let section = ComponentSection::CoreType;
    match ty {
        CoreType::Module(_) => (section, true),
        CoreType::Rec(_) => (section, false),
    }
}

// =============================================================
// ==== Helper Functions for Section Index Space Population ====
// =============================================================

pub(crate) fn populate_space_for_comp_ty(
    ty: &ComponentType,
    registry: RegistryHandle,
    store: StoreHandle,
) {
    match ty {
        ComponentType::Component(decls) => {
            let space_id = store.borrow_mut().new_scope();
            let section = ComponentSection::ComponentType;
            registry.borrow_mut().register(ty, space_id);
            assert_registered_with_id!(registry, ty, space_id);
            // println!("\t@parse COMP_TYPE::component ADDR: {:p}\n\t\t{ty:?}", ty);

            for (idx, decl) in decls.iter().enumerate() {
                populate_space_for_comp_ty_comp_decl(
                    idx,
                    &space_id,
                    decl,
                    &section,
                    registry.clone(),
                    store.clone(),
                );
            }
        }
        ComponentType::Instance(decls) => {
            let space_id = store.borrow_mut().new_scope();
            let section = ComponentSection::ComponentType;
            registry.borrow_mut().register(ty, space_id);
            assert_registered_with_id!(registry, ty, space_id);
            // println!("\t@parse COMP_TYPE::instance ADDR: {:p}\n\t\t{ty:?}", ty);

            assert_eq!(space_id, registry.borrow().scope_entry(ty).unwrap().space);
            for (idx, decl) in decls.iter().enumerate() {
                populate_space_for_comp_ty_inst_decl(
                    idx,
                    &space_id,
                    decl,
                    &section,
                    registry.clone(),
                    store.clone(),
                );
            }
        }
        _ => {}
    }
}

fn populate_space_for_comp_ty_comp_decl(
    idx: usize,
    space_id: &SpaceId,
    decl: &ComponentTypeDeclaration,
    section: &ComponentSection,
    registry: RegistryHandle,
    handle: StoreHandle,
) {
    let space = decl.index_space_of();
    handle
        .borrow_mut()
        .assign_assumed_id(space_id, &space, section, idx);

    match decl {
        ComponentTypeDeclaration::CoreType(ty) => {
            populate_space_for_core_ty(ty, registry, handle);
        }
        ComponentTypeDeclaration::Type(ty) => {
            populate_space_for_comp_ty(ty, registry, handle);
        }
        ComponentTypeDeclaration::Alias(_)
        | ComponentTypeDeclaration::Export { .. }
        | ComponentTypeDeclaration::Import(_) => {}
    }
}

fn populate_space_for_comp_ty_inst_decl(
    idx: usize,
    space_id: &SpaceId,
    decl: &InstanceTypeDeclaration,
    section: &ComponentSection,
    registry: RegistryHandle,
    handle: StoreHandle,
) {
    let space = decl.index_space_of();
    handle
        .borrow_mut()
        .assign_assumed_id(space_id, &space, section, idx);

    match decl {
        InstanceTypeDeclaration::CoreType(ty) => {
            populate_space_for_core_ty(ty, registry, handle);
        }
        InstanceTypeDeclaration::Type(ty) => {
            populate_space_for_comp_ty(ty, registry, handle);
        }
        InstanceTypeDeclaration::Alias(_) | InstanceTypeDeclaration::Export { .. } => {}
    }
}

pub(crate) fn populate_space_for_core_ty(
    ty: &CoreType,
    registry: RegistryHandle,
    handle: StoreHandle,
) {
    match ty {
        CoreType::Module(decls) => {
            let space_id = handle.borrow_mut().new_scope();
            let section = ComponentSection::CoreType;
            registry.borrow_mut().register(ty, space_id);
            assert_registered_with_id!(registry, ty, space_id);
            // println!("\t@parse CORE_TYPE ADDR: {:p}", ty);

            for (idx, decl) in decls.iter().enumerate() {
                populate_space_for_core_module_decl(idx, &space_id, decl, &section, handle.clone());
            }
        }
        _ => {}
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
