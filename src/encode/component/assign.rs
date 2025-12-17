use std::collections::HashMap;
use wasmparser::{CanonicalFunction, CoreType};
use crate::Component;
use crate::encode::component::collect::{ComponentItem, ComponentPlan};

// Phase 2
#[derive(Debug, Default)]
pub(crate) struct Indices<'a> {
    pub(crate) component: HashMap<*const Component<'a>, u32>,
    pub(crate) canonical_func: HashMap<*const CanonicalFunction, u32>,
    pub(crate) core_type: HashMap<*const CoreType<'a>, u32>,
}

#[derive(Debug, Default)]
pub(crate) struct IndexMap {
    canon_funcs: HashMap<u32, u32>, // original_id -> assigned_id
    modules: HashMap<u32, u32>,
    components: HashMap<u32, u32>,
    types: HashMap<u32, u32>,
    // etc
}

pub(crate) fn assign_indices<'a>(plan: &mut ComponentPlan<'a>) -> (Indices<'a>, IndexMap) {
    let mut indices = Indices::default();
    let mut map = IndexMap::default();

    // index trackers
    let mut next_comp = 0;
    let mut next_canon_func = 0;
    let mut next_core_type = 0;

    for item in &mut plan.items {
        match item {
            ComponentItem::Component{ node, plan: subplan, original_id, ..} => {
                let ptr = *node as *const _;
                if !indices.component.contains_key(&ptr) {
                    // I've not visited this node yet!

                    // Visit this component's internals
                    let (sub_indices, sub_map) = assign_indices(subplan);

                    // Assign the component an ID and remember what it was originally!
                    // This allows us to fix ID mappings at encode time.
                    indices.component.insert(ptr, next_comp);
                    map.components.insert(*original_id, next_comp);
                    next_comp += 1;

                    // Save the metadata in the ComponentItem itself!
                    item.update_comp_metadata(sub_indices, sub_map);
                }
            }
            ComponentItem::CanonicalFunc { node, original_id } => {
                let ptr = *node as *const _;
                if !indices.canonical_func.contains_key(&ptr) {
                    indices.canonical_func.insert(ptr, next_canon_func);
                    next_canon_func += 1;
                }
            }
            ComponentItem::CoreType { node, original_id } => {
                let ptr = *node as *const _;
                if !indices.core_type.contains_key(&ptr) {
                    indices.core_type.insert(ptr, next_core_type);
                    next_core_type += 1;
                }
            }
            _ => todo!()
        }
    }

    (indices, map)
}
