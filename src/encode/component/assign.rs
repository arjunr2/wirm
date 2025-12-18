use crate::encode::component::collect::{ComponentItem, ComponentPlan};

use crate::ir::component::idx_spaces::{ExternalItemKind, IdxSpaces};
use crate::ir::section::ComponentSection;

// Phase 2

pub(crate) fn assign_indices<'a>(plan: &mut ComponentPlan<'a>, indices: &mut IdxSpaces) {
    for item in &mut plan.items {
        match item {
            ComponentItem::Component{ node, plan: subplan, idx, ..} => {
                // indices.reset_ids();
                // let ptr = *node as *const _;
                // if !indices.component.contains_key(&ptr) {
                //     // I've not visited this node yet!
                // 
                //     // Visit this component's internals
                //     let (sub_indices, sub_spaces) = assign_indices(subplan, idx_spaces);
                // 
                //     // Assign the component an ID and remember what it was originally!
                //     // This allows us to fix ID mappings at encode time.
                //     indices.component.insert(ptr, next_comp);
                //     // spaces.comp.insert(*original_id, next_comp);
                //     next_comp += 1;
                // 
                //     // Save the metadata in the ComponentItem itself!
                //     item.update_comp_metadata(sub_indices, sub_spaces);
                // }
                todo!()
            }
            ComponentItem::Module { idx, .. } => {
                indices.assign_actual_id(&ComponentSection::Module, &ExternalItemKind::NA, *idx);
            }
            ComponentItem::CompType { idx, .. } => {
                indices.assign_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *idx);
            }
            ComponentItem::CompInst { idx, .. } => {
                todo!()
            }
            ComponentItem::CanonicalFunc { node, idx } => {
                let ptr = *node as *const _;
                indices.assign_actual_id(&ComponentSection::Canon, &ExternalItemKind::from(ptr), *idx);
            }
            ComponentItem::Alias { idx, .. } => {
                todo!()
            }
            ComponentItem::Import { idx, .. } => {
                todo!()
            }
            ComponentItem::Export { idx, .. } => {
                todo!()
            }
            ComponentItem::CoreType { idx, .. } => {
                indices.assign_actual_id(&ComponentSection::CoreType, &ExternalItemKind::NA, *idx);
            }
            ComponentItem::Inst { idx, .. } => {
                todo!()
            }
            ComponentItem::CustomSection { idx, .. } => {
                todo!()
            }
        }
    }
}
