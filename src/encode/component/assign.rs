use std::collections::HashMap;
use wasmparser::{CanonicalFunction, ComponentType, CoreType};
use crate::Component;
use crate::encode::component::collect::{ComponentItem, ComponentPlan};

use crate::ir::component::idx_spaces::{ExternalItemKind, IdxSpaces};
use crate::ir::section::ComponentSection;

// Phase 2
// #[derive(Debug, Default)]
// pub(crate) struct Indices<'a> {
//     pub(crate) component: HashMap<*const Component<'a>, u32>,
//     pub(crate) canonical_func: HashMap<*const CanonicalFunction, u32>,
//     pub(crate) core_type: HashMap<*const CoreType<'a>, u32>,
//     pub(crate) comp_type: HashMap<*const ComponentType<'a>, u32>,
// }

pub(crate) fn assign_indices<'a>(plan: &mut ComponentPlan<'a>, indices: &mut IdxSpaces) {
    // TODO: I don't think I need this --> store in IdxSpaces!
    // let mut indices = Indices::default();

    // index trackers
    let mut next_comp = 0;
    let mut next_core_func = 0;
    let mut next_core_type = 0;
    let mut next_comp_type = 0;

    for item in &mut plan.items {
        match item {
            // ComponentItem::Component{ node, plan: subplan, idx, idx_spaces, ..} => {
            //     let ptr = *node as *const _;
            //     if !indices.component.contains_key(&ptr) {
            //         // I've not visited this node yet!
            //
            //         // Visit this component's internals
            //         let (sub_indices, sub_spaces) = assign_indices(subplan, idx_spaces);
            //
            //         // Assign the component an ID and remember what it was originally!
            //         // This allows us to fix ID mappings at encode time.
            //         indices.component.insert(ptr, next_comp);
            //         // spaces.comp.insert(*original_id, next_comp);
            //         next_comp += 1;
            //
            //         // Save the metadata in the ComponentItem itself!
            //         item.update_comp_metadata(sub_indices, sub_spaces);
            //     }
            // }
            // ComponentItem::CanonicalFunc { node, original_id } => {
            //     let ptr = *node as *const _;
            //     if !indices.canonical_func.contains_key(&ptr) {
            //         indices.canonical_func.insert(ptr, next_core_func);
            //
            //         // TODO: The type of function index is determined by the variant of the canonical function!
            //         spaces.core_func.insert(*original_id, next_core_func);
            //         next_core_func += 1;
            //     }
            // }
            ComponentItem::CoreType { node, idx } => {
                // let ptr = *node as *const _;
                // TODO -- I don't think it's possible to have duplicates here because of how I did the collect phase!
                // if !indices.core_type.contains_key(&ptr) {
                //     indices.core_type.insert(ptr, next_core_type);

                    let section = ComponentSection::CoreType;
                    let kind = ExternalItemKind::NA;
                    indices.assign_actual_id(&section, &kind, *idx);
                    // spaces.core_type.insert(*original_id, next_core_type);
                    // next_core_type += 1;
                // }
            }
            // ComponentItem::CompType { node, original_id } => {
            //     let ptr = *node as *const _;
            //     if !indices.comp_type.contains_key(&ptr) {
            //         indices.comp_type.insert(ptr, next_comp_type);
            //         spaces.comp_type.insert(*original_id, next_comp_type);
            //         next_core_type += 1;
            //     }
            // }
            _ => todo!("Not implemented yet: {item:?}")
        }
    }
}
