use std::collections::HashMap;
use wasmparser::CanonicalFunction;
use crate::encode::component::collect::{EncodeItem, EncodePlan};

// Phase 2
#[derive(Default)]
pub(crate) struct Indices {
    canonical_func: HashMap<*const CanonicalFunction, u32>,
    // canonical_func: HashMap<*const CanonicalFunction<'a>, u32>,
    // module: HashMap<*const Module<'a>, u32>,
    // component: HashMap<*const Component<'a>, u32>,
}

fn assign_indices(plan: &EncodePlan) -> Indices {
    let mut indices = Indices { canonical_func: HashMap::new() };
    // for (i, func) in plan.funcs.iter().enumerate() {
    //     indices.canonical_func.insert(*func as *const _, i as u32);
    // }
    // indices
    let mut next_func = 0;
    let mut next_module = 0;

    for item in &plan.items {
        match item {
            EncodeItem::CanonicalFunc(f) => {
                let ptr = *f as *const _;
                if !indices.canonical_func.contains_key(&ptr) {
                    indices.canonical_func.insert(ptr, next_func);
                    next_func += 1;
                }
            }
            // EncodeItem::Module(m) => {
            //     let ptr = *m as *const _;
            //     if !indices.module.contains_key(&ptr) {
            //         indices.module.insert(ptr, next_module);
            //         next_module += 1;
            //     }
            // }
            _ => {}
        }
    }
    
    indices
}
