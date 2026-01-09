use crate::encode::component::assign::assign_indices;
use crate::encode::component::collect::CollectCtx;
use crate::encode::component::encode::encode_internal;
use crate::Component;

mod assign;
mod collect;
pub(crate) mod encode;
mod fix_indices;
// mod encode_bk;

/// Encode this IR into a WebAssembly binary.
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
///
/// ## Index resolution behavior
///
/// During encoding, this function performs **basic index collection and
/// reindexing** within a single, well-defined index space. This includes
/// adjusting indices to account for:
///
/// - Newly inserted items
/// - Removed or reordered items
/// - Flat, non-nested index spaces
///
/// However, the encoder **does not attempt to resolve or rewrite indices whose
/// meaning depends on nested index-space scopes**, such as those introduced by
/// the WebAssembly component model.
///
/// In particular, indices whose correctness depends on entering or exiting
/// nested scopes are assumed to already be valid in the context where they
/// appear.
///
/// ### Examples
///
/// #### Supported: flat reindexing within a single scope
///
/// If instrumentation inserts a new component function before an existing one:
///
/// ```wat
/// ;; Original
/// (component
///   (func $f0)
///   (func $f1)
/// )
///
/// ;; After instrumentation
/// (component
///   (func $new)
///   (func $f0)
///   (func $f1)
/// )
/// ```
///
/// The encoder will automatically reindex references to `$f0` and `$f1` to
/// account for the inserted function.
///
/// #### Not supported: scope-dependent index resolution
///
/// ```wat
/// (component
///   (component $inner
///     (func $f)
///   )
///   (instance $i (instantiate $inner))
///   (export "f" (func $i "f"))
/// )
/// ```
///
/// In this example, the meaning of the exported function index depends on:
///
/// - Which component is instantiated
/// - Which instance scope is active
///
/// The encoder does **not** attempt to determine or rewrite such indices.
/// The IR is assumed to already reference the correct function in the correct
/// scope.
///
/// #### Not supported: nested core/module scopes
///
/// ```wat
/// (component
///   (component
///     (core module
///       (func $f)
///     )
///   )
///   (canon lift (core func $f))
/// )
/// ```
///
/// If `$f` is defined inside a nested core module scope, the encoder assumes
/// that any reference to it already uses the correct index for that scope.
///
/// ### Summary
///
/// - Flat, single-scope reindexing is handled automatically.
/// - Nested or scope-dependent index resolution is not.
/// - Earlier phases are responsible for ensuring scope-correct indices.
///
/// This design keeps encoding deterministic and avoids implicit cross-scope
/// rewriting that would be difficult to reason about or validate.
pub fn encode(comp: &Component) -> Vec<u8> {
    // Phase 1: Collect
    let mut ctx = CollectCtx::new(comp);
    comp.collect_root(&mut ctx);
    let mut plan = ctx.plan;
    let mut indices = ctx.indices;

    // Phase 2: Assign indices
    indices.reset_ids();
    assign_indices(&mut plan, &mut indices);

    // Phase 3: Encode (pass in the root-level component's plan, assigned indices, and original->new index map)
    let bytes = encode_internal(&comp, &plan, &indices);
    bytes.finish()
}
