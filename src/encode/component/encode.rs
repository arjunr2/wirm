use crate::encode::component::collect::{ComponentItem, ComponentPlan};
use crate::ir::component::idx_spaces::{IdxSpaces, ReferencedIndices, Refs};
use crate::ir::types::CustomSection;
use crate::ir::wrappers::{convert_module_type_declaration, convert_recgroup, do_reencode};
use crate::{Component, Module};
use wasm_encoder::reencode::{Reencode, ReencodeComponent, RoundtripReencoder};
use wasm_encoder::{
    Alias, ComponentAliasSection, ComponentFuncTypeEncoder, ComponentTypeEncoder, CoreTypeEncoder,
    InstanceType, ModuleArg, ModuleSection, NestedComponentSection,
};
use wasmparser::{
    CanonicalFunction, CanonicalOption, ComponentAlias, ComponentExport, ComponentImport,
    ComponentInstance, ComponentInstantiationArg, ComponentStartFunction, ComponentType,
    ComponentTypeDeclaration, ComponentTypeRef, ComponentValType, CoreType, Export, Instance,
    InstanceTypeDeclaration, InstantiationArg, SubType, TagType, TypeRef,
};

/// # PHASE 3 #
/// Encodes all items in the plan into the output buffer.
///
/// This method contains `unsafe` blocks to dereference raw pointers stored in `ComponentItem`s.
/// The `unsafe` is sound because (see more details on safety in [`ComponentItem`]):
/// - All IR nodes live at least as long as the `EncodePlan<'a>` (`'a` lifetime ensures validity).
/// - The IR is immutable and never deallocated during encoding.
/// - Collection and index assignment phases guarantee that all references exist and are topologically ordered.
/// - Unsafe blocks are minimal, scoped only to dereference pointers; all other logic is fully safe.
pub(crate) fn encode_internal<'a>(
    comp: &Component,
    plan: &ComponentPlan<'a>,
    indices: &IdxSpaces,
) -> wasm_encoder::Component {
    let mut component = wasm_encoder::Component::new();
    let mut reencode = RoundtripReencoder;

    for item in &plan.items {
        match item {
            ComponentItem::Component {
                node,
                plan: subplan,
                indices: subindices,
                ..
            } => unsafe {
                let subcomp: &Component = &**node;
                component.section(&NestedComponentSection(&encode_internal(
                    subcomp, subplan, subindices,
                )));
            },
            ComponentItem::Module { node, .. } => unsafe {
                let t: &Module = &**node;
                t.do_encode(&mut component, indices, &mut reencode);
            },
            ComponentItem::CompType { node, .. } => unsafe {
                let t: &ComponentType = &**node;
                t.do_encode(&mut component, indices, &mut reencode);
            },
            ComponentItem::CompInst { node, .. } => unsafe {
                let i: &ComponentInstance = &**node;
                i.do_encode(&mut component, indices, &mut reencode);
            },
            ComponentItem::CanonicalFunc { node, .. } => unsafe {
                let f: &CanonicalFunction = &**node;
                f.do_encode(&mut component, indices, &mut reencode);
            },
            ComponentItem::Alias { node, .. } => unsafe {
                let a: &ComponentAlias = &**node;
                a.do_encode(&mut component, indices, &mut reencode);
            },
            ComponentItem::Import { node, .. } => unsafe {
                let i: &ComponentImport = &**node;
                i.do_encode(&mut component, indices, &mut reencode);
            },
            ComponentItem::Export { node, .. } => unsafe {
                let e: &ComponentExport = &**node;
                e.do_encode(&mut component, indices, &mut reencode);
            },
            ComponentItem::CoreType { node, .. } => unsafe {
                let t: &CoreType = &**node;
                t.do_encode(&mut component, indices, &mut reencode);
            },
            ComponentItem::Inst { node, .. } => unsafe {
                let i: &Instance = &**node;
                i.do_encode(&mut component, indices, &mut reencode);
            },
            ComponentItem::Start { node, .. } => unsafe {
                let c: &ComponentStartFunction = &**node;
                c.do_encode(&mut component, indices, &mut reencode);
            },
            ComponentItem::CustomSection { node, .. } => unsafe {
                let c: &CustomSection = &**node;
                c.do_encode(&mut component, indices, &mut reencode);
            },
        }
    }

    // Name section
    let mut name_sec = wasm_encoder::ComponentNameSection::new();

    if let Some(comp_name) = &comp.component_name {
        name_sec.component(comp_name);
    }

    // TODO -- does the order here matter for names in the map?
    //         might need to fix indices here!
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
    fn do_encode<'a>(
        &self,
        component: &mut wasm_encoder::Component,
        indices: &IdxSpaces,
        reencode: &mut RoundtripReencoder,
    );
}

pub(crate) trait FixIndices {
    fn fix<'a>(
        &self,
        component: &mut wasm_encoder::Component,
        indices: &IdxSpaces,
        reencode: &mut RoundtripReencoder,
    ) -> Self;
}

impl Encode for Module<'_> {
    fn do_encode<'a>(
        &self,
        component: &mut wasm_encoder::Component,
        _: &IdxSpaces,
        _: &mut RoundtripReencoder,
    ) {
        component.section(&ModuleSection(&self.encode_internal(false).0));
    }
}

impl Encode for ComponentType<'_> {
    fn do_encode<'a>(
        &self,
        component: &mut wasm_encoder::Component,
        indices: &IdxSpaces,
        reencode: &mut RoundtripReencoder,
    ) {
        let mut component_ty_section = wasm_encoder::ComponentTypeSection::new();

        match &self {
            ComponentType::Defined(comp_ty) => {
                let enc = component_ty_section.defined_type();
                match comp_ty {
                    wasmparser::ComponentDefinedType::Primitive(p) => {
                        enc.primitive(wasm_encoder::PrimitiveValType::from(*p))
                    }
                    wasmparser::ComponentDefinedType::Record(records) => {
                        enc.record(records.iter().map(|(n, ty)| {
                            let fixed_ty = ty.fix(component, indices, reencode);
                            (*n, reencode.component_val_type(fixed_ty))
                        }));
                    }
                    wasmparser::ComponentDefinedType::Variant(variants) => {
                        enc.variant(variants.iter().map(|variant| {
                            (
                                variant.name,
                                variant.ty.map(|ty| {
                                    let fixed_ty = ty.fix(component, indices, reencode);
                                    reencode.component_val_type(fixed_ty)
                                }),
                                variant.refines,
                            )
                        }))
                    }
                    wasmparser::ComponentDefinedType::List(l) => {
                        let fixed_ty = l.fix(component, indices, reencode);
                        enc.list(reencode.component_val_type(fixed_ty))
                    }
                    wasmparser::ComponentDefinedType::Tuple(tup) => {
                        enc.tuple(tup.iter().map(|val_type| {
                            let fixed_ty = val_type.fix(component, indices, reencode);
                            reencode.component_val_type(fixed_ty)
                        }))
                    }
                    wasmparser::ComponentDefinedType::Flags(flags) => {
                        enc.flags(flags.clone().into_vec().into_iter())
                    }
                    wasmparser::ComponentDefinedType::Enum(en) => {
                        enc.enum_type(en.clone().into_vec().into_iter())
                    }
                    wasmparser::ComponentDefinedType::Option(opt) => {
                        let fixed_ty = opt.fix(component, indices, reencode);
                        enc.option(reencode.component_val_type(fixed_ty))
                    }
                    wasmparser::ComponentDefinedType::Result { ok, err } => enc.result(
                        ok.map(|val_type| {
                            let fixed_ty = val_type.fix(component, indices, reencode);
                            reencode.component_val_type(fixed_ty)
                        }),
                        err.map(|val_type| {
                            let fixed_ty = val_type.fix(component, indices, reencode);
                            reencode.component_val_type(fixed_ty)
                        }),
                    ),
                    wasmparser::ComponentDefinedType::Own(_) => {
                        let Some(Refs { ty: Some(ty), .. }) = comp_ty.referenced_indices() else {
                            panic!()
                        };
                        let id = indices.lookup_actual_id_or_panic(&ty);
                        enc.own(id as u32)
                    }
                    wasmparser::ComponentDefinedType::Borrow(_) => {
                        let Some(Refs { ty: Some(ty), .. }) = comp_ty.referenced_indices() else {
                            panic!()
                        };
                        let id = indices.lookup_actual_id_or_panic(&ty);
                        enc.borrow(id as u32)
                    }
                    wasmparser::ComponentDefinedType::Future(opt) => match opt {
                        Some(u) => {
                            let fixed_ty = u.fix(component, indices, reencode);
                            enc.future(Some(reencode.component_val_type(fixed_ty)))
                        }
                        None => enc.future(None),
                    },
                    wasmparser::ComponentDefinedType::Stream(opt) => match opt {
                        Some(u) => {
                            let fixed_ty = u.fix(component, indices, reencode);
                            enc.stream(Some(reencode.component_val_type(fixed_ty)))
                        }
                        None => enc.stream(None),
                    },
                    wasmparser::ComponentDefinedType::FixedSizeList(ty, i) => {
                        let fixed_ty = ty.fix(component, indices, reencode);
                        enc.fixed_size_list(reencode.component_val_type(fixed_ty), *i)
                    }
                }
            }
            ComponentType::Func(func_ty) => {
                let mut enc = component_ty_section.function();
                enc.params(func_ty.params.iter().map(|p: &(&str, ComponentValType)| {
                    let fixed_ty = p.1.fix(component, indices, reencode);
                    (p.0, reencode.component_val_type(fixed_ty))
                }));
                enc.result(func_ty.result.map(|v| {
                    let fixed_ty = v.fix(component, indices, reencode);
                    reencode.component_val_type(fixed_ty)
                }));
            }
            ComponentType::Component(comp) => {
                let mut new_comp = wasm_encoder::ComponentType::new();
                for c in comp.iter() {
                    match c {
                        ComponentTypeDeclaration::CoreType(core) => match core {
                            CoreType::Rec(recgroup) => {
                                // this doesn't have any ID refs.
                                let types = convert_recgroup(recgroup, reencode);

                                if recgroup.is_explicit_rec_group() {
                                    new_comp.core_type().core().rec(types);
                                } else {
                                    // it's implicit!
                                    for subty in types {
                                        new_comp.core_type().core().subtype(&subty);
                                    }
                                }
                            }
                            CoreType::Module(module) => {
                                // NOTE: The indices in this struct DO NOT get fixed, they are assumed to
                                // be okay (fixing would add complexity in terms of index spaces having scopes.
                                // This can be added in the future.
                                let enc = new_comp.core_type();
                                convert_module_type_declaration(module, enc, reencode);
                            }
                        },
                        ComponentTypeDeclaration::Type(typ) => {
                            // TODO: This needs to be fixed
                            //       infinite depth??
                            let enc = new_comp.ty();
                            convert_component_type(
                                &(*typ).clone(),
                                enc,
                                component,
                                reencode,
                                indices,
                            );
                        }
                        ComponentTypeDeclaration::Alias(a) => {
                            convert_component_alias(a, component, &mut new_comp, reencode, indices)
                        }
                        ComponentTypeDeclaration::Export { name, ty } => {
                            // NOTE: this is self-contained, so theoretically instrumentation should
                            //       insert new types that don't need to be changed.
                            //       (to truly fix, a (type (component ...)) decl would need to carry its own index space...
                            // Will not support fixing such indices for now.

                            let ty = do_reencode(
                                *ty,
                                RoundtripReencoder::component_type_ref,
                                reencode,
                                "component type",
                            );
                            new_comp.export(name.0, ty);
                        }
                        ComponentTypeDeclaration::Import(imp) => {
                            // NOTE: this is self-contained, so theoretically instrumentation should
                            //       insert new types that don't need to be changed.
                            //       (to truly fix, a (type (component ...)) decl would need to carry its own index space...
                            // Will not support fixing such indices for now.

                            let ty = do_reencode(
                                imp.ty,
                                RoundtripReencoder::component_type_ref,
                                reencode,
                                "component type",
                            );
                            new_comp.import(imp.name.0, ty);
                        }
                    }
                }
                component_ty_section.component(&new_comp);
            }
            ComponentType::Instance(inst) => {
                // TODO: This needs to be fixed
                component_ty_section
                    .instance(&convert_instance_type(inst, component, reencode, indices));
            }
            ComponentType::Resource { rep, dtor } => {
                // TODO: This needs to be fixed (the dtor likely points to a function)
                component_ty_section.resource(reencode.val_type(*rep).unwrap(), *dtor);
            }
        }

        component.section(&component_ty_section);
    }
}

impl Encode for ComponentInstance<'_> {
    fn do_encode<'a>(
        &self,
        component: &mut wasm_encoder::Component,
        indices: &IdxSpaces,
        reencode: &mut RoundtripReencoder,
    ) {
        let mut instances = wasm_encoder::ComponentInstanceSection::new();

        let refs = self.referenced_indices();
        match self {
            ComponentInstance::Instantiate { args, .. } => {
                let comp = refs.as_ref().unwrap().comp();
                let new_id = indices.lookup_actual_id_or_panic(&comp);

                instances.instantiate(
                    new_id as u32,
                    args.iter().map(|arg| {
                        let fixed = arg.fix(component, indices, reencode);
                        (
                            fixed.name,
                            reencode.component_export_kind(fixed.kind),
                            fixed.index,
                        )
                    }),
                );
            }
            ComponentInstance::FromExports(export) => {
                instances.export_items(export.iter().map(|value| {
                    let fixed = value.fix(component, indices, reencode);
                    (
                        fixed.name.0,
                        reencode.component_export_kind(fixed.kind),
                        fixed.index,
                    )
                }));
            }
        }

        component.section(&instances);
    }
}

impl Encode for CanonicalFunction {
    fn do_encode<'a>(
        &self,
        component: &mut wasm_encoder::Component,
        indices: &IdxSpaces,
        reencode: &mut RoundtripReencoder,
    ) {
        let mut canon_sec = wasm_encoder::CanonicalFunctionSection::new();

        let refs = self.referenced_indices();
        match self {
            CanonicalFunction::Lift {
                options: options_orig,
                ..
            } => {
                let func = refs.as_ref().unwrap().func();
                let ty = refs.as_ref().unwrap().ty();
                let new_fid = indices.lookup_actual_id_or_panic(&func);
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(component, indices, reencode));
                }

                canon_sec.lift(
                    new_fid as u32,
                    new_tid as u32,
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
            CanonicalFunction::Lower {
                options: options_orig,
                ..
            } => {
                let func = refs.as_ref().unwrap().func();
                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(component, indices, reencode));
                }

                let new_fid = indices.lookup_actual_id_or_panic(&func);
                canon_sec.lower(
                    new_fid as u32,
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
            CanonicalFunction::ResourceNew { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                canon_sec.resource_new(new_tid as u32);
            }
            CanonicalFunction::ResourceDrop { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                canon_sec.resource_drop(new_tid as u32);
            }
            CanonicalFunction::ResourceRep { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                canon_sec.resource_rep(new_tid as u32);
            }
            CanonicalFunction::ResourceDropAsync { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                canon_sec.resource_drop_async(new_tid as u32);
            }
            CanonicalFunction::ThreadAvailableParallelism => {
                canon_sec.thread_available_parallelism();
            }
            CanonicalFunction::BackpressureSet => {
                canon_sec.backpressure_set();
            }
            CanonicalFunction::TaskReturn {
                result,
                options: options_orig,
            } => {
                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(component, indices, reencode).into());
                }
                let result = result.map(|v| {
                    let fixed = v.fix(component, indices, reencode);
                    fixed.into()
                });
                canon_sec.task_return(result, fixed_options);
            }
            CanonicalFunction::WaitableSetNew => {
                canon_sec.waitable_set_new();
            }
            CanonicalFunction::WaitableSetWait { cancellable, .. } => {
                // NOTE: There's a discrepancy in naming here. `cancellable` refers to the same bit as `async_`
                let mem = refs.as_ref().unwrap().mem();
                let new_mid = indices.lookup_actual_id_or_panic(&mem);
                canon_sec.waitable_set_wait(*cancellable, new_mid as u32);
            }
            CanonicalFunction::WaitableSetPoll { cancellable, .. } => {
                // NOTE: There's a discrepancy in naming here. `cancellable` refers to the same bit as `async_`
                let mem = refs.as_ref().unwrap().mem();
                let new_mid = indices.lookup_actual_id_or_panic(&mem);
                canon_sec.waitable_set_poll(*cancellable, new_mid as u32);
            }
            CanonicalFunction::WaitableSetDrop => {
                canon_sec.waitable_set_drop();
            }
            CanonicalFunction::WaitableJoin => {
                canon_sec.waitable_join();
            }
            CanonicalFunction::SubtaskDrop => {
                canon_sec.subtask_drop();
            }
            CanonicalFunction::StreamNew { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                canon_sec.stream_new(new_tid as u32);
            }
            CanonicalFunction::StreamRead {
                options: options_orig,
                ..
            } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);

                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(component, indices, reencode).into());
                }

                canon_sec.stream_read(new_tid as u32, fixed_options);
            }
            CanonicalFunction::StreamWrite {
                options: options_orig,
                ..
            } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);

                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(component, indices, reencode).into());
                }

                canon_sec.stream_write(new_tid as u32, fixed_options);
            }
            CanonicalFunction::StreamCancelRead { async_, .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                canon_sec.stream_cancel_read(new_tid as u32, *async_);
            }
            CanonicalFunction::StreamCancelWrite { async_, .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                canon_sec.stream_cancel_write(new_tid as u32, *async_);
            }
            CanonicalFunction::FutureNew { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                canon_sec.future_new(new_tid as u32);
            }
            CanonicalFunction::FutureRead {
                options: options_orig,
                ..
            } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);

                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(component, indices, reencode).into());
                }
                canon_sec.future_read(new_tid as u32, fixed_options);
            }
            CanonicalFunction::FutureWrite {
                options: options_orig,
                ..
            } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);

                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(component, indices, reencode).into());
                }
                canon_sec.future_write(new_tid as u32, fixed_options);
            }
            CanonicalFunction::FutureCancelRead { async_, .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                canon_sec.future_cancel_read(new_tid as u32, *async_);
            }
            CanonicalFunction::FutureCancelWrite { async_, .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                canon_sec.future_cancel_write(new_tid as u32, *async_);
            }
            CanonicalFunction::ErrorContextNew {
                options: options_orig,
            } => {
                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(component, indices, reencode).into());
                }
                canon_sec.error_context_new(fixed_options);
            }
            CanonicalFunction::ErrorContextDebugMessage {
                options: options_orig,
            } => {
                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(component, indices, reencode).into());
                }
                canon_sec.error_context_debug_message(fixed_options);
            }
            CanonicalFunction::ErrorContextDrop => {
                canon_sec.error_context_drop();
            }
            CanonicalFunction::ThreadSpawnRef { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                canon_sec.thread_spawn_ref(new_tid as u32);
            }
            CanonicalFunction::ThreadSpawnIndirect { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let table = refs.as_ref().unwrap().table();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                let new_tbl_id = indices.lookup_actual_id_or_panic(&table);
                canon_sec.thread_spawn_indirect(new_tid as u32, new_tbl_id as u32);
            }
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
            CanonicalFunction::StreamDropReadable { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                canon_sec.stream_drop_readable(new_tid as u32);
            }
            CanonicalFunction::StreamDropWritable { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                canon_sec.stream_drop_writable(new_tid as u32);
            }
            CanonicalFunction::FutureDropReadable { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                canon_sec.future_drop_readable(new_tid as u32);
            }
            CanonicalFunction::FutureDropWritable { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                canon_sec.future_drop_writable(new_tid as u32);
            }
            CanonicalFunction::BackpressureInc => {
                canon_sec.backpressure_inc();
            }
            CanonicalFunction::BackpressureDec => {
                canon_sec.backpressure_dec();
            }
            CanonicalFunction::ThreadYield { cancellable } => {
                canon_sec.thread_yield(*cancellable);
            }
            CanonicalFunction::ThreadIndex => {
                canon_sec.thread_index();
            }
            CanonicalFunction::ThreadNewIndirect { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let table = refs.as_ref().unwrap().table();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                let new_tbl_id = indices.lookup_actual_id_or_panic(&table);
                canon_sec.thread_new_indirect(new_tid as u32, new_tbl_id as u32);
            }
            CanonicalFunction::ThreadSwitchTo { cancellable } => {
                canon_sec.thread_switch_to(*cancellable);
            }
            CanonicalFunction::ThreadSuspend { cancellable } => {
                canon_sec.thread_suspend(*cancellable);
            }
            CanonicalFunction::ThreadResumeLater => {
                canon_sec.thread_resume_later();
            }
            CanonicalFunction::ThreadYieldTo { cancellable } => {
                canon_sec.thread_yield_to(*cancellable);
            }
        }
        component.section(&canon_sec);
    }
}

impl Encode for ComponentAlias<'_> {
    fn do_encode<'a>(
        &self,
        component: &mut wasm_encoder::Component,
        indices: &IdxSpaces,
        reencode: &mut RoundtripReencoder,
    ) {
        let mut alias = ComponentAliasSection::new();
        let a = match self {
            ComponentAlias::InstanceExport { .. } => {
                let ComponentAlias::InstanceExport {
                    kind,
                    instance_index,
                    name,
                } = self.fix(component, indices, reencode)
                else {
                    panic!()
                };
                Alias::InstanceExport {
                    instance: instance_index,
                    kind: reencode.component_export_kind(kind),
                    name,
                }
            }
            ComponentAlias::CoreInstanceExport { .. } => {
                let ComponentAlias::CoreInstanceExport {
                    kind,
                    instance_index,
                    name,
                } = self.fix(component, indices, reencode)
                else {
                    panic!()
                };
                Alias::CoreInstanceExport {
                    instance: instance_index,
                    kind: do_reencode(
                        kind,
                        RoundtripReencoder::export_kind,
                        reencode,
                        "export kind",
                    ),
                    name,
                }
            }
            ComponentAlias::Outer { .. } => {
                let ComponentAlias::Outer { kind, count, index } =
                    self.fix(component, indices, reencode)
                else {
                    panic!()
                };
                Alias::Outer {
                    kind: reencode.component_outer_alias_kind(kind),
                    count,
                    index,
                }
            }
        };

        alias.alias(a);
        component.section(&alias);
    }
}

impl Encode for ComponentImport<'_> {
    fn do_encode<'a>(
        &self,
        component: &mut wasm_encoder::Component,
        indices: &IdxSpaces,
        reencode: &mut RoundtripReencoder,
    ) {
        let mut imports = wasm_encoder::ComponentImportSection::new();

        let fixed_ty = self.ty.fix(component, indices, reencode);
        let ty = do_reencode(
            fixed_ty,
            RoundtripReencoder::component_type_ref,
            reencode,
            "component import",
        );
        imports.import(self.name.0, ty);

        component.section(&imports);
    }
}

impl Encode for ComponentExport<'_> {
    fn do_encode<'a>(
        &self,
        component: &mut wasm_encoder::Component,
        indices: &IdxSpaces,
        reencode: &mut RoundtripReencoder,
    ) {
        let mut exports = wasm_encoder::ComponentExportSection::new();

        let res = self.ty.map(|ty| {
            let fixed_ty = ty.fix(component, indices, reencode);
            do_reencode(
                fixed_ty,
                RoundtripReencoder::component_type_ref,
                reencode,
                "component export",
            )
        });

        // NOTE: We will not be fixing indices here (complexity)
        // let Some(Refs { misc: Some(misc),..}) = self.referenced_indices() else {
        //     panic!()
        // };
        // let new_id = indices.new_lookup_actual_id_or_panic(&misc);
        exports.export(
            self.name.0,
            reencode.component_export_kind(self.kind),
            self.index,
            res,
        );

        component.section(&exports);
    }
}

impl Encode for CoreType<'_> {
    fn do_encode<'a>(
        &self,
        component: &mut wasm_encoder::Component,
        indices: &IdxSpaces,
        reencode: &mut RoundtripReencoder,
    ) {
        let mut type_section = wasm_encoder::CoreTypeSection::new();

        match &self {
            CoreType::Rec(recgroup) => {
                let types = convert_recgroup(recgroup, reencode);

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

impl Encode for Instance<'_> {
    fn do_encode<'a>(
        &self,
        component: &mut wasm_encoder::Component,
        indices: &IdxSpaces,
        reencode: &mut RoundtripReencoder,
    ) {
        let mut instances = wasm_encoder::InstanceSection::new();

        match self {
            Instance::Instantiate {
                args: args_orig, ..
            } => {
                let refs = self.referenced_indices();
                let module = refs.as_ref().unwrap().module();
                let new_id = indices.lookup_actual_id_or_panic(&module);

                let mut args = vec![];
                for arg in args_orig.iter() {
                    args.push(arg.fix(component, indices, reencode));
                }
                instances.instantiate(
                    new_id as u32,
                    args.iter()
                        .map(|arg| (arg.name, ModuleArg::Instance(arg.index))),
                );
            }
            Instance::FromExports(exports) => {
                // NOTE: We will not be fixing indices here (complexity)
                // let mut exports = vec![];
                // for export in exports_orig.iter() {
                //     exports.push(export.fix(component, indices, reencode));
                // }

                instances.export_items(exports.iter().map(|export| {
                    (
                        export.name,
                        wasm_encoder::ExportKind::from(export.kind),
                        export.index,
                    )
                }));
            }
        }

        component.section(&instances);
    }
}

impl Encode for ComponentStartFunction {
    fn do_encode<'a>(
        &self,
        component: &mut wasm_encoder::Component,
        _indices: &IdxSpaces,
        _reencode: &mut RoundtripReencoder,
    ) {
        // TODO: reindex func_index and arguments!

        component.section(&wasm_encoder::ComponentStartSection {
            function_index: self.func_index,
            args: self.arguments.clone(),
            results: self.results,
        });
    }
}

impl Encode for CustomSection<'_> {
    fn do_encode<'a>(
        &self,
        component: &mut wasm_encoder::Component,
        _indices: &IdxSpaces,
        _reencode: &mut RoundtripReencoder,
    ) {
        component.section(&wasm_encoder::CustomSection {
            name: std::borrow::Cow::Borrowed(self.name),
            data: self.data.clone(),
        });
    }
}

impl FixIndices for ComponentExport<'_> {
    fn fix<'a>(
        &self,
        comp: &mut wasm_encoder::Component,
        indices: &IdxSpaces,
        reenc: &mut RoundtripReencoder,
    ) -> Self {
        let refs = self.referenced_indices();
        let misc = refs.as_ref().unwrap().misc();
        let new_id = indices.lookup_actual_id_or_panic(&misc);

        let fixed_ty = if let Some(ty) = &self.ty {
            Some(ty.fix(comp, indices, reenc))
        } else {
            None
        };

        ComponentExport {
            name: self.name,
            kind: self.kind.clone(),
            index: new_id as u32,
            ty: fixed_ty,
        }
    }
}

impl FixIndices for ComponentInstantiationArg<'_> {
    fn fix<'a>(
        &self,
        _: &mut wasm_encoder::Component,
        indices: &IdxSpaces,
        _: &mut RoundtripReencoder,
    ) -> Self {
        let refs = self.referenced_indices();
        let ty = refs.as_ref().unwrap().ty();
        let new_id = indices.lookup_actual_id_or_panic(&ty);

        ComponentInstantiationArg {
            name: self.name,
            kind: self.kind.clone(),
            index: new_id as u32,
        }
    }
}

impl FixIndices for ComponentValType {
    fn fix<'a>(
        &self,
        _component: &mut wasm_encoder::Component,
        indices: &IdxSpaces,
        _reencode: &mut RoundtripReencoder,
    ) -> Self {
        if let ComponentValType::Type(_) = self {
            let refs = self.referenced_indices();
            let ty = refs.as_ref().unwrap().ty();
            let new_id = indices.lookup_actual_id_or_panic(&ty);
            ComponentValType::Type(new_id as u32)
        } else {
            self.clone()
        }
    }
}

impl FixIndices for ComponentAlias<'_> {
    fn fix<'a>(
        &self,
        _component: &mut wasm_encoder::Component,
        indices: &IdxSpaces,
        _reencode: &mut RoundtripReencoder,
    ) -> Self {
        // NOTE: We will not be fixing indices here (complexity due to index spaces with scopes)
        self.clone()
    }
}

impl FixIndices for ComponentTypeRef {
    fn fix<'a>(
        &self,
        component: &mut wasm_encoder::Component,
        indices: &IdxSpaces,
        reencode: &mut RoundtripReencoder,
    ) -> Self {
        let refs = self.referenced_indices();
        match self {
            ComponentTypeRef::Module(_) => {
                let ty = refs.as_ref().unwrap().ty();
                let new_id = indices.lookup_actual_id_or_panic(&ty);
                ComponentTypeRef::Module(new_id as u32)
            }
            ComponentTypeRef::Value(ty) => {
                ComponentTypeRef::Value(ty.fix(component, indices, reencode))
            }
            ComponentTypeRef::Func(_) => {
                let ty = refs.as_ref().unwrap().ty();
                let new_id = indices.lookup_actual_id_or_panic(&ty);
                ComponentTypeRef::Func(new_id as u32)
            }
            ComponentTypeRef::Instance(_) => {
                let ty = refs.as_ref().unwrap().ty();
                let new_id = indices.lookup_actual_id_or_panic(&ty);
                ComponentTypeRef::Instance(new_id as u32)
            }
            ComponentTypeRef::Component(_) => {
                let ty = refs.as_ref().unwrap().ty();
                let new_id = indices.lookup_actual_id_or_panic(&ty);
                ComponentTypeRef::Component(new_id as u32)
            }
            ComponentTypeRef::Type(_) => self.clone(), // nothing to do
        }
    }
}

impl FixIndices for CanonicalOption {
    fn fix<'a>(
        &self,
        _: &mut wasm_encoder::Component,
        indices: &IdxSpaces,
        _: &mut RoundtripReencoder,
    ) -> Self {
        let refs = self.referenced_indices();
        match self {
            CanonicalOption::Realloc(_)
            | CanonicalOption::PostReturn(_)
            | CanonicalOption::Callback(_) => {
                let func = refs.as_ref().unwrap().func();
                let new_fid = indices.lookup_actual_id_or_panic(&func);
                match self {
                    CanonicalOption::Realloc(_) => CanonicalOption::Realloc(new_fid as u32),
                    CanonicalOption::PostReturn(_) => CanonicalOption::PostReturn(new_fid as u32),
                    CanonicalOption::Callback(_) => CanonicalOption::Callback(new_fid as u32),
                    _ => unreachable!(),
                }
            }
            CanonicalOption::CoreType(_) => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                CanonicalOption::CoreType(new_tid as u32)
            }

            CanonicalOption::Memory(_) => {
                let mem = refs.as_ref().unwrap().mem();
                let new_mid = indices.lookup_actual_id_or_panic(&mem);
                CanonicalOption::Memory(new_mid as u32)
            }
            CanonicalOption::UTF8
            | CanonicalOption::UTF16
            | CanonicalOption::CompactUTF16
            | CanonicalOption::Async
            | CanonicalOption::Gc => self.clone(),
        }
    }
}

impl FixIndices for InstantiationArg<'_> {
    fn fix<'a>(
        &self,
        _component: &mut wasm_encoder::Component,
        indices: &IdxSpaces,
        _reencode: &mut RoundtripReencoder,
    ) -> Self {
        let refs = self.referenced_indices();
        let misc = refs.as_ref().unwrap().misc();
        let new_id = indices.lookup_actual_id_or_panic(&misc);
        Self {
            name: self.name,
            kind: self.kind.clone(),
            index: new_id as u32,
        }
    }
}

impl FixIndices for Export<'_> {
    fn fix<'a>(
        &self,
        _component: &mut wasm_encoder::Component,
        indices: &IdxSpaces,
        _reencode: &mut RoundtripReencoder,
    ) -> Self {
        let refs = self.referenced_indices();
        let misc = refs.as_ref().unwrap().misc();
        let new_id = indices.lookup_actual_id_or_panic(&misc);
        Self {
            name: self.name,
            kind: self.kind.clone(),
            index: new_id as u32,
        }
    }
}

impl FixIndices for InstanceTypeDeclaration<'_> {
    fn fix<'a>(
        &self,
        component: &mut wasm_encoder::Component,
        indices: &IdxSpaces,
        reencode: &mut RoundtripReencoder,
    ) -> Self {
        match self {
            InstanceTypeDeclaration::CoreType(core_type) => match core_type {
                CoreType::Rec(_) => todo!(),
                CoreType::Module(_) => todo!(),
            },
            InstanceTypeDeclaration::Type(_) => todo!(),
            InstanceTypeDeclaration::Alias(_) => todo!(),
            InstanceTypeDeclaration::Export { .. } => todo!(),
        }
    }
}

impl FixIndices for TypeRef {
    fn fix<'a>(
        &self,
        _component: &mut wasm_encoder::Component,
        indices: &IdxSpaces,
        _reencode: &mut RoundtripReencoder,
    ) -> Self {
        let refs = self.referenced_indices();
        match self {
            TypeRef::Func(_) => {
                let ty = refs.as_ref().unwrap().ty();
                let new_id = indices.lookup_actual_id_or_panic(&ty);
                TypeRef::Func(new_id as u32)
            }
            TypeRef::Tag(TagType {
                kind,
                func_type_idx: _,
            }) => {
                let ty = refs.as_ref().unwrap().ty();
                let new_id = indices.lookup_actual_id_or_panic(&ty);
                TypeRef::Tag(TagType {
                    kind: kind.clone(),
                    func_type_idx: new_id as u32,
                })
            }
            TypeRef::FuncExact(_) => {
                let ty = refs.as_ref().unwrap().ty();
                let new_id = indices.lookup_actual_id_or_panic(&ty);
                TypeRef::FuncExact(new_id as u32)
            }
            TypeRef::Table(_) | TypeRef::Memory(_) | TypeRef::Global(_) => self.clone(),
        }
    }
}

fn convert_component_type(
    ty: &ComponentType,
    enc: ComponentTypeEncoder,
    component: &mut wasm_encoder::Component,
    reencode: &mut RoundtripReencoder,
    indices: &IdxSpaces,
) {
    match ty {
        ComponentType::Defined(comp_ty) => {
            let def_enc = enc.defined_type();
            match comp_ty {
                wasmparser::ComponentDefinedType::Primitive(p) => {
                    def_enc.primitive(wasm_encoder::PrimitiveValType::from(*p))
                }
                wasmparser::ComponentDefinedType::Record(record) => {
                    def_enc.record(
                        record
                            .iter()
                            .map(|record| (record.0, reencode.component_val_type(record.1))),
                    );
                }
                wasmparser::ComponentDefinedType::Variant(variant) => {
                    def_enc.variant(variant.iter().map(|variant| {
                        (
                            variant.name,
                            variant.ty.map(|ty| reencode.component_val_type(ty)),
                            variant.refines,
                        )
                    }))
                }
                wasmparser::ComponentDefinedType::List(l) => {
                    def_enc.list(reencode.component_val_type(*l))
                }
                wasmparser::ComponentDefinedType::Tuple(tup) => def_enc.tuple(
                    tup.iter()
                        .map(|val_type| reencode.component_val_type(*val_type)),
                ),
                wasmparser::ComponentDefinedType::Flags(flags) => {
                    def_enc.flags((*flags).clone().into_vec())
                }
                wasmparser::ComponentDefinedType::Enum(en) => {
                    def_enc.enum_type((*en).clone().into_vec())
                }
                wasmparser::ComponentDefinedType::Option(opt) => {
                    def_enc.option(reencode.component_val_type(*opt))
                }
                wasmparser::ComponentDefinedType::Result { ok, err } => def_enc.result(
                    ok.map(|val_type| reencode.component_val_type(val_type)),
                    err.map(|val_type| reencode.component_val_type(val_type)),
                ),
                wasmparser::ComponentDefinedType::Own(u) => def_enc.own(*u),
                wasmparser::ComponentDefinedType::Borrow(u) => def_enc.borrow(*u),
                wasmparser::ComponentDefinedType::Future(opt) => match opt {
                    Some(u) => def_enc.future(Some(reencode.component_val_type(*u))),
                    None => def_enc.future(None),
                },
                wasmparser::ComponentDefinedType::Stream(opt) => match opt {
                    Some(u) => def_enc.stream(Some(reencode.component_val_type(*u))),
                    None => def_enc.future(None),
                },
                wasmparser::ComponentDefinedType::FixedSizeList(ty, len) => {
                    def_enc.fixed_size_list(reencode.component_val_type(*ty), *len)
                }
            }
        }
        ComponentType::Func(func_ty) => {
            let mut new_enc = enc.function();
            new_enc.params(
                func_ty
                    .clone()
                    .params
                    .into_vec()
                    .into_iter()
                    .map(|p| (p.0, reencode.component_val_type(p.1))),
            );
            convert_results(func_ty.clone().result, new_enc, reencode);
        }
        ComponentType::Component(comp) => {
            let mut new_comp = wasm_encoder::ComponentType::new();
            for c in comp.iter() {
                match c {
                    ComponentTypeDeclaration::CoreType(core) => match core {
                        CoreType::Rec(recgroup) => {
                            for sub in recgroup.types() {
                                let enc = new_comp.core_type().core();
                                encode_core_type_subtype(enc, sub, reencode);
                            }
                        }
                        CoreType::Module(module) => {
                            let enc = new_comp.core_type();
                            convert_module_type_declaration(module, enc, reencode);
                        }
                    },
                    ComponentTypeDeclaration::Type(typ) => {
                        let enc = new_comp.ty();
                        convert_component_type(typ, enc, component, reencode, indices);
                    }
                    ComponentTypeDeclaration::Alias(a) => {
                        convert_component_alias(a, component, &mut new_comp, reencode, indices)
                    }
                    ComponentTypeDeclaration::Export { name, ty } => {
                        new_comp.export(
                            name.0,
                            do_reencode(
                                *ty,
                                RoundtripReencoder::component_type_ref,
                                reencode,
                                "component type",
                            ),
                        );
                    }
                    ComponentTypeDeclaration::Import(imp) => {
                        new_comp.import(
                            imp.name.0,
                            do_reencode(
                                imp.ty,
                                RoundtripReencoder::component_type_ref,
                                reencode,
                                "component type",
                            ),
                        );
                    }
                }
            }
            enc.component(&new_comp);
        }
        ComponentType::Instance(inst) => {
            let ity = convert_instance_type(inst, component, reencode, indices);
            enc.instance(&ity);
        }
        ComponentType::Resource { rep, dtor } => {
            enc.resource(reencode.val_type(*rep).unwrap(), *dtor);
        }
    }
}

fn convert_component_alias(
    alias: &ComponentAlias,
    component: &mut wasm_encoder::Component,
    comp_ty: &mut wasm_encoder::ComponentType,
    reencode: &mut RoundtripReencoder,
    indices: &IdxSpaces,
) {
    let new_a = match alias {
        ComponentAlias::InstanceExport { .. } => {
            let ComponentAlias::InstanceExport {
                kind,
                instance_index,
                name,
            } = alias.fix(component, indices, reencode)
            else {
                panic!()
            };
            Alias::InstanceExport {
                instance: instance_index,
                kind: reencode.component_export_kind(kind),
                name,
            }
        }
        ComponentAlias::CoreInstanceExport { .. } => {
            let ComponentAlias::CoreInstanceExport {
                kind,
                instance_index,
                name,
            } = alias.fix(component, indices, reencode)
            else {
                panic!()
            };
            Alias::CoreInstanceExport {
                instance: instance_index,
                kind: do_reencode(
                    kind,
                    RoundtripReencoder::export_kind,
                    reencode,
                    "export kind",
                ),
                name,
            }
        }
        ComponentAlias::Outer { .. } => {
            let ComponentAlias::Outer { kind, count, index } =
                alias.fix(component, indices, reencode)
            else {
                panic!()
            };
            Alias::Outer {
                kind: reencode.component_outer_alias_kind(kind),
                count,
                index,
            }
        }
    };
    comp_ty.alias(new_a);
}

fn convert_instance_type(
    instance: &[InstanceTypeDeclaration],
    component: &mut wasm_encoder::Component,
    reencode: &mut RoundtripReencoder,
    indices: &IdxSpaces,
) -> InstanceType {
    let mut ity = InstanceType::new();
    for value in instance.iter() {
        match value {
            InstanceTypeDeclaration::CoreType(core_type) => match core_type {
                CoreType::Rec(recgroup) => {
                    for sub in recgroup.types() {
                        let enc = ity.core_type().core();
                        encode_core_type_subtype(enc, sub, reencode);
                    }
                }
                CoreType::Module(module) => {
                    let enc = ity.core_type();
                    convert_module_type_declaration(module, enc, reencode);
                }
            },
            InstanceTypeDeclaration::Type(ty) => {
                let enc = ity.ty();
                convert_component_type(ty, enc, component, reencode, indices);
            }
            InstanceTypeDeclaration::Alias(alias) => match alias {
                ComponentAlias::InstanceExport { .. } => {
                    let ComponentAlias::InstanceExport {
                        kind,
                        instance_index,
                        name,
                    } = alias.fix(component, indices, reencode)
                    else {
                        panic!()
                    };
                    ity.alias(Alias::InstanceExport {
                        instance: instance_index,
                        kind: reencode.component_export_kind(kind),
                        name,
                    });
                }
                ComponentAlias::CoreInstanceExport { .. } => {
                    let ComponentAlias::CoreInstanceExport {
                        kind,
                        instance_index,
                        name,
                    } = alias.fix(component, indices, reencode)
                    else {
                        panic!()
                    };
                    ity.alias(Alias::CoreInstanceExport {
                        instance: instance_index,
                        kind: do_reencode(
                            kind,
                            RoundtripReencoder::export_kind,
                            reencode,
                            "export kind",
                        ),
                        name,
                    });
                }
                ComponentAlias::Outer { .. } => {
                    let ComponentAlias::Outer { kind, count, index } =
                        alias.fix(component, indices, reencode)
                    else {
                        panic!()
                    };
                    ity.alias(Alias::Outer {
                        kind: reencode.component_outer_alias_kind(kind),
                        count,
                        index,
                    });
                }
            },
            InstanceTypeDeclaration::Export { name, ty } => {
                ity.export(
                    name.0,
                    do_reencode(
                        *ty,
                        RoundtripReencoder::component_type_ref,
                        reencode,
                        "component type",
                    ),
                );
            }
        }
    }
    ity
}

// Not added to wasm-tools
/// CoreTypeEncoding
pub fn encode_core_type_subtype(
    enc: CoreTypeEncoder,
    subtype: &SubType,
    reencode: &mut RoundtripReencoder,
) {
    let subty = reencode
        .sub_type(subtype.to_owned())
        .unwrap_or_else(|_| panic!("Could not encode type as subtype: {:?}", subtype));
    enc.subtype(&subty);
}

// Not added to wasm-tools
/// Convert Func Results
pub fn convert_results(
    result: Option<ComponentValType>,
    mut enc: ComponentFuncTypeEncoder,
    reencode: &mut RoundtripReencoder,
) {
    enc.result(result.map(|v| reencode.component_val_type(v)));
}
