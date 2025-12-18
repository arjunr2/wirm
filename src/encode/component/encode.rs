// Phase 3

use wasm_encoder::NestedComponentSection;
use wasm_encoder::reencode::{Reencode, ReencodeComponent, RoundtripReencoder};
use wasmparser::{CanonicalFunction, CanonicalOption, ComponentType, CoreType};
use crate::Component;
use crate::encode::component::assign::Indices;
use crate::encode::component::collect::{ComponentItem, ComponentPlan};
use crate::encode::component::idx_spaces::{IdxSpace, IndexSpaces};
use crate::ir::wrappers::{convert_module_type_declaration, do_reencode};

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
pub(crate) fn encode_internal<'a>(comp: &Component, plan: &ComponentPlan<'a>, indices: &Indices, map: &IndexSpaces) -> wasm_encoder::Component {
    let mut component = wasm_encoder::Component::new();
    let mut reencode = RoundtripReencoder;

    for item in &plan.items {
        match item {
            ComponentItem::Component { node, plan: subplan, indices, idx_spaces: map, .. } => unsafe {
                let subcomp: &Component = &**node;
                component.section(&NestedComponentSection(
                    &encode_internal(subcomp, subplan, indices, map)
                ));
            },
            ComponentItem::CanonicalFunc { node, .. } => unsafe {
                let f: &CanonicalFunction = &**node;
                f.do_encode(&mut component, indices, map, &mut reencode);
            },
            ComponentItem::CoreType { node, .. } => unsafe {
                let t: &CoreType = &**node;
                t.do_encode(&mut component, indices, map, &mut reencode)
            },
            ComponentItem::CompType { node, .. } => unsafe {
                let t: &ComponentType = &**node;
                t.do_encode(&mut component, indices, map, &mut reencode)
            },
            i => todo!("Not implemented yet: {i:?}"),
        }
    }

    // Name section
    let mut name_sec = wasm_encoder::ComponentNameSection::new();

    if let Some(comp_name) = &comp.component_name {
        name_sec.component(comp_name);
    }

    name_sec.core_funcs(&comp.core_func_names);
    name_sec.core_tables(&comp.table_names);
    name_sec.core_memories(&comp.memory_names);
    name_sec.core_tags(&comp.tag_names);
    name_sec.core_globals(&comp.global_names);
    name_sec.core_types(&comp.core_type_names);
    name_sec.core_modules(&comp.module_names);
    name_sec.core_instances(&comp.core_instances_names);
    name_sec.funcs(&comp.func_names);
    name_sec.values(&comp.value_names);
    name_sec.types(&comp.type_names);
    name_sec.components(&comp.components_names);
    name_sec.instances(&comp.instance_names);

    // Add the name section back to the component
    component.section(&name_sec);

    component
}


trait Encode {
    fn do_encode<'a>(&self, component: &mut wasm_encoder::Component, indices: &Indices, spaces: &IndexSpaces, reencode: &mut RoundtripReencoder);
}

impl Encode for CanonicalFunction {
    fn do_encode<'a>(&self, component: &mut wasm_encoder::Component, indices: &Indices, spaces: &IndexSpaces, reencode: &mut RoundtripReencoder) {
        let mut canon_sec = wasm_encoder::CanonicalFunctionSection::new();

        // TODO: This is where I'm going to look up the indices that should be assigned at this point for any dependencies of this item
        let idx = indices.canonical_func[&(&*self as *const _)];
        // let idx_space = spaces.get_space(&self.idx_space());
        // out.push(idx as u8); // pretend the "encoding" is just the index
        // encode body etc.
        match self {
            CanonicalFunction::Lift {
                core_func_index: core_func_index_orig,
                type_index: type_idx_orig,
                options: options_orig,
            } => {
                // a lift would need to reference a CORE function
                let new_fid = spaces.core_func.get(core_func_index_orig).unwrap();
                let new_tid = spaces.comp_type.get(type_idx_orig).unwrap();
                canon_sec.lift(
                    *new_fid,
                    *new_tid,
                    options_orig.iter().map(|canon| {
                        do_reencode(
                            *canon,
                            RoundtripReencoder::canonical_option,
                            reencode,
                            "canonical option",
                        )
                    }),
                );
            }
            CanonicalFunction::Lower {
                func_index: fid_orig,
                options: options_orig
            } => {
                // TODO -- need to fix options!!!
                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    let fixed = match opt {
                        CanonicalOption::Realloc(opt_fid_orig) |
                        CanonicalOption::PostReturn(opt_fid_orig) |
                        CanonicalOption::Callback(opt_fid_orig) => {
                            let new_fid = spaces.core_func.get(opt_fid_orig).unwrap();
                            match opt {
                                CanonicalOption::Realloc(_) => CanonicalOption::Realloc(*new_fid),
                                CanonicalOption::PostReturn(_) => CanonicalOption::PostReturn(*new_fid),
                                CanonicalOption::Callback(_) => CanonicalOption::Callback(*new_fid),
                                _ => unreachable!()
                            }
                        }
                        CanonicalOption::CoreType(opt_tid_orig) => {
                            let new_tid = spaces.core_type.get(opt_tid_orig).unwrap();
                            CanonicalOption::CoreType(*new_tid)
                        }

                        // TODO -- handle remapping of map ids!
                        CanonicalOption::Memory(_mid) => opt.clone(),
                        CanonicalOption::UTF8 |
                        CanonicalOption::UTF16 |
                        CanonicalOption::CompactUTF16 |
                        CanonicalOption::Async |
                        CanonicalOption::Gc => opt.clone(),
                    };
                    fixed_options.push(fixed);
                }

                let new_fid = spaces.comp_func.get(fid_orig).unwrap();
                canon_sec.lower(
                    *new_fid,
                    fixed_options.iter().map(|canon| {
                        do_reencode(
                            *canon,
                            RoundtripReencoder::canonical_option,
                            reencode,
                            "canonical option",
                        )
                    }),
                );
            }
            CanonicalFunction::ResourceNew { resource: rsc_orig } => {
                let new_rsc = spaces.comp_type.get(rsc_orig).unwrap();
                canon_sec.resource_new(*new_rsc);
            }
            CanonicalFunction::ResourceDrop { resource: rsc_orig } => {
                let new_rsc = spaces.comp_type.get(rsc_orig).unwrap();
                canon_sec.resource_drop(*new_rsc);
            }
            CanonicalFunction::ResourceRep { resource: rsc_orig } => {
                let new_rsc = spaces.comp_type.get(rsc_orig).unwrap();
                canon_sec.resource_rep(*new_rsc);
            }
            CanonicalFunction::ResourceDropAsync { resource: rsc_orig } => {
                let new_rsc = spaces.comp_type.get(rsc_orig).unwrap();
                canon_sec.resource_drop_async(*new_rsc);
            }
            CanonicalFunction::ThreadAvailableParallelism => {
                canon_sec.thread_available_parallelism();
            }
            CanonicalFunction::BackpressureSet => {
                canon_sec.backpressure_set();
            }
            // CanonicalFunction::TaskReturn { result, options } => {
            //     // TODO: This needs to be fixed
            //     let options = options
            //         .iter()
            //         .cloned()
            //         .map(|v| v.into())
            //         .collect::<Vec<_>>();
            //     let result = result.map(|v| {
            //         let fixed_ty = self.lookup_component_val_type(
            //             v, component, reencode, indices
            //         );
            //         fixed_ty.into()
            //     });
            //     canon_sec.task_return(result, options);
            // }
            // CanonicalFunction::Yield { async_ } => {
            //     canon_sec.yield_(*async_);
            // }
            CanonicalFunction::WaitableSetNew => {
                canon_sec.waitable_set_new();
            }
            // CanonicalFunction::WaitableSetWait { async_, memory } => {
            //     canon_sec.waitable_set_wait(*async_, *memory);
            // }
            // CanonicalFunction::WaitableSetPoll { async_, memory } => {
            //     canon_sec.waitable_set_poll(*async_, *memory);
            // }
            CanonicalFunction::WaitableSetDrop => {
                canon_sec.waitable_set_drop();
            }
            CanonicalFunction::WaitableJoin => {
                canon_sec.waitable_join();
            }
            CanonicalFunction::SubtaskDrop => {
                canon_sec.subtask_drop();
            }
            // CanonicalFunction::StreamNew { ty } => {
            //     let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *ty as usize) {
            //         // has already been encoded
            //         *id
            //     } else {
            //         // we need to skip around and encode this type first!
            //         println!("here");
            //         let (_, idx) = indices.index_from_assumed_id(&section, &kind, *ty as usize);
            //         println!("    ==> using idx: {idx}");
            //         self.internal_encode_canon(idx, 1, component, reencode, indices);
            //         indices.lookup_actual_id_or_panic(&section, &kind, *ty as usize)
            //     };
            //     canon_sec.stream_new(ty_id as u32);
            // }
            // CanonicalFunction::StreamRead { ty, options } => {
            //     let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *ty as usize) {
            //         // has already been encoded
            //         *id
            //     } else {
            //         // we need to skip around and encode this type first!
            //         println!("here");
            //         let (_, idx) = indices.index_from_assumed_id(&section, &kind, *ty as usize);
            //         println!("    ==> using idx: {idx}");
            //         self.internal_encode_canon(idx, 1, component, reencode, indices);
            //         indices.lookup_actual_id_or_panic(&section, &kind, *ty as usize)
            //     };
            //     canon_sec.stream_read(
            //         ty_id as u32,
            //         options
            //             .into_iter()
            //             .map(|t| {
            //                 do_reencode(
            //                     *t,
            //                     RoundtripReencoder::canonical_option,
            //                     reencode,
            //                     "canonical option",
            //                 )
            //             })
            //             .collect::<Vec<wasm_encoder::CanonicalOption>>(),
            //     );
            // }
            // CanonicalFunction::StreamWrite { ty, options } => {
            //     let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *ty as usize) {
            //         // has already been encoded
            //         *id
            //     } else {
            //         // we need to skip around and encode this type first!
            //         println!("here");
            //         let (_, idx) = indices.index_from_assumed_id(&section, &kind, *ty as usize);
            //         println!("    ==> using idx: {idx}");
            //         self.internal_encode_canon(idx, 1, component, reencode, indices);
            //         indices.lookup_actual_id_or_panic(&section, &kind, *ty as usize)
            //     };
            //     canon_sec.stream_write(
            //         ty_id as u32,
            //         options
            //             .into_iter()
            //             .map(|t| {
            //                 do_reencode(
            //                     *t,
            //                     RoundtripReencoder::canonical_option,
            //                     reencode,
            //                     "canonical option",
            //                 )
            //             })
            //             .collect::<Vec<wasm_encoder::CanonicalOption>>(),
            //     );
            // }
            // CanonicalFunction::StreamCancelRead { ty, async_ } => {
            //     let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *ty as usize) {
            //         // has already been encoded
            //         *id
            //     } else {
            //         // we need to skip around and encode this type first!
            //         println!("here");
            //         let (_, idx) = indices.index_from_assumed_id(&section, &kind, *ty as usize);
            //         println!("    ==> using idx: {idx}");
            //         self.internal_encode_canon(idx, 1, component, reencode, indices);
            //         indices.lookup_actual_id_or_panic(&section, &kind, *ty as usize)
            //     };
            //     canon_sec.stream_cancel_read(ty_id as u32, *async_);
            // }
            // CanonicalFunction::StreamCancelWrite { ty, async_ } => {
            //     let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *ty as usize) {
            //         // has already been encoded
            //         *id
            //     } else {
            //         // we need to skip around and encode this type first!
            //         println!("here");
            //         let (_, idx) = indices.index_from_assumed_id(&section, &kind, *ty as usize);
            //         println!("    ==> using idx: {idx}");
            //         self.internal_encode_canon(idx, 1, component, reencode, indices);
            //         indices.lookup_actual_id_or_panic(&section, &kind, *ty as usize)
            //     };
            //     canon_sec.stream_cancel_write(ty_id as u32, *async_);
            // }
            // CanonicalFunction::FutureNew { ty } => {
            //     let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *ty as usize) {
            //         // has already been encoded
            //         *id
            //     } else {
            //         // we need to skip around and encode this type first!
            //         println!("here");
            //         let (_, idx) = indices.index_from_assumed_id(&section, &kind, *ty as usize);
            //         println!("    ==> using idx: {idx}");
            //         self.internal_encode_canon(idx, 1, component, reencode, indices);
            //         indices.lookup_actual_id_or_panic(&section, &kind, *ty as usize)
            //     };
            //     canon_sec.future_new(ty_id as u32);
            // }
            // CanonicalFunction::FutureRead { ty, options } => {
            //     let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *ty as usize) {
            //         // has already been encoded
            //         *id
            //     } else {
            //         // we need to skip around and encode this type first!
            //         println!("here");
            //         let (_, idx) = indices.index_from_assumed_id(&section, &kind, *ty as usize);
            //         println!("    ==> using idx: {idx}");
            //         self.internal_encode_canon(idx, 1, component, reencode, indices);
            //         indices.lookup_actual_id_or_panic(&section, &kind, *ty as usize)
            //     };
            //     canon_sec.future_read(
            //         ty_id as u32,
            //         options
            //             .into_iter()
            //             .map(|t| {
            //                 do_reencode(
            //                     *t,
            //                     RoundtripReencoder::canonical_option,
            //                     reencode,
            //                     "canonical option",
            //                 )
            //             })
            //             .collect::<Vec<wasm_encoder::CanonicalOption>>(),
            //     );
            // }
            // CanonicalFunction::FutureWrite { ty, options } => {
            //     let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *ty as usize) {
            //         // has already been encoded
            //         *id
            //     } else {
            //         // we need to skip around and encode this type first!
            //         println!("here");
            //         let (_, idx) = indices.index_from_assumed_id(&section, &kind, *ty as usize);
            //         println!("    ==> using idx: {idx}");
            //         self.internal_encode_canon(idx, 1, component, reencode, indices);
            //         indices.lookup_actual_id_or_panic(&section, &kind, *ty as usize)
            //     };
            //     canon_sec.future_write(
            //         ty_id as u32,
            //         options
            //             .into_iter()
            //             .map(|t| {
            //                 do_reencode(
            //                     *t,
            //                     RoundtripReencoder::canonical_option,
            //                     reencode,
            //                     "canonical option",
            //                 )
            //             })
            //             .collect::<Vec<wasm_encoder::CanonicalOption>>(),
            //     );
            // }
            // CanonicalFunction::FutureCancelRead { ty, async_ } => {
            //     let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *ty as usize) {
            //         // has already been encoded
            //         *id
            //     } else {
            //         // we need to skip around and encode this type first!
            //         println!("here");
            //         let (_, idx) = indices.index_from_assumed_id(&section, &kind, *ty as usize);
            //         println!("    ==> using idx: {idx}");
            //         self.internal_encode_canon(idx, 1, component, reencode, indices);
            //         indices.lookup_actual_id_or_panic(&section, &kind, *ty as usize)
            //     };
            //     canon_sec.future_cancel_read(ty_id as u32, *async_);
            // }
            // CanonicalFunction::FutureCancelWrite { ty, async_ } => {
            //     let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *ty as usize) {
            //         // has already been encoded
            //         *id
            //     } else {
            //         // we need to skip around and encode this type first!
            //         println!("here");
            //         let (_, idx) = indices.index_from_assumed_id(&section, &kind, *ty as usize);
            //         println!("    ==> using idx: {idx}");
            //         self.internal_encode_canon(idx, 1, component, reencode, indices);
            //         indices.lookup_actual_id_or_panic(&section, &kind, *ty as usize)
            //     };
            //     canon_sec.future_cancel_write(ty_id as u32, *async_);
            // }
            CanonicalFunction::ErrorContextNew { options } => {
                // TODO: This needs to be fixed
                canon_sec.error_context_new(
                    options
                        .into_iter()
                        .map(|t| {
                            do_reencode(
                                *t,
                                RoundtripReencoder::canonical_option,
                                reencode,
                                "canonical option",
                            )
                        })
                        .collect::<Vec<wasm_encoder::CanonicalOption>>(),
                );
            }
            CanonicalFunction::ErrorContextDebugMessage { options } => {
                // TODO: This needs to be fixed
                canon_sec.error_context_debug_message(
                    options
                        .into_iter()
                        .map(|t| {
                            do_reencode(
                                *t,
                                RoundtripReencoder::canonical_option,
                                reencode,
                                "canonical option",
                            )
                        })
                        .collect::<Vec<wasm_encoder::CanonicalOption>>(),
                );
            }
            CanonicalFunction::ErrorContextDrop => {
                canon_sec.error_context_drop();
            }
            // CanonicalFunction::ThreadSpawnRef { func_ty_index } => {
            //     let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *func_ty_index as usize) {
            //         // has already been encoded
            //         *id
            //     } else {
            //         // we need to skip around and encode this type first!
            //         println!("here");
            //         let (_, idx) = indices.index_from_assumed_id(&section, &kind, *func_ty_index as usize);
            //         println!("    ==> using idx: {idx}");
            //         self.internal_encode_canon(idx, 1, component, reencode, indices);
            //         indices.lookup_actual_id_or_panic(&section, &kind, *func_ty_index as usize)
            //     };
            //     canon_sec.thread_spawn_ref(ty_id as u32);
            // }
            // CanonicalFunction::ThreadSpawnIndirect {
            //     func_ty_index,
            //     table_index,
            // } => {
            //     // TODO: This needs to be fixed
            //     let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *func_ty_index as usize) {
            //         // has already been encoded
            //         *id
            //     } else {
            //         // we need to skip around and encode this type first!
            //         println!("here");
            //         let (_, idx) = indices.index_from_assumed_id(&section, &kind, *func_ty_index as usize);
            //         println!("    ==> using idx: {idx}");
            //         self.internal_encode_canon(idx, 1, component, reencode, indices);
            //         indices.lookup_actual_id_or_panic(&section, &kind, *func_ty_index as usize)
            //     };
            //     canon_sec.thread_spawn_indirect(ty_id as u32, *table_index);
            // }
            CanonicalFunction::TaskCancel => {
                canon_sec.task_cancel();
            }
            CanonicalFunction::ContextGet(i) => {
                canon_sec.context_get(*i);
            }
            CanonicalFunction::ContextSet(i) => {
                canon_sec.context_set(*i);
            }
            CanonicalFunction::SubtaskCancel { async_ } => {
                canon_sec.subtask_cancel(*async_);
            }
            // CanonicalFunction::StreamDropReadable { ty } => {
            //     let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *ty as usize) {
            //         // has already been encoded
            //         *id
            //     } else {
            //         // we need to skip around and encode this type first!
            //         println!("here");
            //         let (_, idx) = indices.index_from_assumed_id(&section, &kind, *ty as usize);
            //         println!("    ==> using idx: {idx}");
            //         self.internal_encode_canon(idx, 1, component, reencode, indices);
            //         indices.lookup_actual_id_or_panic(&section, &kind, *ty as usize)
            //     };
            //     canon_sec.stream_drop_readable(ty_id as u32);
            // }
            // CanonicalFunction::StreamDropWritable { ty } => {
            //     let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *ty as usize) {
            //         // has already been encoded
            //         *id
            //     } else {
            //         // we need to skip around and encode this type first!
            //         println!("here");
            //         let (_, idx) = indices.index_from_assumed_id(&section, &kind, *ty as usize);
            //         println!("    ==> using idx: {idx}");
            //         self.internal_encode_canon(idx, 1, component, reencode, indices);
            //         indices.lookup_actual_id_or_panic(&section, &kind, *ty as usize)
            //     };
            //     canon_sec.stream_drop_writable(ty_id as u32);
            // }
            // CanonicalFunction::FutureDropReadable { ty } => {
            //     let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *ty as usize) {
            //         // has already been encoded
            //         *id
            //     } else {
            //         // we need to skip around and encode this type first!
            //         println!("here");
            //         let (_, idx) = indices.index_from_assumed_id(&section, &kind, *ty as usize);
            //         println!("    ==> using idx: {idx}");
            //         self.internal_encode_canon(idx, 1, component, reencode, indices);
            //         indices.lookup_actual_id_or_panic(&section, &kind, *ty as usize)
            //     };
            //     canon_sec.future_drop_readable(ty_id as u32);
            // }
            // CanonicalFunction::FutureDropWritable { ty } => {
            //     let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *ty as usize) {
            //         // has already been encoded
            //         *id
            //     } else {
            //         // we need to skip around and encode this type first!
            //         println!("here");
            //         let (_, idx) = indices.index_from_assumed_id(&section, &kind, *ty as usize);
            //         println!("    ==> using idx: {idx}");
            //         self.internal_encode_canon(idx, 1, component, reencode, indices);
            //         indices.lookup_actual_id_or_panic(&section, &kind, *ty as usize)
            //     };
            //     canon_sec.future_drop_writable(ty_id as u32);
            // }
            _ => todo!("not yet implemented for {self:?}"),
        }
        component.section(&canon_sec);
    }
}

impl Encode for CoreType<'_> {
    fn do_encode<'a>(&self, component: &mut wasm_encoder::Component, indices: &Indices, spaces: &IndexSpaces, reencode: &mut RoundtripReencoder) {
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
                let enc = type_section.ty();
                convert_module_type_declaration(module, enc, reencode);
            }
        }
        component.section(&type_section);
    }
}

impl Encode for ComponentType<'_> {
    fn do_encode<'a>(&self, component: &mut wasm_encoder::Component, indices: &Indices, spaces: &IndexSpaces, reencode: &mut RoundtripReencoder) {
        let mut component_ty_section = wasm_encoder::ComponentTypeSection::new();

        // TODO: This is where I'm going to look up the indices that should be assigned at this point for any dependencies of this item
        let idx = indices.comp_type[&(&*self as *const _)];

        match &self {
            // ComponentType::Defined(comp_ty) => {
            //     let enc = component_ty_section.defined_type();
            //     match comp_ty {
            //         wasmparser::ComponentDefinedType::Primitive(p) => {
            //             enc.primitive(wasm_encoder::PrimitiveValType::from(*p))
            //         }
            //         wasmparser::ComponentDefinedType::Record(records) => {
            //             enc.record(
            //                 records.iter().map(|(n, ty)| {
            //                     let fixed_ty = self.lookup_component_val_type(
            //                         *ty, component, reencode, indices
            //                     );
            //                     (*n, reencode.component_val_type(fixed_ty))
            //                 }),
            //             );
            //         }
            //         wasmparser::ComponentDefinedType::Variant(variants) => {
            //             enc.variant(variants.iter().map(|variant| {
            //                 (
            //                     variant.name,
            //                     variant.ty.map(|ty| {
            //                         let fixed_ty = self.lookup_component_val_type(
            //                             ty, component, reencode, indices
            //                         );
            //                         reencode.component_val_type(fixed_ty)
            //                     }),
            //                     variant.refines,
            //                 )
            //             }))
            //         }
            //         wasmparser::ComponentDefinedType::List(l) => {
            //             let fixed_ty = self.lookup_component_val_type(
            //                 *l, component, reencode, indices
            //             );
            //             enc.list(reencode.component_val_type(fixed_ty))
            //         }
            //         wasmparser::ComponentDefinedType::Tuple(tup) => enc.tuple(
            //             tup.iter()
            //                 .map(|val_type| {
            //                     let fixed_ty = self.lookup_component_val_type(
            //                         *val_type, component, reencode, indices
            //                     );
            //                     reencode.component_val_type(fixed_ty)
            //                 }),
            //         ),
            //         wasmparser::ComponentDefinedType::Flags(flags) => {
            //             enc.flags(flags.clone().into_vec().into_iter())
            //         }
            //         wasmparser::ComponentDefinedType::Enum(en) => {
            //             enc.enum_type(en.clone().into_vec().into_iter())
            //         }
            //         wasmparser::ComponentDefinedType::Option(opt) => {
            //             let fixed_ty = self.lookup_component_val_type(
            //                 *opt, component, reencode, indices
            //             );
            //             enc.option(reencode.component_val_type(fixed_ty))
            //         }
            //         wasmparser::ComponentDefinedType::Result { ok, err } => enc.result(
            //             ok.map(|val_type| {
            //                 let fixed_ty = self.lookup_component_val_type(
            //                     val_type, component, reencode, indices
            //                 );
            //                 reencode.component_val_type(fixed_ty)
            //             }),
            //             err.map(|val_type| {
            //                 let fixed_ty = self.lookup_component_val_type(
            //                     val_type, component, reencode, indices
            //                 );
            //                 reencode.component_val_type(fixed_ty)
            //             }),
            //         ),
            //         wasmparser::ComponentDefinedType::Own(u) => {
            //             let id = if let Some(id) = indices.lookup_actual_id(&section, &kind, *u as usize) {
            //                 // has already been encoded
            //                 *id
            //             } else {
            //                 // we need to skip around and encode this type first!
            //                 self.internal_encode_component_type(*u as usize, 1, component, reencode, indices);
            //                 indices.lookup_actual_id_or_panic(&section, &kind, *u as usize)
            //             };
            //             enc.own(id as u32)
            //         },
            //         wasmparser::ComponentDefinedType::Borrow(u) => {
            //             let id = if let Some(id) = indices.lookup_actual_id(&section, &kind, *u as usize) {
            //                 // has already been encoded
            //                 *id
            //             } else {
            //                 // we need to skip around and encode this type first!
            //                 self.internal_encode_component_type(*u as usize, 1, component, reencode, indices);
            //                 indices.lookup_actual_id_or_panic(&section, &kind, *u as usize)
            //             };
            //             enc.borrow(id as u32)
            //         },
            //         wasmparser::ComponentDefinedType::Future(opt) => match opt {
            //             Some(u) => {
            //                 let fixed_ty = self.lookup_component_val_type(
            //                     *u, component, reencode, indices
            //                 );
            //                 enc.future(Some(reencode.component_val_type(fixed_ty)))
            //             },
            //             None => enc.future(None),
            //         },
            //         wasmparser::ComponentDefinedType::Stream(opt) => match opt {
            //             Some(u) => {
            //                 let fixed_ty = self.lookup_component_val_type(
            //                     *u, component, reencode, indices
            //                 );
            //                 enc.stream(Some(reencode.component_val_type(fixed_ty)))
            //             },
            //             None => enc.stream(None),
            //         },
            //         wasmparser::ComponentDefinedType::FixedSizeList(ty, i) => {
            //             let fixed_ty = self.lookup_component_val_type(
            //                 *ty, component, reencode, indices
            //             );
            //             enc.fixed_size_list(reencode.component_val_type(fixed_ty), *i)
            //         }
            //     }
            // }
            // ComponentType::Func(func_ty) => {
            //     let mut enc = component_ty_section.function();
            //     enc.params(func_ty.params.iter().map(
            //         |p: &(&str, wasmparser::ComponentValType)| {
            //             let fixed_ty = self.lookup_component_val_type(
            //                 p.1, component, reencode, indices
            //             );
            //             (p.0, reencode.component_val_type(fixed_ty))
            //         },
            //     ));
            //     enc.result(func_ty.result.map(|v| {
            //         let fixed_ty = self.lookup_component_val_type(
            //             v, component, reencode, indices
            //         );
            //         reencode.component_val_type(fixed_ty)
            //     }));
            // }
            // ComponentType::Component(comp) => {
            //     // TODO: Check if we need to lookup IDs here
            //     let mut new_comp = wasm_encoder::ComponentType::new();
            //     for c in comp.iter() {
            //         match c {
            //             ComponentTypeDeclaration::CoreType(core) => match core {
            //                 CoreType::Rec(recgroup) => {
            //                     let types = recgroup
            //                         .types()
            //                         .map(|ty| {
            //                             reencode.sub_type(ty.to_owned()).unwrap_or_else(|_| {
            //                                 panic!("Could not encode type as subtype: {:?}", ty)
            //                             })
            //                         })
            //                         .collect::<Vec<_>>();
            //
            //                     if recgroup.is_explicit_rec_group() {
            //                         new_comp.core_type().core().rec(types);
            //                     } else {
            //                         // it's implicit!
            //                         for subty in types {
            //                             new_comp.core_type().core().subtype(&subty);
            //                         }
            //                     }
            //                 }
            //                 CoreType::Module(module) => {
            //                     // TODO: This needs to be fixed
            //                     let enc = new_comp.core_type();
            //                     convert_module_type_declaration(module, enc, reencode);
            //                 }
            //             },
            //             ComponentTypeDeclaration::Type(typ) => {
            //                 // TODO: This needs to be fixed
            //                 let enc = new_comp.ty();
            //                 self.convert_component_type(&(*typ).clone(), enc, component, reencode, indices);
            //             }
            //             ComponentTypeDeclaration::Alias(a) => {
            //                 // TODO: This needs to be fixed
            //                 new_comp.alias(self.process_alias(a, component, reencode, indices));
            //             }
            //             ComponentTypeDeclaration::Export { name, ty } => {
            //                 let fixed_ty = self.fix_component_type_ref(*ty, component, reencode, indices);
            //
            //                 let ty = do_reencode(
            //                     fixed_ty,
            //                     RoundtripReencoder::component_type_ref,
            //                     reencode,
            //                     "component type",
            //                 );
            //                 new_comp.export(name.0, ty);
            //             }
            //             ComponentTypeDeclaration::Import(imp) => {
            //                 let fixed_ty = self.fix_component_type_ref(imp.ty, component, reencode, indices);
            //
            //                 let ty = do_reencode(
            //                     fixed_ty,
            //                     RoundtripReencoder::component_type_ref,
            //                     reencode,
            //                     "component type",
            //                 );
            //                 new_comp.import(imp.name.0, ty);
            //             }
            //         }
            //     }
            //     component_ty_section.component(&new_comp);
            // }
            // ComponentType::Instance(inst) => {
            //     // TODO: This needs to be fixed
            //     component_ty_section.instance(&self.convert_instance_type(inst, component, reencode, indices));
            // }
            ComponentType::Resource { rep, dtor } => {
                // TODO: This needs to be fixed (the dtor likely points to a function)
                component_ty_section.resource(reencode.val_type(*rep).unwrap(), *dtor);
            }
            i => todo!("Not implemented yet: {self:?}"),
        }

        component.section(&component_ty_section);
    }
}

impl Encode for Component<'_> {
    fn do_encode<'a>(&self, component: &mut wasm_encoder::Component, indices: &Indices, spaces: &IndexSpaces, reencode: &mut RoundtripReencoder) {
        println!("\n\n==========================\n==== ENCODE COMPONENT ====\n==========================");
        let mut component = wasm_encoder::Component::new();
        let mut reencode = RoundtripReencoder;
        todo!()
    }
}
