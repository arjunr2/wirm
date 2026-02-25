use crate::ir::component::idx_spaces::IndexSpaceOf;
use crate::ir::component::section::ComponentSection;
use crate::ir::component::visitor::driver::VisitEvent;
use crate::ir::component::visitor::utils::{emit_indexed, for_each_indexed};
use crate::ir::component::visitor::VisitCtx;
use crate::Component;
use wasmparser::{
    ComponentType, ComponentTypeDeclaration, CoreType, InstanceTypeDeclaration,
    ModuleTypeDeclaration,
};

pub(crate) fn get_structural_events<'ir>(
    component: &'ir Component<'ir>,
    ctx: &mut VisitCtx<'ir>,
    out: &mut Vec<VisitEvent<'ir>>,
) {
    ctx.inner.push_comp_section_tracker();
    out.push(VisitEvent::enter_root_comp(component));

    visit_comp(component, ctx, out);

    out.push(VisitEvent::exit_root_comp(component));
    ctx.inner.pop_component();
}
fn visit_comp<'ir>(
    component: &'ir Component<'ir>,
    ctx: &mut VisitCtx<'ir>,
    out: &mut Vec<VisitEvent<'ir>>,
) {
    for (num, section) in component.sections.iter() {
        let count = *num as usize;
        let start_idx = ctx.inner.visit_section(section, count);

        match section {
            ComponentSection::Component => {
                for_each_indexed(&component.components, start_idx, count, |idx, sub| {
                    ctx.inner.push_comp_section_tracker();
                    out.push(VisitEvent::enter_comp(idx, sub));
                    visit_comp(sub, ctx, out);
                    ctx.inner.pop_comp_section_tracker();
                    out.push(VisitEvent::exit_comp(idx, sub));
                });
            }

            ComponentSection::Module => {
                for_each_indexed(&component.modules.vec, start_idx, count, |idx, module| {
                    emit_indexed(out, idx, module, VisitEvent::module)
                });
            }

            ComponentSection::ComponentType => {
                for_each_indexed(
                    &component.component_types.items,
                    start_idx,
                    count,
                    |idx, ty| visit_comp_type(idx, ty, out),
                );
            }

            ComponentSection::ComponentInstance => {
                for_each_indexed(
                    &component.component_instance,
                    start_idx,
                    count,
                    |idx, inst| emit_indexed(out, idx, inst, VisitEvent::comp_inst),
                );
            }

            ComponentSection::CoreInstance => {
                for_each_indexed(&component.instances, start_idx, count, |idx, inst| {
                    emit_indexed(out, idx, inst, VisitEvent::core_inst)
                });
            }

            ComponentSection::CoreType => {
                for_each_indexed(&component.core_types, start_idx, count, |idx, ty| {
                    visit_core_type(idx, ty, out)
                });
            }

            ComponentSection::Canon => {
                for_each_indexed(&component.canons.items, start_idx, count, |idx, canon| {
                    emit_indexed(out, idx, canon, VisitEvent::canon)
                });
            }

            ComponentSection::ComponentExport => {
                for_each_indexed(&component.exports, start_idx, count, |idx, export| {
                    emit_indexed(out, idx, export, VisitEvent::export)
                });
            }

            ComponentSection::ComponentImport => {
                for_each_indexed(&component.imports, start_idx, count, |idx, import| {
                    emit_indexed(out, idx, import, VisitEvent::import)
                });
            }

            ComponentSection::Alias => {
                for_each_indexed(&component.alias.items, start_idx, count, |idx, alias| {
                    emit_indexed(out, idx, alias, VisitEvent::alias)
                });
            }

            ComponentSection::CustomSection => {
                for_each_indexed(
                    &component.custom_sections.custom_sections,
                    start_idx,
                    count,
                    |idx, sect| emit_indexed(out, idx, sect, VisitEvent::custom_sect),
                );
            }

            ComponentSection::ComponentStartSection => {
                for_each_indexed(&component.start_section, start_idx, count, |idx, func| {
                    emit_indexed(out, idx, func, VisitEvent::start_func)
                });
            }
        }
    }
}
fn visit_comp_type<'ir>(idx: usize, ty: &'ir ComponentType<'ir>, out: &mut Vec<VisitEvent<'ir>>) {
    out.push(VisitEvent::enter_comp_type(
        ty.index_space_of().into(),
        idx,
        ty,
    ));

    match ty {
        ComponentType::Component(decls) => {
            for (i, decl) in decls.iter().enumerate() {
                visit_component_type_decl(ty, decl, i, out);
            }
        }

        ComponentType::Instance(decls) => {
            for (i, decl) in decls.iter().enumerate() {
                visit_instance_type_decl(ty, decl, i, out);
            }
        }

        // no sub-scoping for the below variants
        ComponentType::Defined(_) | ComponentType::Func(_) | ComponentType::Resource { .. } => {}
    }

    out.push(VisitEvent::exit_comp_type(
        ty.index_space_of().into(),
        idx,
        ty,
    ));
}
fn visit_component_type_decl<'ir>(
    parent: &'ir ComponentType<'ir>,
    decl: &'ir ComponentTypeDeclaration<'ir>,
    idx: usize,
    out: &mut Vec<VisitEvent<'ir>>,
) {
    out.push(VisitEvent::comp_type_decl(parent, idx, decl));

    match decl {
        ComponentTypeDeclaration::Type(ty) => visit_comp_type(idx, ty, out),
        ComponentTypeDeclaration::CoreType(ty) => visit_core_type(idx, ty, out),
        ComponentTypeDeclaration::Alias(_)
        | ComponentTypeDeclaration::Export { .. }
        | ComponentTypeDeclaration::Import(_) => {}
    }
}
fn visit_instance_type_decl<'ir>(
    parent: &'ir ComponentType<'ir>,
    decl: &'ir InstanceTypeDeclaration<'ir>,
    idx: usize,
    out: &mut Vec<VisitEvent<'ir>>,
) {
    out.push(VisitEvent::inst_type_decl(parent, idx, decl));

    match decl {
        InstanceTypeDeclaration::Type(ty) => visit_comp_type(idx, ty, out),
        InstanceTypeDeclaration::CoreType(ty) => visit_core_type(idx, ty, out),
        InstanceTypeDeclaration::Alias(_) | InstanceTypeDeclaration::Export { .. } => {}
    }
}
fn visit_core_type<'ir>(idx: usize, ty: &'ir CoreType<'ir>, out: &mut Vec<VisitEvent<'ir>>) {
    match ty {
        CoreType::Module(decls) => {
            out.push(VisitEvent::enter_core_type(
                ty.index_space_of().into(),
                idx,
                ty,
            ));
            for (i, decl) in decls.iter().enumerate() {
                visit_module_type_decl(ty, decl, i, out);
            }
            out.push(VisitEvent::exit_core_type(
                ty.index_space_of().into(),
                idx,
                ty,
            ));
        }

        // no sub-scoping for the below variant
        CoreType::Rec(group) => {
            out.push(VisitEvent::enter_rec_group(group.types().len(), ty));
            for (subvec_idx, item) in group.types().enumerate() {
                out.push(VisitEvent::core_subtype(idx, subvec_idx, item));
            }
            out.push(VisitEvent::exit_rec_group());
        }
    }
}

fn visit_module_type_decl<'ir>(
    parent: &'ir CoreType<'ir>,
    decl: &'ir ModuleTypeDeclaration<'ir>,
    idx: usize,
    out: &mut Vec<VisitEvent<'ir>>,
) {
    out.push(VisitEvent::mod_type_decl(parent, idx, decl));
}
