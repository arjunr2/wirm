// I want this file to be a bunch of oneliners (easier to read)!
#[rustfmt::skip]

use wasmparser::{ArrayType, CanonicalFunction, CanonicalOption, ComponentAlias, ComponentDefinedType, ComponentExport, ComponentFuncType, ComponentImport, ComponentInstance, ComponentInstantiationArg, ComponentStartFunction, ComponentType, ComponentTypeDeclaration, ComponentTypeRef, ComponentValType, CompositeInnerType, CompositeType, ContType, CoreType, Export, FieldType, FuncType, HeapType, Import, Instance, InstanceTypeDeclaration, InstantiationArg, ModuleTypeDeclaration, PackedIndex, PrimitiveValType, RecGroup, RefType, StorageType, StructType, SubType, TagType, TypeRef, UnpackedIndex, ValType, VariantCase};
use crate::encode::component::collect::SubItemPlan;
use crate::encode::component::EncodeCtx;
use crate::ir::component::idx_spaces::{Depth, ReferencedIndices};
use crate::ir::component::scopes::GetScopeKind;
use crate::ir::types::CustomSection;

mod sealed {
    pub trait Sealed {}
}
trait FixIndicesImpl {
    fn fixme(&self, subitem_plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self;
}
pub(crate) trait FixIndices: sealed::Sealed {
    fn fix<'a>(&self, subitem_plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self where Self: Sized;
}

impl<T> FixIndices for T where T: GetScopeKind + sealed::Sealed + FixIndicesImpl {
    fn fix<'a>(&self, subitem_plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self where Self: Sized {
        ctx.maybe_enter_scope(self);
        let fixed = self.fixme(subitem_plan, ctx);
        ctx.maybe_exit_scope(self);

        fixed
    }
}

impl sealed::Sealed for ComponentExport<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for ComponentExport<'_> {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        // ctx.maybe_enter_scope(self);
        let refs = self.referenced_indices(Depth::default());
        let misc = refs.as_ref().unwrap().misc();
        let new_id = ctx.lookup_actual_id_or_panic(&misc);

        let fixed_ty = self.ty.map(|ty| {
            ty.fix(plan, ctx)
        });

        // ctx.maybe_exit_scope(self);
        ComponentExport {
            name: self.name,
            kind: self.kind.clone(),
            index: new_id as u32,
            ty: fixed_ty,
        }
    }
}

impl sealed::Sealed for ComponentInstantiationArg<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for ComponentInstantiationArg<'_> {
    fn fixme<'a>(&self, _: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        let refs = self.referenced_indices(Depth::default());
        let ty = refs.as_ref().unwrap().ty();
        let new_id = ctx.lookup_actual_id_or_panic(&ty);

        ComponentInstantiationArg {
            name: self.name,
            kind: self.kind.clone(),
            index: new_id as u32,
        }
    }
}

impl sealed::Sealed for ComponentType<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for ComponentType<'_> {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        println!("\t---> ComponentType: {:p}", self);
        match self {
            ComponentType::Defined(ty) => ComponentType::Defined(ty.fix(plan, ctx)),
            ComponentType::Func(ty) => ComponentType::Func(ty.fix(plan, ctx)),
            ComponentType::Component(tys) => {
                let mut new_tys = vec![];
                for (idx, subplan) in plan.as_ref().unwrap().order().iter() {
                    let decl = &tys[*idx];
                    new_tys.push(decl.fix(subplan, ctx));
                }

                ComponentType::Component(new_tys.into_boxed_slice())
            },
            ComponentType::Instance(tys) => {
                let mut new_tys = vec![];
                for (idx, subplan) in plan.as_ref().unwrap().order().iter() {
                    let decl = &tys[*idx];
                    println!("\t---> comp_type: {:p}", decl);
                    new_tys.push(decl.fix(subplan, ctx));
                }

                ComponentType::Instance(new_tys.into_boxed_slice())
            },
            ComponentType::Resource { rep, dtor } => {
                ComponentType::Resource {
                    rep: rep.fix(plan, ctx),
                    dtor: dtor.map(|_| {
                        let refs = self.referenced_indices(Depth::default());
                        let func = refs.as_ref().unwrap().func();
                        ctx.lookup_actual_id_or_panic(&func) as u32
                    })
                }
            }
        }
    }
}

impl sealed::Sealed for ComponentInstance<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for ComponentInstance<'_> {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        match self {
            ComponentInstance::Instantiate { args, .. } => {
                let refs = self.referenced_indices(Depth::default());
                let comp = refs.as_ref().unwrap().comp();
                let new_id = ctx.lookup_actual_id_or_panic(&comp);

                ComponentInstance::Instantiate {
                    component_index: new_id as u32,
                    args: args.iter().map( | arg| {
                        arg.fix(plan, ctx)
                    }).collect(),
                }
            }
            ComponentInstance::FromExports(export) => ComponentInstance::FromExports(
                export.iter().map(|value| {
                    value.fix(plan, ctx)
                }).collect()
            )
        }
    }
}

impl sealed::Sealed for CanonicalFunction {}
#[rustfmt::skip]
impl FixIndicesImpl for CanonicalFunction {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        let refs = self.referenced_indices(Depth::default());
        match self {
            CanonicalFunction::Lift { options: options_orig, .. } => {
                let func = refs.as_ref().unwrap().func();
                let ty = refs.as_ref().unwrap().ty();
                let new_fid = ctx.lookup_actual_id_or_panic(&func);
                let new_tid = ctx.lookup_actual_id_or_panic(&ty);
                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(plan, ctx));
                }

                CanonicalFunction::Lift {
                    core_func_index: new_fid as u32,
                    type_index: new_tid as u32,
                    options: fixed_options.into_boxed_slice()
                }
            }
            CanonicalFunction::Lower { options: options_orig, .. } => {
                let func = refs.as_ref().unwrap().func();
                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(plan, ctx));
                }

                let new_fid = ctx.lookup_actual_id_or_panic(&func);
                CanonicalFunction::Lower {
                    func_index: new_fid as u32,
                    options: fixed_options.into_boxed_slice()
                }
            }
            CanonicalFunction::ResourceNew { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = ctx.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::ResourceNew { resource: new_tid as u32}
            }
            CanonicalFunction::ResourceDrop { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = ctx.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::ResourceDrop { resource: new_tid as u32}
            }
            CanonicalFunction::ResourceRep { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = ctx.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::ResourceRep { resource: new_tid as u32}
            }
            CanonicalFunction::ResourceDropAsync { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = ctx.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::ResourceDropAsync { resource: new_tid as u32}
            }
            CanonicalFunction::TaskReturn {
                result,
                options: options_orig,
            } => {
                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(plan, ctx));
                }
                CanonicalFunction::TaskReturn {
                    result: result.map(|v| {
                        v.fix(plan, ctx)
                    }),
                    options: fixed_options.into_boxed_slice()
                }
            }
            CanonicalFunction::WaitableSetWait { cancellable, .. } => {
                let mem = refs.as_ref().unwrap().mem();
                let new_mid = ctx.lookup_actual_id_or_panic(&mem);
                CanonicalFunction::WaitableSetWait {
                    cancellable: *cancellable,
                    memory: new_mid as u32,
                }
            }
            CanonicalFunction::WaitableSetPoll { cancellable, .. } => {
                let mem = refs.as_ref().unwrap().mem();
                let new_mid = ctx.lookup_actual_id_or_panic(&mem);
                CanonicalFunction::WaitableSetPoll {
                    cancellable: *cancellable,
                    memory: new_mid as u32,
                }
            }
            CanonicalFunction::StreamNew { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = ctx.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::StreamNew {
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::StreamRead { options: options_orig, .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = ctx.lookup_actual_id_or_panic(&ty);

                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(plan, ctx));
                }

                CanonicalFunction::StreamRead {
                    ty: new_tid as u32,
                    options: fixed_options.into_boxed_slice()
                }
            }
            CanonicalFunction::StreamWrite { options: options_orig, .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = ctx.lookup_actual_id_or_panic(&ty);

                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(plan, ctx));
                }

                CanonicalFunction::StreamWrite {
                    ty: new_tid as u32,
                    options: fixed_options.into_boxed_slice()
                }
            }
            CanonicalFunction::StreamCancelRead { async_, .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = ctx.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::StreamCancelRead {
                    async_: *async_,
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::StreamCancelWrite { async_, .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = ctx.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::StreamCancelWrite {
                    async_: *async_,
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::FutureNew { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = ctx.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::FutureNew {
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::FutureRead { options: options_orig, .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = ctx.lookup_actual_id_or_panic(&ty);

                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(plan, ctx));
                }
                CanonicalFunction::FutureRead {
                    ty: new_tid as u32,
                    options: fixed_options.into_boxed_slice()
                }
            }
            CanonicalFunction::FutureWrite { options: options_orig, .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = ctx.lookup_actual_id_or_panic(&ty);

                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(plan, ctx));
                }
                CanonicalFunction::FutureWrite {
                    ty: new_tid as u32,
                    options: fixed_options.into_boxed_slice()
                }
            }
            CanonicalFunction::FutureCancelRead { async_, .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = ctx.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::FutureCancelRead {
                    async_: *async_,
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::FutureCancelWrite { async_, .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = ctx.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::FutureCancelWrite {
                    async_: *async_,
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::ErrorContextNew { options: options_orig } => {
                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(plan, ctx));
                }
                CanonicalFunction::ErrorContextNew {
                    options: fixed_options.into_boxed_slice()
                }
            }
            CanonicalFunction::ErrorContextDebugMessage { options: options_orig } => {
                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(plan, ctx));
                }
                CanonicalFunction::ErrorContextDebugMessage {
                    options: fixed_options.into_boxed_slice()
                }
            }
            CanonicalFunction::ThreadSpawnRef { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = ctx.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::ThreadSpawnRef {
                    func_ty_index: new_tid as u32,
                }
            }
            CanonicalFunction::ThreadSpawnIndirect { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let table = refs.as_ref().unwrap().table();
                let new_tid = ctx.lookup_actual_id_or_panic(&ty);
                let new_tbl_id = ctx.lookup_actual_id_or_panic(&table);
                CanonicalFunction::ThreadSpawnIndirect {
                    func_ty_index: new_tid as u32,
                    table_index: new_tbl_id as u32,
                }
            }
            CanonicalFunction::ThreadNewIndirect { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let table = refs.as_ref().unwrap().table();
                let new_tid = ctx.lookup_actual_id_or_panic(&ty);
                let new_tbl_id = ctx.lookup_actual_id_or_panic(&table);
                CanonicalFunction::ThreadNewIndirect {
                    func_ty_index: new_tid as u32,
                    table_index: new_tbl_id as u32,
                }
            }
            CanonicalFunction::StreamDropReadable { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = ctx.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::StreamDropReadable {
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::StreamDropWritable { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = ctx.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::StreamDropWritable {
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::FutureDropReadable { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = ctx.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::FutureDropReadable {
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::FutureDropWritable { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = ctx.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::FutureDropWritable {
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::ThreadAvailableParallelism
            | CanonicalFunction::BackpressureSet
            | CanonicalFunction::WaitableSetNew
            | CanonicalFunction::WaitableSetDrop
            | CanonicalFunction::WaitableJoin
            | CanonicalFunction::SubtaskDrop
            | CanonicalFunction::TaskCancel
            | CanonicalFunction::SubtaskCancel { .. }
            | CanonicalFunction::ContextGet(_)
            | CanonicalFunction::ContextSet(_)
            | CanonicalFunction::BackpressureInc
            | CanonicalFunction::BackpressureDec
            | CanonicalFunction::ThreadYield { .. }
            | CanonicalFunction::ThreadIndex
            | CanonicalFunction::ThreadSwitchTo { .. }
            | CanonicalFunction::ThreadSuspend { .. }
            | CanonicalFunction::ThreadResumeLater
            | CanonicalFunction::ThreadYieldTo {..}
            | CanonicalFunction::ErrorContextDrop => self.clone(),
        }
    }
}

impl sealed::Sealed for Instance<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for Instance<'_> {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        match self {
            Instance::Instantiate { args: args_orig, .. } => {
                let refs = self.referenced_indices(Depth::default());
                let module = refs.as_ref().unwrap().module();
                let new_id = ctx.lookup_actual_id_or_panic(&module);

                let mut args = vec![];
                for arg in args_orig.iter() {
                    args.push(arg.fix(plan, ctx));
                }
                Instance::Instantiate {
                    module_index: new_id as u32,
                    args: args.into_boxed_slice()
                }
            }
            Instance::FromExports(exports_orig) => {
                // NOTE: We will not be fixing ALL indices here (complexity)
                let mut exports = vec![];
                for export in exports_orig.iter() {
                    exports.push(export.fix(plan, ctx));
                }
                Instance::FromExports(exports.into_boxed_slice())
            }
        }
    }
}

impl sealed::Sealed for ComponentStartFunction {}
#[rustfmt::skip]
impl FixIndicesImpl for ComponentStartFunction {
    fn fixme<'a>(&self, _: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        let refs = self.referenced_indices(Depth::default());
        let func = refs.as_ref().unwrap().func();
        let new_fid = ctx.lookup_actual_id_or_panic(&func);

        let mut new_args = vec![];
        for arg_refs in refs.as_ref().unwrap().others() {
            let ty = arg_refs.as_ref().unwrap().ty();
            let new_arg = ctx.lookup_actual_id_or_panic(&ty);
            new_args.push(new_arg as u32)
        }

        Self {
            func_index: new_fid as u32,
            arguments: new_args.into_boxed_slice(),
            results: self.results,
        }
    }
}

impl sealed::Sealed for CustomSection<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for CustomSection<'_> {
    fn fixme<'a>(&self, _: &Option<SubItemPlan>, _: &mut EncodeCtx) -> Self {
        self.clone()
    }
}

impl sealed::Sealed for ComponentDefinedType<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for ComponentDefinedType<'_> {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        match self {
            ComponentDefinedType::Flags(_)
            | ComponentDefinedType::Enum(_) => self.clone(),
            ComponentDefinedType::Primitive(ty) => ComponentDefinedType::Primitive(ty.fix(plan, ctx)),
            ComponentDefinedType::Record(tys) => {
                let mut new_tys = vec![];
                for (s, ty) in tys.iter() {
                    new_tys.push((*s, ty.fix(plan, ctx)))
                }
                ComponentDefinedType::Record(new_tys.into_boxed_slice())
            },
            ComponentDefinedType::Variant(tys) => {
                let mut new_tys = vec![];
                for ty in tys.iter() {
                    new_tys.push(ty.fix(plan, ctx))
                }
                ComponentDefinedType::Variant(new_tys.into_boxed_slice())
            },
            ComponentDefinedType::List(ty) => ComponentDefinedType::List(ty.fix(plan, ctx)),
            ComponentDefinedType::FixedSizeList(ty, len) => ComponentDefinedType::FixedSizeList(ty.fix(plan, ctx), *len),
            ComponentDefinedType::Tuple(tys) => {
                let mut new_tys = vec![];
                for t in tys.iter() {
                    new_tys.push(t.fix(plan, ctx))
                }
                ComponentDefinedType::Tuple(new_tys.into_boxed_slice())
            }
            ComponentDefinedType::Option(ty) => ComponentDefinedType::Option(ty.fix(plan, ctx)),
            ComponentDefinedType::Result { ok, err } => ComponentDefinedType::Result {
                ok: if let Some(ok) = ok {
                    Some(ok.fix(plan, ctx))
                } else {
                    None
                },
                err: if let Some(err) = err {
                    Some(err.fix(plan, ctx))
                } else {
                    None
                }
            },
            ComponentDefinedType::Own(_) => {
                let refs = self.referenced_indices(Depth::default());
                let ty = refs.as_ref().unwrap().ty();
                let id = ctx.lookup_actual_id_or_panic(&ty);
                ComponentDefinedType::Own(id as u32)
            },
            ComponentDefinedType::Borrow(_) => {
                let refs = self.referenced_indices(Depth::default());
                let ty = refs.as_ref().unwrap().ty();
                let id = ctx.lookup_actual_id_or_panic(&ty);
                ComponentDefinedType::Borrow(id as u32)
            },
            ComponentDefinedType::Future(ty) => ComponentDefinedType::Future(if let Some(ty) = ty {
                Some(ty.fix(plan, ctx))
            } else {
                None
            }),
            ComponentDefinedType::Stream(ty) => ComponentDefinedType::Stream(if let Some(ty) = ty {
                Some(ty.fix(plan, ctx))
            } else {
                None
            }),
        }
    }
}

impl sealed::Sealed for PrimitiveValType {}
#[rustfmt::skip]
impl FixIndicesImpl for PrimitiveValType {
    fn fixme<'a>(&self, _: &Option<SubItemPlan>, _: &mut EncodeCtx) -> Self {
        self.clone()
    }
}

impl sealed::Sealed for VariantCase<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for VariantCase<'_> {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        Self {
            name: self.name,
            ty: self.ty.map(|ty| ty.fix(plan, ctx)),
            refines: self.refines.map(|_| {
                let refs = self.referenced_indices(Depth::default());
                let ty = refs.as_ref().unwrap().misc();
                ctx.lookup_actual_id_or_panic(&ty) as u32
            }),
        }
    }
}

impl sealed::Sealed for ComponentFuncType<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for ComponentFuncType<'_> {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        let mut new_params = vec![];
        for (orig_name, orig_ty) in self.params.iter() {
            new_params.push((*orig_name, orig_ty.fix(plan, ctx)));
        }

        let new_res = if let Some(res) = self.result {
            Some(res.fix(plan, ctx))
        } else {
            None
        };

        Self {
            async_: self.async_,
            params: new_params.into_boxed_slice(),
            result: new_res,
        }
    }
}

impl sealed::Sealed for SubType {}
#[rustfmt::skip]
impl FixIndicesImpl for SubType {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        Self {
            is_final: self.is_final,
            supertype_idx: if let Some(_) = self.supertype_idx {
                let refs = self.referenced_indices(Depth::default());
                let ty = refs.as_ref().unwrap().ty();
                let new_id = ctx.lookup_actual_id_or_panic(&ty);
                Some(PackedIndex::from_module_index(new_id as u32).unwrap())
            } else {
                None
            },
            composite_type: self.composite_type.fix(plan, ctx)
        }
    }
}

impl sealed::Sealed for CompositeType {}
#[rustfmt::skip]
impl FixIndicesImpl for CompositeType {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        Self {
            inner: self.inner.fix(plan, ctx),
            shared: false,
            descriptor_idx: None,
            describes_idx: None,
        }
    }
}

impl sealed::Sealed for CompositeInnerType {}
#[rustfmt::skip]
impl FixIndicesImpl for CompositeInnerType {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        match self {
            CompositeInnerType::Func(ty) => CompositeInnerType::Func(ty.fix(plan, ctx)),
            CompositeInnerType::Array(ty) => CompositeInnerType::Array(ArrayType(ty.0.fix(plan, ctx))),
            CompositeInnerType::Struct(s) => CompositeInnerType::Struct(s.fix(plan, ctx)),
            CompositeInnerType::Cont(_) => {
                let refs = self.referenced_indices(Depth::default());
                let ty = refs.as_ref().unwrap().ty();
                let new_id = ctx.lookup_actual_id_or_panic(&ty);
                CompositeInnerType::Cont(ContType(PackedIndex::from_module_index(new_id as u32).unwrap()))
            },
        }
    }
}

impl sealed::Sealed for FuncType {}
#[rustfmt::skip]
impl FixIndicesImpl for FuncType {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        let mut new_params = vec![];
        for p in self.params() {
            new_params.push(p.fix(plan, ctx));
        }
        let mut new_results = vec![];
        for r in self.results() {
            new_results.push(r.fix(plan, ctx));
        }

        Self::new(new_params, new_results)
    }
}

impl sealed::Sealed for FieldType {}
#[rustfmt::skip]
impl FixIndicesImpl for FieldType {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        Self {
            element_type: self.element_type.fix(plan, ctx),
            mutable: self.mutable,
        }
    }
}

impl sealed::Sealed for StorageType {}
#[rustfmt::skip]
impl FixIndicesImpl for StorageType {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        match self {
            StorageType::I8
            | StorageType::I16 => self.clone(),
            StorageType::Val(value) => StorageType::Val(value.fix(plan, ctx))
        }
    }
}

impl sealed::Sealed for StructType {}
#[rustfmt::skip]
impl FixIndicesImpl for StructType {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        let mut new_fields = vec![];
        for f in self.fields.iter() {
            new_fields.push(f.fix(plan, ctx));
        }

        Self {
            fields: new_fields.into_boxed_slice()
        }
    }
}

impl sealed::Sealed for ComponentTypeDeclaration<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for ComponentTypeDeclaration<'_> {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        match self {
            ComponentTypeDeclaration::CoreType(ty) => ComponentTypeDeclaration::CoreType(ty.fix(plan, ctx)),
            ComponentTypeDeclaration::Type(ty) => ComponentTypeDeclaration::Type(ty.fix(plan, ctx)),
            ComponentTypeDeclaration::Alias(a) => ComponentTypeDeclaration::Alias(a.fix(plan, ctx)),
            ComponentTypeDeclaration::Import(i) => ComponentTypeDeclaration::Import(i.fix(plan, ctx)),
            ComponentTypeDeclaration::Export { name, ty } => ComponentTypeDeclaration::Export {
                name: name.clone(),
                ty: ty.fix(plan, ctx)
            },
        }
    }
}

impl sealed::Sealed for ValType {}
#[rustfmt::skip]
impl FixIndicesImpl for ValType {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        match self {
            ValType::I32
            | ValType::I64
            | ValType::F32
            | ValType::F64
            | ValType::V128 => self.clone(),
            ValType::Ref(r) => ValType::Ref(r.fix(plan, ctx)),
        }
    }
}

impl sealed::Sealed for RefType {}
#[rustfmt::skip]
impl FixIndicesImpl for RefType {
    fn fixme<'a>(&self, _: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        let refs = self.referenced_indices(Depth::default());
        let ty = refs.as_ref().unwrap().ty();
        let new_id = ctx.lookup_actual_id_or_panic(&ty);

        Self::new(self.is_nullable(), HeapType::Exact(UnpackedIndex::Module(new_id as u32))).unwrap()
    }
}

impl sealed::Sealed for CoreType<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for CoreType<'_> {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        println!("\t---> CoreType: {:p}", self);
        match &self {
            CoreType::Rec(recgroup) => {
                CoreType::Rec(recgroup.fix(plan, ctx))
            }
            CoreType::Module(module) => {
                let mut new_modules = vec![];
                for (idx, subplan) in plan.as_ref().unwrap().order().iter() {
                    let decl = &module[*idx];
                    new_modules.push(decl.fix(subplan, ctx));
                }

                CoreType::Module(new_modules.into_boxed_slice())
            }
        }
    }
}

impl sealed::Sealed for ModuleTypeDeclaration<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for ModuleTypeDeclaration<'_> {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        println!("\t---> ModuleTypeDeclaration: {:p}", self);
        match self {
            ModuleTypeDeclaration::Type(group) => ModuleTypeDeclaration::Type(group.fix(plan, ctx)),
            ModuleTypeDeclaration::Export { name, ty } => ModuleTypeDeclaration::Export {
                name,
                ty: ty.fix(plan, ctx)
            },
            ModuleTypeDeclaration::Import(import) => ModuleTypeDeclaration::Import(import.fix(plan, ctx)),
            // In the case of outer aliases, the u32 pair serves as a de Bruijn index, with first u32 being the number of enclosing components/modules to skip and the second u32 being an index into the target's sort's index space. In particular, the first u32 can be 0, in which case the outer alias refers to the current component. To maintain the acyclicity of module instantiation, outer aliases are only allowed to refer to preceding outer definitions.
            ModuleTypeDeclaration::OuterAlias { .. } => todo!(), // TODO: Fix this after scoped index spaces!
        }
    }
}

impl sealed::Sealed for Import<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for Import<'_> {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        Self {
            module: self.module,
            name: self.name,
            ty: self.ty.fix(plan, ctx),
        }
    }
}

impl sealed::Sealed for RecGroup {}
#[rustfmt::skip]
impl FixIndicesImpl for RecGroup {
    fn fixme<'a>(&self, _: &Option<SubItemPlan>, _: &mut EncodeCtx) -> Self {
        // This is kept as an opaque IR node (indices not fixed here)
        // This is because wasmparser does not allow library users to create
        // a new RecGroup.
        // Indices will be fixed in self.do_encode()!
        self.clone()
    }
}

impl sealed::Sealed for ComponentImport<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for ComponentImport<'_> {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        Self {
            name: self.name,
            ty: self.ty.fix(plan, ctx)
        }
    }
}

impl sealed::Sealed for ComponentValType {}
#[rustfmt::skip]
impl FixIndicesImpl for ComponentValType {
    fn fixme<'a>(&self, _: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        if let ComponentValType::Type(_) = self {
            let refs = self.referenced_indices(Depth::default());
            let ty = refs.as_ref().unwrap().ty();
            let new_id = ctx.lookup_actual_id_or_panic(&ty);
            ComponentValType::Type(new_id as u32)
        } else {
            self.clone()
        }
    }
}

impl sealed::Sealed for ComponentAlias<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for ComponentAlias<'_> {
    fn fixme<'a>(&self, _: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        match self {
            ComponentAlias::InstanceExport { kind, name, .. } => {
                let refs = self.referenced_indices(Depth::default());
                let inst = refs.as_ref().unwrap().inst();
                let new_id = ctx.lookup_actual_id_or_panic(&inst);
                Self::InstanceExport {
                    kind: kind.clone(),
                    name,
                    instance_index: new_id as u32,
                }
            }
            ComponentAlias::CoreInstanceExport { kind, name, .. } => {
                let refs = self.referenced_indices(Depth::default());
                let inst = refs.as_ref().unwrap().inst();
                let new_id = ctx.lookup_actual_id_or_panic(&inst);
                Self::CoreInstanceExport {
                    kind: kind.clone(),
                    name,
                    instance_index: new_id as u32,
                }
            }
            // In the case of outer aliases, the u32 pair serves as a de Bruijn index, with first u32 being the number of enclosing components/modules to skip and the second u32 being an index into the target's sort's index space. In particular, the first u32 can be 0, in which case the outer alias refers to the current component. To maintain the acyclicity of module instantiation, outer aliases are only allowed to refer to preceding outer definitions.
            ComponentAlias::Outer { .. } => self.clone(), // TODO: Fix this after scoped index spaces!
        }
    }
}

impl sealed::Sealed for ComponentTypeRef {}
#[rustfmt::skip]
impl FixIndicesImpl for ComponentTypeRef {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        let refs = self.referenced_indices(Depth::default());
        match self {
            ComponentTypeRef::Module(_) => {
                let ty = refs.as_ref().unwrap().ty();
                let new_id = ctx.lookup_actual_id_or_panic(&ty);
                ComponentTypeRef::Module(new_id as u32)
            }
            ComponentTypeRef::Value(ty) => {
                ComponentTypeRef::Value(ty.fix(plan, ctx))
            }
            ComponentTypeRef::Func(_) => {
                let ty = refs.as_ref().unwrap().ty();
                let new_id = ctx.lookup_actual_id_or_panic(&ty);
                ComponentTypeRef::Func(new_id as u32)
            }
            ComponentTypeRef::Instance(_) => {
                let ty = refs.as_ref().unwrap().ty();
                let new_id = ctx.lookup_actual_id_or_panic(&ty);
                ComponentTypeRef::Instance(new_id as u32)
            }
            ComponentTypeRef::Component(_) => {
                let ty = refs.as_ref().unwrap().ty();
                let new_id = ctx.lookup_actual_id_or_panic(&ty);
                ComponentTypeRef::Component(new_id as u32)
            }
            ComponentTypeRef::Type(_) => self.clone(), // nothing to do
        }
    }
}

impl sealed::Sealed for CanonicalOption {}
#[rustfmt::skip]
impl FixIndicesImpl for CanonicalOption {
    fn fixme<'a>(&self, _: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        let refs = self.referenced_indices(Depth::default());
        match self {
            CanonicalOption::Realloc(_)
            | CanonicalOption::PostReturn(_)
            | CanonicalOption::Callback(_) => {
                let func = refs.as_ref().unwrap().func();
                let new_fid = ctx.lookup_actual_id_or_panic(&func);
                match self {
                    CanonicalOption::Realloc(_) => CanonicalOption::Realloc(new_fid as u32),
                    CanonicalOption::PostReturn(_) => CanonicalOption::PostReturn(new_fid as u32),
                    CanonicalOption::Callback(_) => CanonicalOption::Callback(new_fid as u32),
                    _ => unreachable!(),
                }
            }
            CanonicalOption::CoreType(_) => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = ctx.lookup_actual_id_or_panic(&ty);
                CanonicalOption::CoreType(new_tid as u32)
            }

            CanonicalOption::Memory(_) => {
                let mem = refs.as_ref().unwrap().mem();
                let new_mid = ctx.lookup_actual_id_or_panic(&mem);
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

impl sealed::Sealed for InstantiationArg<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for InstantiationArg<'_> {
    fn fixme<'a>(&self, _: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        let refs = self.referenced_indices(Depth::default());
        let misc = refs.as_ref().unwrap().misc();
        let new_id = ctx.lookup_actual_id_or_panic(&misc);
        Self {
            name: self.name,
            kind: self.kind.clone(),
            index: new_id as u32,
        }
    }
}

impl sealed::Sealed for Export<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for Export<'_> {
    fn fixme<'a>(&self, _: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        let refs = self.referenced_indices(Depth::default());
        let misc = refs.as_ref().unwrap().misc();
        let new_id = ctx.lookup_actual_id_or_panic(&misc);
        Self {
            name: self.name,
            kind: self.kind.clone(),
            index: new_id as u32,
        }
    }
}

impl sealed::Sealed for InstanceTypeDeclaration<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for InstanceTypeDeclaration<'_> {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        println!("\t---> InstanceTypeDeclaration: {:p}", self);
        match self {
            InstanceTypeDeclaration::CoreType(core_type) => InstanceTypeDeclaration::CoreType(core_type.fix(plan, ctx)),
            InstanceTypeDeclaration::Type(ty) => InstanceTypeDeclaration::Type(ty.fix(plan, ctx)),
            InstanceTypeDeclaration::Alias(alias) => InstanceTypeDeclaration::Alias(alias.fix(plan, ctx)),
            InstanceTypeDeclaration::Export { name, ty } => InstanceTypeDeclaration::Export {
                name: name.clone(),
                ty: ty.fix(plan, ctx)
            },
        }
    }
}

impl sealed::Sealed for TypeRef {}
#[rustfmt::skip]
impl FixIndicesImpl for TypeRef {
    fn fixme<'a>(&self, _: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        let refs = self.referenced_indices(Depth::default());
        match self {
            TypeRef::Func(_) => {
                let ty = refs.as_ref().unwrap().ty();
                let new_id = ctx.lookup_actual_id_or_panic(&ty);
                TypeRef::Func(new_id as u32)
            }
            TypeRef::Tag(TagType { kind, .. }) => {
                let ty = refs.as_ref().unwrap().ty();
                let new_id = ctx.lookup_actual_id_or_panic(&ty);
                TypeRef::Tag(TagType {
                    kind: kind.clone(),
                    func_type_idx: new_id as u32,
                })
            }
            TypeRef::FuncExact(_) => {
                let ty = refs.as_ref().unwrap().ty();
                let new_id = ctx.lookup_actual_id_or_panic(&ty);
                TypeRef::FuncExact(new_id as u32)
            }
            TypeRef::Table(_) | TypeRef::Memory(_) | TypeRef::Global(_) => self.clone(),
        }
    }
}
