use std::collections::{HashSet, HashMap};
use wasmparser::CanonicalFunction;
use crate::Component;

pub(crate) enum EncodeItem<'a> {
    // Type(&'a TypeDef),
    CanonicalFunc(&'a CanonicalFunction),
    // Module(&'a Module<'a>),
    // Component(&'a Component<'a>),
    // ... add others as needed
}


#[derive(Default)]
pub(crate) struct EncodePlan<'a> {
    pub(crate) items: Vec<EncodeItem<'a>>,
}

#[derive(Default)]
struct Seen {
    /// Points to a TEMPORARY ID -- this is just for bookkeeping, not the final ID
    /// The final ID is assigned during the "Assign" phase.
    canon_funcs: HashMap<*const CanonicalFunction, u32>,
}

struct CollectCtx<'a> {
    plan: EncodePlan<'a>,
    seen: Seen,
}

/// A trait for each IR node to implement --> The node knows how to `collect` itself.
trait Collect {
    fn collect(&mut self, ctx: &mut CollectCtx<'_>);
}

impl Collect for Component<'_> {
    fn collect(&mut self, ctx: &mut CollectCtx<'_>) {
        // let ptr = comp as *const _;
        // if self.seen.components.contains(&ptr) {
        //     return;
        // }
        // self.seen.components.insert(ptr);

        // traverse the IR in the order items appear
        // for item in &comp.items {
        //     match item {
        //         ComponentItem::Type(ty) => {
        //             self.plan.items.push(EncodeItem::Type(ty));
        //         }
        //         ComponentItem::CanonicalFunc(func) => {
        //             self.collect_func(func);
        //             self.plan.items.push(EncodeItem::CanonicalFunc(func));
        //         }
        //         ComponentItem::Module(module) => {
        //             self.collect_module(module);
        //             self.plan.items.push(EncodeItem::Module(module));
        //         }
        //         ComponentItem::Component(sub) => {
        //             self.collect_component(sub);
        //             self.plan.items.push(EncodeItem::Component(sub));
        //         }
        //     }
        // }
        todo!()
    }
}

impl Collect for CanonicalFunction {
    fn collect(&mut self, ctx: &mut CollectCtx<'_>) {
        let ptr = self as *const _;
        if ctx.seen.canon_funcs.contains_key(&ptr) {
            return;
        }

        // TODO: collect dependencies first
        // for dep in func.deps() { // assume you have a way to get dependent funcs
        //     ctx.collect_func(dep, ctx);
        // }

        // assign a temporary index during collection
        let idx = ctx.plan.items.len() as u32;
        ctx.seen.canon_funcs.insert(ptr, idx);

        // TODO: push to ordered plan
        // ctx.plan.items.push(func);
        todo!()
    }
}
