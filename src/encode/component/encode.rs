use crate::encode::component::collect::{ComponentItem, ComponentPlan};
use crate::ir::component::idx_spaces::{IdxSpaces};
use crate::ir::types::CustomSection;
use crate::ir::wrappers::{convert_module_type_declaration, convert_recgroup, do_reencode};
use crate::{Component, Module};
use wasm_encoder::reencode::{Reencode, ReencodeComponent, RoundtripReencoder};
use wasm_encoder::{
    Alias, ComponentAliasSection, ComponentFuncTypeEncoder, ComponentTypeEncoder, CoreTypeEncoder,
    InstanceType, ModuleArg, ModuleSection, NestedComponentSection,
};
use wasmparser::{CanonicalFunction, ComponentAlias, ComponentDefinedType, ComponentExport, ComponentImport, ComponentInstance, ComponentStartFunction, ComponentType, ComponentTypeDeclaration, ComponentValType, CoreType, Instance, InstanceTypeDeclaration, SubType};
use crate::encode::component::fix_indices::FixIndices;

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
                // let fixed = t.fix(&mut component, indices, &mut reencode);
                t.do_encode(&mut component, &mut reencode);
            },
            ComponentItem::CompType { node, .. } => unsafe {
                let t: &ComponentType = &**node;
                let fixed = t.fix(&mut component, indices);
                fixed.do_encode(&mut component, &mut reencode);
            },
            ComponentItem::CompInst { node, .. } => unsafe {
                let i: &ComponentInstance = &**node;
                let fixed = i.fix(&mut component, indices);
                fixed.do_encode(&mut component, &mut reencode);
            },
            ComponentItem::CanonicalFunc { node, .. } => unsafe {
                let f: &CanonicalFunction = &**node;
                let fixed = f.fix(&mut component, indices);
                fixed.do_encode(&mut component, &mut reencode);
            },
            ComponentItem::Alias { node, .. } => unsafe {
                let a: &ComponentAlias = &**node;
                let fixed = a.fix(&mut component, indices);
                fixed.do_encode(&mut component, &mut reencode);
            },
            ComponentItem::Import { node, .. } => unsafe {
                let i: &ComponentImport = &**node;
                let fixed = i.fix(&mut component, indices);
                fixed.do_encode(&mut component, &mut reencode);
            },
            ComponentItem::Export { node, .. } => unsafe {
                let e: &ComponentExport = &**node;
                let fixed = e.fix(&mut component, indices);
                fixed.do_encode(&mut component, &mut reencode);
            },
            ComponentItem::CoreType { node, .. } => unsafe {
                let t: &CoreType = &**node;
                let fixed = t.fix(&mut component, indices);
                fixed.do_encode(&mut component, &mut reencode);
            },
            ComponentItem::Inst { node, .. } => unsafe {
                let i: &Instance = &**node;
                let fixed = i.fix(&mut component, indices);
                fixed.do_encode(&mut component, &mut reencode);
            },
            ComponentItem::Start { node, .. } => unsafe {
                let f: &ComponentStartFunction = &**node;
                let fixed = f.fix(&mut component, indices);
                fixed.do_encode(&mut component, &mut reencode);
            },
            ComponentItem::CustomSection { node, .. } => unsafe {
                let c: &CustomSection = &**node;
                let fixed = c.fix(&mut component, indices);
                fixed.do_encode(&mut component, &mut reencode);
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
        reencode: &mut RoundtripReencoder,
    );
}

impl Encode for Module<'_> {
    fn do_encode<'a>(
        &self,
        component: &mut wasm_encoder::Component,
        _reencode: &mut RoundtripReencoder,
    ) {
        component.section(&ModuleSection(&self.encode_internal(false).0));
    }
}

impl Encode for ComponentType<'_> {
    fn do_encode<'a>(
        &self,
        component: &mut wasm_encoder::Component,
        reencode: &mut RoundtripReencoder,
    ) {
        let mut component_ty_section = wasm_encoder::ComponentTypeSection::new();

        match self {
            ComponentType::Defined(comp_ty) => {
                let enc = component_ty_section.defined_type();
                match comp_ty {
                    ComponentDefinedType::Primitive(p) => {
                        enc.primitive(wasm_encoder::PrimitiveValType::from(*p))
                    }
                    ComponentDefinedType::Record(records) => {
                        enc.record(records.iter().map(|(n, ty)| {
                            (*n, reencode.component_val_type(*ty))
                        }));
                    }
                    ComponentDefinedType::Variant(variants) => {
                        enc.variant(variants.iter().map(|variant| {
                            (
                                variant.name,
                                variant.ty.map(|ty| {
                                    reencode.component_val_type(ty)
                                }),
                                variant.refines,
                            )
                        }))
                    }
                    ComponentDefinedType::List(l) => {
                        enc.list(reencode.component_val_type(*l))
                    }
                    ComponentDefinedType::Tuple(tup) => {
                        enc.tuple(tup.iter().map(|val_type| {
                            reencode.component_val_type(*val_type)
                        }))
                    }
                    ComponentDefinedType::Flags(flags) => {
                        enc.flags(flags.clone().into_vec().into_iter())
                    }
                    ComponentDefinedType::Enum(en) => {
                        enc.enum_type(en.clone().into_vec().into_iter())
                    }
                    ComponentDefinedType::Option(opt) => {
                        enc.option(reencode.component_val_type(*opt))
                    }
                    ComponentDefinedType::Result { ok, err } => enc.result(
                        ok.map(|val_type| {
                            reencode.component_val_type(val_type)
                        }),
                        err.map(|val_type| {
                            reencode.component_val_type(val_type)
                        }),
                    ),
                    ComponentDefinedType::Own(id) => enc.own(*id),
                    ComponentDefinedType::Borrow(id) => enc.borrow(*id),
                    ComponentDefinedType::Future(opt) => enc.future(opt.map(|opt| {
                        reencode.component_val_type(opt)
                    })),
                    ComponentDefinedType::Stream(opt) => enc.stream(opt.map(|opt| {
                        reencode.component_val_type(opt)
                    })),
                    ComponentDefinedType::FixedSizeList(ty, i) => {
                        enc.fixed_size_list(reencode.component_val_type(*ty), *i)
                    }
                }
            }
            ComponentType::Func(func_ty) => {
                let mut enc = component_ty_section.function();
                enc.params(func_ty.params.iter().map(|p: &(&str, ComponentValType)| {
                    (p.0, reencode.component_val_type(p.1))
                }));
                enc.result(func_ty.result.map(|v| {
                    reencode.component_val_type(v)
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
                                let enc = new_comp.core_type();
                                convert_module_type_declaration(module, enc, reencode);
                            }
                        },
                        ComponentTypeDeclaration::Type(typ) => {
                            convert_component_type(
                                typ,
                                new_comp.ty(),
                                component,
                                reencode
                            );
                        }
                        ComponentTypeDeclaration::Alias(a) => {
                            convert_component_alias(a, &mut new_comp, reencode)
                        }
                        ComponentTypeDeclaration::Export { name, ty } => {
                            let ty = do_reencode(
                                *ty,
                                RoundtripReencoder::component_type_ref,
                                reencode,
                                "component type",
                            );
                            new_comp.export(name.0, ty);
                        }
                        ComponentTypeDeclaration::Import(imp) => {
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
                component_ty_section
                    .instance(&convert_instance_type(inst, component, reencode));
            }
            ComponentType::Resource { rep, dtor } => {
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
        reencode: &mut RoundtripReencoder,
    ) {
        let mut instances = wasm_encoder::ComponentInstanceSection::new();

        match self {
            ComponentInstance::Instantiate { component_index, args } => {
                instances.instantiate(
                    *component_index,
                    args.iter().map(|arg| {
                        (
                            arg.name,
                            reencode.component_export_kind(arg.kind),
                            arg.index,
                        )
                    }),
                );
            }
            ComponentInstance::FromExports(export) => {
                instances.export_items(export.iter().map(|value| {
                    (
                        value.name.0,
                        reencode.component_export_kind(value.kind),
                        value.index,
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
        reencode: &mut RoundtripReencoder,
    ) {
        let mut canon_sec = wasm_encoder::CanonicalFunctionSection::new();

        match self {
            CanonicalFunction::Lift {
                core_func_index, type_index, options
            } => {
                canon_sec.lift(
                    *core_func_index,
                    *type_index,
                    options.iter().map(|canon| {
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
                func_index, options
            } => {
                canon_sec.lower(
                    *func_index,
                    options.iter().map(|canon| {
                        do_reencode(
                            *canon,
                            RoundtripReencoder::canonical_option,
                            reencode,
                            "canonical option",
                        )
                    }),
                );
            }
            CanonicalFunction::ResourceNew { resource } => {
                canon_sec.resource_new(*resource);
            }
            CanonicalFunction::ResourceDrop { resource } => {
                canon_sec.resource_drop(*resource);
            }
            CanonicalFunction::ResourceRep { resource } => {
                canon_sec.resource_rep(*resource);
            }
            CanonicalFunction::ResourceDropAsync { resource } => {
                canon_sec.resource_drop_async(*resource);
            }
            CanonicalFunction::ThreadAvailableParallelism => {
                canon_sec.thread_available_parallelism();
            }
            CanonicalFunction::BackpressureSet => {
                canon_sec.backpressure_set();
            }
            CanonicalFunction::TaskReturn {
                result,
                options
            } => {
                canon_sec.task_return(
                    result.map(|v| {
                        v.into()
                    }),
                    options.iter().map(|opt| (*opt).into())
                );
            }
            CanonicalFunction::WaitableSetNew => {
                canon_sec.waitable_set_new();
            }
            CanonicalFunction::WaitableSetWait { cancellable, memory} => {
                // NOTE: There's a discrepancy in naming here. `cancellable` refers to the same bit as `async_`
                canon_sec.waitable_set_wait(*cancellable, *memory);
            }
            CanonicalFunction::WaitableSetPoll { cancellable, memory } => {
                // NOTE: There's a discrepancy in naming here. `cancellable` refers to the same bit as `async_`
                canon_sec.waitable_set_poll(*cancellable, *memory);
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
            CanonicalFunction::StreamNew { ty } => {
                canon_sec.stream_new(*ty);
            }
            CanonicalFunction::StreamRead {
                ty, options
            } => {
                canon_sec.stream_read(*ty, options.iter().map(|opt| (*opt).into()));
            }
            CanonicalFunction::StreamWrite {
                ty,
                options
            } => {
                canon_sec.stream_write(*ty, options.iter().map(|opt| (*opt).into()));
            }
            CanonicalFunction::StreamCancelRead { async_, ty } => {
                canon_sec.stream_cancel_read(*ty, *async_);
            }
            CanonicalFunction::StreamCancelWrite { async_, ty } => {
                canon_sec.stream_cancel_write(*ty, *async_);
            }
            CanonicalFunction::FutureNew { ty } => {
                canon_sec.future_new(*ty);
            }
            CanonicalFunction::FutureRead {
                ty,
                options,
            } => {
                canon_sec.future_read(*ty, options.iter().map(|opt| (*opt).into()));
            }
            CanonicalFunction::FutureWrite {
                ty,
                options
            } => {
                canon_sec.future_write(*ty, options.iter().map(|opt| (*opt).into()));
            }
            CanonicalFunction::FutureCancelRead { async_, ty } => {
                canon_sec.future_cancel_read(*ty, *async_);
            }
            CanonicalFunction::FutureCancelWrite { async_, ty } => {
                canon_sec.future_cancel_write(*ty, *async_);
            }
            CanonicalFunction::ErrorContextNew {
                options
            } => {
                canon_sec.error_context_new(options.iter().map(|opt| (*opt).into()));
            }
            CanonicalFunction::ErrorContextDebugMessage {
                options
            } => {
                canon_sec.error_context_debug_message(options.iter().map(|opt| (*opt).into()));
            }
            CanonicalFunction::ErrorContextDrop => {
                canon_sec.error_context_drop();
            }
            CanonicalFunction::ThreadSpawnRef { func_ty_index } => {
                canon_sec.thread_spawn_ref(*func_ty_index);
            }
            CanonicalFunction::ThreadSpawnIndirect { func_ty_index, table_index } => {
                canon_sec.thread_spawn_indirect(*func_ty_index, *table_index);
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
            CanonicalFunction::StreamDropReadable { ty } => {
                canon_sec.stream_drop_readable(*ty);
            }
            CanonicalFunction::StreamDropWritable { ty } => {
                canon_sec.stream_drop_writable(*ty);
            }
            CanonicalFunction::FutureDropReadable { ty } => {
                canon_sec.future_drop_readable(*ty);
            }
            CanonicalFunction::FutureDropWritable { ty } => {
                canon_sec.future_drop_writable(*ty);
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
            CanonicalFunction::ThreadNewIndirect { func_ty_index, table_index } => {
                canon_sec.thread_new_indirect(*func_ty_index, *table_index);
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
        reencode: &mut RoundtripReencoder,
    ) {
        let mut alias = ComponentAliasSection::new();
        let a = match self {
            ComponentAlias::InstanceExport { kind,
                instance_index,
                name, } => {
                Alias::InstanceExport {
                    instance: *instance_index,
                    kind: reencode.component_export_kind(*kind),
                    name: *name,
                }
            }
            ComponentAlias::CoreInstanceExport { kind,
                instance_index,
                name, } => {
                Alias::CoreInstanceExport {
                    instance: *instance_index,
                    kind: do_reencode(
                        *kind,
                        RoundtripReencoder::export_kind,
                        reencode,
                        "export kind",
                    ),
                    name: *name,
                }
            }
            ComponentAlias::Outer { kind, count, index } => {
                Alias::Outer {
                    kind: reencode.component_outer_alias_kind(*kind),
                    count: *count,
                    index: *index,
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
        reencode: &mut RoundtripReencoder,
    ) {
        let mut imports = wasm_encoder::ComponentImportSection::new();

        let ty = do_reencode(
            self.ty,
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
        reencode: &mut RoundtripReencoder,
    ) {
        let mut exports = wasm_encoder::ComponentExportSection::new();

        let ty = self.ty.map(|ty| {
            do_reencode(
                ty,
                RoundtripReencoder::component_type_ref,
                reencode,
                "component export",
            )
        });

        exports.export(
            self.name.0,
            reencode.component_export_kind(self.kind),
            self.index,
            ty,
        );

        component.section(&exports);
    }
}

impl Encode for CoreType<'_> {
    fn do_encode<'a>(
        &self,
        component: &mut wasm_encoder::Component,
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
        _reencode: &mut RoundtripReencoder,
    ) {
        let mut instances = wasm_encoder::InstanceSection::new();

        match self {
            Instance::Instantiate {
                module_index, args
            } => {
                instances.instantiate(
                    *module_index,
                    args.iter()
                        .map(|arg| (arg.name, ModuleArg::Instance(arg.index))),
                );
            }
            Instance::FromExports(exports) => {
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
        _reencode: &mut RoundtripReencoder,
    ) {
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
        _reencode: &mut RoundtripReencoder,
    ) {
        component.section(&wasm_encoder::CustomSection {
            name: std::borrow::Cow::Borrowed(self.name),
            data: self.data.clone(),
        });
    }
}

fn convert_component_type(
    ty: &ComponentType,
    enc: ComponentTypeEncoder,
    component: &mut wasm_encoder::Component,
    reencode: &mut RoundtripReencoder,
) {
    match ty {
        ComponentType::Defined(comp_ty) => {
            let def_enc = enc.defined_type();
            match comp_ty {
                ComponentDefinedType::Primitive(p) => {
                    def_enc.primitive(wasm_encoder::PrimitiveValType::from(*p))
                }
                ComponentDefinedType::Record(record) => {
                    def_enc.record(
                        record
                            .iter()
                            .map(|record| (record.0, reencode.component_val_type(record.1))),
                    );
                }
                ComponentDefinedType::Variant(variant) => {
                    def_enc.variant(variant.iter().map(|variant| {
                        (
                            variant.name,
                            variant.ty.map(|ty| reencode.component_val_type(ty)),
                            variant.refines,
                        )
                    }))
                }
                ComponentDefinedType::List(l) => {
                    def_enc.list(reencode.component_val_type(*l))
                }
                ComponentDefinedType::Tuple(tup) => def_enc.tuple(
                    tup.iter()
                        .map(|val_type| reencode.component_val_type(*val_type)),
                ),
                ComponentDefinedType::Flags(flags) => {
                    def_enc.flags((*flags).clone().into_vec())
                }
                ComponentDefinedType::Enum(en) => {
                    def_enc.enum_type((*en).clone().into_vec())
                }
                ComponentDefinedType::Option(opt) => {
                    def_enc.option(reencode.component_val_type(*opt))
                }
                ComponentDefinedType::Result { ok, err } => def_enc.result(
                    ok.map(|val_type| reencode.component_val_type(val_type)),
                    err.map(|val_type| reencode.component_val_type(val_type)),
                ),
                ComponentDefinedType::Own(u) => def_enc.own(*u),
                ComponentDefinedType::Borrow(u) => def_enc.borrow(*u),
                ComponentDefinedType::Future(opt) => match opt {
                    Some(u) => def_enc.future(Some(reencode.component_val_type(*u))),
                    None => def_enc.future(None),
                },
                ComponentDefinedType::Stream(opt) => match opt {
                    Some(u) => def_enc.stream(Some(reencode.component_val_type(*u))),
                    None => def_enc.future(None),
                },
                ComponentDefinedType::FixedSizeList(ty, len) => {
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
                        convert_component_type(typ, enc, component, reencode);
                    }
                    ComponentTypeDeclaration::Alias(a) => {
                        convert_component_alias(a, &mut new_comp, reencode)
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
            let ity = convert_instance_type(inst, component, reencode);
            enc.instance(&ity);
        }
        ComponentType::Resource { rep, dtor } => {
            enc.resource(reencode.val_type(*rep).unwrap(), *dtor);
        }
    }
}

fn convert_component_alias(
    alias: &ComponentAlias,
    comp_ty: &mut wasm_encoder::ComponentType,
    reencode: &mut RoundtripReencoder,
) {
    let new_a = match alias {
        ComponentAlias::InstanceExport { kind,
            instance_index,
            name, } => {
            Alias::InstanceExport {
                instance: *instance_index,
                kind: reencode.component_export_kind(*kind),
                name,
            }
        }
        ComponentAlias::CoreInstanceExport { kind,
            instance_index,
            name, } => {
            Alias::CoreInstanceExport {
                instance: *instance_index,
                kind: do_reencode(
                    *kind,
                    RoundtripReencoder::export_kind,
                    reencode,
                    "export kind",
                ),
                name: *name,
            }
        }
        ComponentAlias::Outer { kind, count, index } => {
            Alias::Outer {
                kind: reencode.component_outer_alias_kind(*kind),
                count: *count,
                index: *index,
            }
        }
    };
    comp_ty.alias(new_a);
}

fn convert_instance_type(
    instance: &[InstanceTypeDeclaration],
    component: &mut wasm_encoder::Component,
    reencode: &mut RoundtripReencoder
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
                convert_component_type(ty, enc, component, reencode);
            }
            InstanceTypeDeclaration::Alias(alias) => match alias {
                ComponentAlias::InstanceExport {
                    kind,
                    instance_index,
                    name,
                } => {
                    ity.alias(Alias::InstanceExport {
                        instance: *instance_index,
                        kind: reencode.component_export_kind(*kind),
                        name,
                    });
                }
                ComponentAlias::CoreInstanceExport {
                    kind,
                    instance_index,
                    name,
                } => {
                    ity.alias(Alias::CoreInstanceExport {
                        instance: *instance_index,
                        kind: do_reencode(
                            *kind,
                            RoundtripReencoder::export_kind,
                            reencode,
                            "export kind",
                        ),
                        name,
                    });
                }
                ComponentAlias::Outer {
                    kind,
                    count,
                    index,
                } => {
                    ity.alias(Alias::Outer {
                        kind: reencode.component_outer_alias_kind(*kind),
                        count: *count,
                        index: *index,
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
