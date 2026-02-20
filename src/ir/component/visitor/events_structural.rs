use crate::Component;
use crate::ir::component::idx_spaces::IndexSpaceOf;
use crate::ir::component::section::ComponentSection;
use crate::ir::component::visitor::driver::VisitEvent;
use crate::ir::component::visitor::{ItemKind, VisitCtx};

pub(crate) fn get_structural_evts<'ir>(
    component: &'ir Component<'ir>,
    comp_idx: Option<usize>,
    ctx: &mut VisitCtx<'ir>,
    out: &mut Vec<VisitEvent<'ir>>,
) {
    ctx.inner.push_comp_section_tracker();
    if let Some(idx) = comp_idx {
        out.push(VisitEvent::enter_comp(
            component.index_space_of().into(),
            idx,
            component
        ));
    } else {
        out.push(VisitEvent::enter_root_comp(
            component.index_space_of().into(),
            0,
            component
        ));
    }

    for (num, section) in component.sections.iter() {
        let start_idx = ctx.inner.visit_section(section, *num as usize);

        match section {
            ComponentSection::Component => {
                debug_assert!(start_idx + *num as usize <= component.components.len());
                for i in 0..*num {
                    let idx = start_idx + i as usize;
                    let sub = &component.components[idx];
                    get_structural_evts(sub, Some(idx), ctx, out);
                }
            }

            ComponentSection::Module => {
                debug_assert!(start_idx + *num as usize <= component.modules.vec.len());
                push_events_for(
                    &component.modules.vec[start_idx..start_idx + *num as usize],
                    start_idx,
                    VisitEvent::module,
                    out
                );
            }

            ComponentSection::ComponentType => {
                debug_assert!(start_idx + *num as usize <= component.component_types.items.len());
                push_events_for_boxed(
                    &component.component_types.items[start_idx..start_idx + *num as usize],
                    start_idx,
                    VisitEvent::comp_type,
                    out
                );
            }

            ComponentSection::ComponentInstance => {
                debug_assert!(start_idx + *num as usize <= component.component_instance.len());
                push_events_for(
                    &component.component_instance[start_idx..start_idx + *num as usize],
                    start_idx,
                    VisitEvent::comp_inst,
                    out
                );
            }

            ComponentSection::Canon => {
                debug_assert!(start_idx + *num as usize <= component.canons.items.len());
                push_events_for(
                    &component.canons.items[start_idx..start_idx + *num as usize],
                    start_idx,
                    VisitEvent::canon,
                    out
                );
            }

            ComponentSection::Alias => {
                debug_assert!(start_idx + *num as usize <= component.alias.items.len());
                push_events_for(
                    &component.alias.items[start_idx..start_idx + *num as usize],
                    start_idx,
                    VisitEvent::alias,
                    out
                );
            }

            ComponentSection::ComponentImport => {
                debug_assert!(start_idx + *num as usize <= component.imports.len());
                push_events_for(
                    &component.imports[start_idx..start_idx + *num as usize],
                    start_idx,
                    VisitEvent::import,
                    out
                );
            }

            ComponentSection::ComponentExport => {
                debug_assert!(start_idx + *num as usize <= component.exports.len());
                push_events_for(
                    &component.exports[start_idx..start_idx + *num as usize],
                    start_idx,
                    VisitEvent::export,
                    out
                );
            }

            ComponentSection::CoreType => {
                debug_assert!(start_idx + *num as usize <= component.core_types.len());
                push_events_for_boxed(
                    &component.core_types[start_idx..start_idx + *num as usize],
                    start_idx,
                    VisitEvent::core_type,
                    out
                );
            }

            ComponentSection::CoreInstance => {
                debug_assert!(start_idx + *num as usize <= component.instances.len());
                push_events_for(
                    &component.instances[start_idx..start_idx + *num as usize],
                    start_idx,
                    VisitEvent::core_inst,
                    out
                );
            }

            ComponentSection::CustomSection => {
                debug_assert!(start_idx + *num as usize <= component.custom_sections.custom_sections.len());
                push_events_for(
                    &component.custom_sections.custom_sections[start_idx..start_idx + *num as usize],
                    start_idx,
                    VisitEvent::custom_sect,
                    out
                );
            }

            ComponentSection::ComponentStartSection => {
                debug_assert!(start_idx + *num as usize <= component.start_section.len());
                push_events_for(
                    &component.start_section[start_idx..start_idx + *num as usize],
                    start_idx,
                    VisitEvent::start_func,
                    out
                );
            }
        }
    }

    if let Some(idx) = comp_idx {
        ctx.inner.pop_comp_section_tracker();
        out.push(VisitEvent::exit_comp(
            component.index_space_of().into(),
            idx,
            component
        ));
    } else {
        out.push(VisitEvent::exit_root_comp(
            component.index_space_of().into(),
            0,
            component
        ));
    }
}

fn push_events_for<'ir, T: 'ir + IndexSpaceOf>(
    slice: &'ir [T],
    start: usize,
    new_evt: fn(ItemKind, usize, &'ir T) -> VisitEvent<'ir>,
    out: &mut Vec<VisitEvent<'ir>>
) {
    for (i, item) in slice.iter().enumerate() {
        out.push(
            new_evt(item.index_space_of().into(), start + i, &item)
        );
    }
}

fn push_events_for_boxed<'ir, T: 'ir + IndexSpaceOf>(
    slice: &'ir [Box<T>],
    start: usize,
    new_evt: fn(ItemKind, usize, &'ir T) -> VisitEvent<'ir>,
    out: &mut Vec<VisitEvent<'ir>>
) {
    for (i, item) in slice.iter().enumerate() {
        out.push(
            new_evt(item.index_space_of().into(), start + i, &item)
        );
    }
}
