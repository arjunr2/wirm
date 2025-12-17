mod collect;
mod assign;
mod encode;

/// Encoding a component gets split into 3 phases (the first two are for planning, the final
/// phase is to actually perform the encoding)
/// 1. Collect phase
///  - Walk dependencies
///  - Deduplicate using Seen
///  - Produce topologically sorted plan
/// 2. Index assignment phase
///  - Assign sequential indices after collection
///  - Separate from bytes
/// 3. Encoding phase
///  - Emit bytes using indices
///  - No recursion needed, all references are guaranteed to be valid
pub fn encode() {
    // // Phase 1: Collect
    // let mut ctx = CollectCtx { plan: EncodePlan::default(), seen: Seen::default() };
    // for func in &component.funcs {
    //     ctx.collect_func(func, &all_funcs);
    // }
    // let plan = ctx.plan;
    //
    // // Phase 2: Assign indices
    // let indices = assign_indices(&plan);
    //
    // // Phase 3: Encode
    // let bytes = encode(&plan, &indices);
    // println!("{}", String::from_utf8(bytes).unwrap());
}