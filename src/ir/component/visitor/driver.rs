use wasmparser::{CanonicalFunction, ComponentAlias, ComponentExport, ComponentImport, ComponentInstance, ComponentStartFunction, ComponentType, ComponentTypeDeclaration, CoreType, Instance, InstanceTypeDeclaration, ModuleTypeDeclaration, SubType};
use crate::{Component, Module};
use crate::ir::component::idx_spaces::{IndexSpaceOf, Space};
use crate::ir::component::section::ComponentSection;
use crate::ir::component::visitor::{ComponentVisitor, ItemKind, VisitCtx};
use crate::ir::types::CustomSection;

pub fn drive_event<'ir, V: ComponentVisitor<'ir>>(
    event: VisitEvent<'ir>,
    visitor: &mut V,
    ctx: &mut VisitCtx<'ir>,
) {
    match event {
        VisitEvent::EnterRootComp { component } => {
            ctx.inner.push_component(component);
            visitor.enter_root_component(ctx, component);
        }

        VisitEvent::ExitRootComp { component } => {
            visitor.exit_root_component(ctx, component);
        }
        VisitEvent::EnterComp { component, idx, .. } => {
            ctx.inner.push_component(component);

            // TODO: This seems like the wrong time to do the lookup
            //       (should it be before `push_component`?)
            let id = ctx.inner.lookup_id_for(
                &Space::Comp,
                &ComponentSection::Component,
                idx,
            );
            visitor.enter_component(ctx, id, component);
        }

        VisitEvent::ExitComp { component, idx } => {
            let id = ctx.inner.lookup_id_for(
                &Space::Comp,
                &ComponentSection::Component,
                idx,
            );
            ctx.inner.pop_component();
            visitor.exit_component(ctx, id, component);
        }

        VisitEvent::Module { idx, module } => {
            ctx.inner.maybe_enter_scope(module);
            let id = ctx.inner.lookup_id_for(
                &Space::CoreModule,
                &ComponentSection::Module,
                idx,
            );
            visitor.visit_module(ctx, id, module);
            ctx.inner.maybe_exit_scope(module);
        }

        VisitEvent::CompInst { idx, inst } => {
            ctx.inner.maybe_enter_scope(inst);
            let id = ctx.inner.lookup_id_for(
                &Space::CompInst,
                &ComponentSection::ComponentInstance,
                idx,
            );
            visitor.visit_comp_instance(ctx, id, inst);
            ctx.inner.maybe_exit_scope(inst);
        }

        VisitEvent::EnterCompType {idx, ty } => {
            ctx.inner.maybe_enter_scope(ty);
            let id = ctx.inner.lookup_id_for(
                &Space::CompType,
                &ComponentSection::ComponentType,
                idx,
            );
            visitor.enter_comp_type(ctx, id, ty);
        }

        VisitEvent::CompTypeDecl {idx, parent, decl } => {
            ctx.inner.maybe_enter_scope(decl);
            let id = ctx.inner.lookup_id_for(
                &decl.index_space_of(),
                &ComponentSection::ComponentType,
                idx,
            );
            visitor.visit_comp_type_decl(ctx, idx, id, parent, decl);
            ctx.inner.maybe_exit_scope(decl);
        }

        VisitEvent::InstTypeDecl {idx, parent, decl } => {
            ctx.inner.maybe_enter_scope(decl);
            let id = ctx.inner.lookup_id_for(
                &decl.index_space_of(),
                &ComponentSection::ComponentType,
                idx,
            );
            visitor.visit_inst_type_decl(ctx, idx, id, parent, decl);
            ctx.inner.maybe_exit_scope(decl);
        }

        VisitEvent::ExitCompType {idx, ty } => {
            let id = ctx.inner.lookup_id_for(
                &Space::CompType,
                &ComponentSection::ComponentType,
                idx,
            );
            ctx.inner.maybe_exit_scope(ty);
            visitor.exit_comp_type(ctx, id, ty);
        }

        VisitEvent::Canon { kind, idx, canon } => {
            ctx.inner.maybe_enter_scope(canon);
            let space = canon.index_space_of();
            let id = ctx.inner.lookup_id_for(
                &space,
                &ComponentSection::Canon,
                idx,
            );
            visitor.visit_canon(ctx, kind, id, canon);
            ctx.inner.maybe_exit_scope(canon);
        }
        VisitEvent::Alias { kind, idx, alias } => {
            ctx.inner.maybe_enter_scope(alias);
            let space = alias.index_space_of();
            let id = ctx.inner.lookup_id_for(
                &space,
                &ComponentSection::Alias,
                idx,
            );
            visitor.visit_alias(ctx, kind, id, alias);
            ctx.inner.maybe_exit_scope(alias);
        }
        VisitEvent::Import { kind, idx, imp } => {
            ctx.inner.maybe_enter_scope(imp);
            let space = imp.index_space_of();
            let id = ctx.inner.lookup_id_for(
                &space,
                &ComponentSection::ComponentImport,
                idx,
            );
            visitor.visit_comp_import(ctx, kind, id, imp);
            ctx.inner.maybe_exit_scope(imp);
        }
        VisitEvent::Export { kind, idx, exp } => {
            ctx.inner.maybe_enter_scope(exp);
            let space = exp.index_space_of();
            let id = ctx.inner.lookup_id_for(
                &space,
                &ComponentSection::ComponentExport,
                idx,
            );
            visitor.visit_comp_export(ctx, kind, id, exp);
            ctx.inner.maybe_exit_scope(exp);
        }
        VisitEvent::EnterCoreRecGroup { count, ty } => {
            visitor.enter_core_rec_group(ctx, count, ty);
        }
        VisitEvent::CoreSubtype { parent_idx, subvec_idx, subtype } => {
            ctx.inner.maybe_enter_scope(subtype);
            let id = ctx.inner.lookup_id_with_subvec_for(
                &Space::CoreType,
                &ComponentSection::CoreType,
                parent_idx,
                subvec_idx,
            );
            visitor.visit_core_subtype(ctx, id, subtype);
            ctx.inner.maybe_exit_scope(subtype);
        }
        VisitEvent::ExitCoreRecGroup { } => {
            visitor.exit_core_rec_group(ctx);
        }
        VisitEvent::EnterCoreType { idx, ty } => {
            ctx.inner.maybe_enter_scope(ty);
            let id = ctx.inner.lookup_id_for(
                &Space::CoreType,
                &ComponentSection::CoreType,
                idx,
            );
            visitor.enter_core_type(ctx, id, ty);
        }
        VisitEvent::ModuleTypeDecl {idx, parent, decl } => {
            ctx.inner.maybe_enter_scope(decl);
            let id = ctx.inner.lookup_id_for(
                &decl.index_space_of(),
                &ComponentSection::CoreType,
                idx,
            );
            visitor.visit_module_type_decl(ctx, idx, id, parent, decl);
            ctx.inner.maybe_exit_scope(decl);
        }
        VisitEvent::ExitCoreType {idx, ty } => {
            let id = ctx.inner.lookup_id_for(
                &Space::CoreType,
                &ComponentSection::CoreType,
                idx,
            );
            ctx.inner.maybe_exit_scope(ty);
            visitor.exit_core_type(ctx, id, ty);
        }
        VisitEvent::CoreInst { idx, inst } => {
            ctx.inner.maybe_enter_scope(inst);
            let id = ctx.inner.lookup_id_for(
                &Space::CoreInst,
                &ComponentSection::CoreInstance,
                idx,
            );
            visitor.visit_core_instance(ctx, id, inst);
            ctx.inner.maybe_exit_scope(inst);
        }
        VisitEvent::CustomSection { sect } => {
            ctx.inner.maybe_enter_scope(sect);
            visitor.visit_custom_section(ctx, sect);
            ctx.inner.maybe_exit_scope(sect);
        }
        VisitEvent::StartFunc { func } => {
            ctx.inner.maybe_enter_scope(func);
            visitor.visit_start_section(ctx, func);
            ctx.inner.maybe_exit_scope(func);
        }
    }
}

pub enum VisitEvent<'ir> {
    EnterRootComp {
        component: &'ir Component<'ir>,
    },
    ExitRootComp {
        component: &'ir Component<'ir>,
    },
    EnterComp {
        idx: usize,
        component: &'ir Component<'ir>,
    },
    ExitComp {
        idx: usize,
        component: &'ir Component<'ir>,
    },
    Module {
        idx: usize,
        module: &'ir Module<'ir>,
    },

    // ------------------------
    // Component-level items
    // ------------------------

    EnterCompType {
        idx: usize,
        ty: &'ir ComponentType<'ir>,
    },
    ExitCompType {
        idx: usize,
        ty: &'ir ComponentType<'ir>,
    },
    // subitems of a component type
    CompTypeDecl {
        parent: &'ir ComponentType<'ir>,
        /// index in the decl vector
        idx: usize,
        decl: &'ir ComponentTypeDeclaration<'ir>,
    },
    InstTypeDecl {
        parent: &'ir ComponentType<'ir>,
        /// index in the decl vector
        idx: usize,
        decl: &'ir InstanceTypeDeclaration<'ir>,
    },

    CompInst {
        idx: usize,
        inst: &'ir ComponentInstance<'ir>,
    },

    // ------------------------------------------------
    // Items with multiple possible resolved namespaces
    // ------------------------------------------------
    Canon {
        kind: ItemKind,
        idx: usize,
        canon: &'ir CanonicalFunction,
    },
    Alias {
        kind: ItemKind,
        idx: usize,
        alias: &'ir ComponentAlias<'ir>,
    },
    Import {
        kind: ItemKind,
        idx: usize,
        imp: &'ir ComponentImport<'ir>,
    },
    Export {
        kind: ItemKind,
        idx: usize,
        exp: &'ir ComponentExport<'ir>,
    },

    // ------------------------
    // Core WebAssembly items
    // ------------------------
    EnterCoreRecGroup {
        ty: &'ir CoreType<'ir>,
        count: usize,
    },
    CoreSubtype {
        parent_idx: usize,
        subvec_idx: usize,
        subtype: &'ir SubType
    },
    ExitCoreRecGroup {},
    EnterCoreType {
        idx: usize,
        ty: &'ir CoreType<'ir>,
    },
    ModuleTypeDecl {
        parent: &'ir CoreType<'ir>,
        /// index in the decl vector
        idx: usize,
        decl: &'ir ModuleTypeDeclaration<'ir>,
    },
    ExitCoreType {
        idx: usize,
        ty: &'ir CoreType<'ir>,
    },
    CoreInst {
        idx: usize,
        inst: &'ir Instance<'ir>,
    },

    // ------------------------
    // Sections
    // ------------------------
    CustomSection {
        sect: &'ir CustomSection<'ir>,
    },
    StartFunc {
        func: &'ir ComponentStartFunction
    },
}
impl<'ir> VisitEvent<'ir> {
    pub fn enter_root_comp(component: &'ir Component<'ir>) -> Self {
        Self::EnterRootComp { component }
    }
    pub fn exit_root_comp(component: &'ir Component<'ir>) -> Self {
        Self::ExitRootComp { component }
    }
    pub fn enter_comp(idx: usize, component: &'ir Component<'ir>) -> Self {
        Self::EnterComp { idx, component }
    }
    pub fn exit_comp(idx: usize, component: &'ir Component<'ir>) -> Self {
        Self::ExitComp { idx, component }
    }
    pub fn module(_: ItemKind, idx: usize, module: &'ir Module<'ir>) -> Self {
        Self::Module { idx, module }
    }
    pub fn enter_comp_type(_: ItemKind, idx: usize, ty: &'ir ComponentType<'ir>) -> Self {
        Self::EnterCompType { idx, ty }
    }
    pub fn comp_type_decl(parent: &'ir ComponentType<'ir>, idx: usize, decl: &'ir ComponentTypeDeclaration<'ir>) -> Self {
        Self::CompTypeDecl { parent, idx, decl }
    }
    pub fn inst_type_decl(parent: &'ir ComponentType<'ir>, idx: usize, decl: &'ir InstanceTypeDeclaration<'ir>) -> Self {
        Self::InstTypeDecl { parent, idx, decl }
    }
    pub fn exit_comp_type(_: ItemKind, idx: usize, ty: &'ir ComponentType<'ir>) -> Self {
        Self::ExitCompType { idx, ty }
    }
    pub fn comp_inst(_: ItemKind, idx: usize, inst: &'ir ComponentInstance<'ir>) -> Self {
        Self::CompInst { idx, inst }
    }
    pub fn canon(kind: ItemKind, idx: usize, canon: &'ir CanonicalFunction) -> Self {
        Self::Canon { kind, idx, canon }
    }
    pub fn alias(kind: ItemKind, idx: usize, alias: &'ir ComponentAlias<'ir>) -> Self {
        Self::Alias { kind, idx, alias }
    }
    pub fn import(kind: ItemKind, idx: usize, imp: &'ir ComponentImport<'ir>) -> Self {
        Self::Import { kind, idx, imp }
    }
    pub fn export(kind: ItemKind, idx: usize, exp: &'ir ComponentExport<'ir>) -> Self {
        Self::Export { kind, idx, exp }
    }
    pub fn enter_rec_group(count: usize, ty: &'ir CoreType<'ir>) -> Self {
        Self::EnterCoreRecGroup { count, ty }
    }
    pub fn core_subtype(parent_idx: usize, subvec_idx: usize, subtype: &'ir SubType) -> Self {
        Self::CoreSubtype { parent_idx, subvec_idx, subtype }
    }
    pub fn exit_rec_group() -> Self {
        Self::ExitCoreRecGroup {}
    }
    pub fn enter_core_type(_: ItemKind, idx: usize, ty: &'ir CoreType<'ir>) -> Self {
        Self::EnterCoreType { idx, ty }
    }
    pub fn mod_type_decl(parent: &'ir CoreType<'ir>, idx: usize, decl: &'ir ModuleTypeDeclaration<'ir>) -> Self {
        Self::ModuleTypeDecl { parent, idx, decl }
    }
    pub fn exit_core_type(_: ItemKind, idx: usize, ty: &'ir CoreType<'ir>) -> Self {
        Self::ExitCoreType { idx, ty }
    }
    pub fn core_inst(_: ItemKind, idx: usize, inst: &'ir Instance<'ir>) -> Self {
        Self::CoreInst { idx, inst }
    }
    pub fn custom_sect(_: ItemKind, _: usize, sect: &'ir CustomSection<'ir>) -> Self {
        Self::CustomSection { sect }
    }
    pub fn start_func(_: ItemKind, _: usize, func: &'ir ComponentStartFunction) -> Self {
        Self::StartFunc { func }
    }
}
