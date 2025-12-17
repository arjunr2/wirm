// Phase 3

use wasm_encoder::NestedComponentSection;
use wasm_encoder::reencode::{Reencode, RoundtripReencoder};
use wasmparser::{CanonicalFunction, CoreType};
use crate::Component;
use crate::encode::component::assign::{IndexMap, Indices};
use crate::encode::component::collect::{ComponentItem, ComponentPlan};

/// Encodes all items in the plan into the output buffer.
///
/// This method contains `unsafe` blocks to dereference raw pointers stored in `ComponentItem`s.
/// The `unsafe` is sound because (see more details on safety in [`ComponentItem`]):
/// - All IR nodes live at least as long as the `EncodePlan<'a>` (`'a` lifetime ensures validity).
/// - The IR is immutable and never deallocated during encoding.
/// - Collection and index assignment phases guarantee that all references exist and are topologically ordered.
/// - Unsafe blocks are minimal, scoped only to dereference pointers; all other logic is fully safe.
///
/// # Example
///
/// ```rust
/// let bytes = encode(&plan, &indices);
/// ```
///
/// Here, `plan` is a linear `EncodePlan<'a>` of IR nodes, and `indices` maps nodes to assigned IDs.
pub(crate) fn encode_internal<'a>(plan: &ComponentPlan<'a>, indices: &Indices, map: &IndexMap) -> wasm_encoder::Component {
    let mut component = wasm_encoder::Component::new();
    let mut reencode = RoundtripReencoder;

    for item in &plan.items {
        match item {
            ComponentItem::Component { plan: subplan, indices, map, .. } => unsafe {
                component.section(&NestedComponentSection(
                    &encode_internal(subplan, indices, map)
                ));
            },
            ComponentItem::CanonicalFunc { node, .. } => unsafe {
                let f: &CanonicalFunction = &**node;
                f.do_encode(&mut component, indices, &mut reencode);
            },
            ComponentItem::CoreType { node, .. } => unsafe {
                let t: &CoreType = &**node;
                t.do_encode(&mut component, indices, &mut reencode)
            },
            i => todo!("Not implemented yet: {i:?}"),
        }
    }

    component
}


trait Encode {
    fn do_encode<'a>(&self, component: &mut wasm_encoder::Component, indices: &Indices, reencode: &mut RoundtripReencoder);
}

impl Encode for CanonicalFunction {
    fn do_encode<'a>(&self, component: &mut wasm_encoder::Component, indices: &Indices, reencode: &mut RoundtripReencoder) {
        // TODO: This is where I'm going to look up the indices that should be assigned at this point for any dependencies of this item
        let idx = indices.canonical_func[&(&*self as *const _)];
        // out.push(idx as u8); // pretend the "encoding" is just the index
        // encode body etc.
        todo!()
    }
}

impl Encode for CoreType<'_> {
    fn do_encode<'a>(&self, component: &mut wasm_encoder::Component, indices: &Indices, reencode: &mut RoundtripReencoder) {
        let mut type_section = wasm_encoder::CoreTypeSection::new();

        // TODO: This is where I'm going to look up the indices that should be assigned at this point for any dependencies of this item
        let idx = indices.core_type[&(&*self as *const _)];
        // out.push(idx as u8); // pretend the "encoding" is just the index
        // encode body etc.
        match &self {
            CoreType::Rec(recgroup) => {
                let types = recgroup
                    .types()
                    .map(|ty| {
                        reencode.sub_type(ty.to_owned()).unwrap_or_else(|_| {
                            panic!("Could not encode type as subtype: {:?}", ty)
                        })
                    })
                    .collect::<Vec<_>>();

                if recgroup.is_explicit_rec_group() {
                    type_section.ty().core().rec(types);
                } else {
                    // it's implicit!
                    for subty in types {
                        type_section.ty().core().subtype(&subty);
                    }
                }
            }
            CoreType::Module(module) => {
                // TODO: This *might* need to be fixed, but I'm unsure
                // let enc = type_section.ty();
                // convert_module_type_declaration(module, enc, reencode);
                todo!()
            }
        }
        component.section(&type_section);
    }
}

impl Encode for Component<'_> {
    fn do_encode<'a>(&self, component: &mut wasm_encoder::Component, indices: &Indices, reencode: &mut RoundtripReencoder) {
        println!("\n\n==========================\n==== ENCODE COMPONENT ====\n==========================");
        let mut component = wasm_encoder::Component::new();
        let mut reencode = RoundtripReencoder;
        todo!()
    }
}
