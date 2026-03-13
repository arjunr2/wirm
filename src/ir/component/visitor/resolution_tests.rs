//! Resolution-correctness tests for `walk_structural` and `walk_topological`.
//!
//! These tests exercise the resolution layer (`cx.resolve`,
//! `cx.enter_comp_ty_scope`, `cx.enter_core_ty_scope`) rather than event
//! generation.  They exist to catch:
//!
//!   - `comp_at` depth/scope-stack misalignment inside type scopes
//!     (`type_scope_nesting` accounting).
//!   - Missing `ScopedVisitCtx` for refs that live in type-body subvecs
//!     ("No index for assumed ID" panics).
//!   - `type_scope_nesting` failing to reset correctly after scope exit.
//!
//! **Layer 1** — paranoid "resolve everything" tests: run a visitor that calls
//! `scoped.resolve()` for every ref on every type-body decl.  Resolution bugs
//! surface as panics, catching regressions without requiring knowledge of
//! exact IDs.
//!
//! **Layer 2** — specific result assertions: small, targeted WAT components
//! where the resolved `ResolvedItem` variant is checked precisely.

use crate::ir::component::refs::{RefKind, ReferencedIndices};
use crate::ir::component::visitor::{
    walk_structural, walk_topological, ComponentVisitor, ResolvedItem, ScopedVisitCtx,
};
use crate::Component;
use wasmparser::{
    ComponentType, ComponentTypeDeclaration, CoreType, InstanceTypeDeclaration,
    ModuleTypeDeclaration,
};

// ============================================================
// Paranoid visitor
// ============================================================

/// Visits every type-body declaration and resolves all its refs using the
/// `ScopedVisitCtx` the driver now provides directly.  Any resolution bug
/// (comp_at overflow, wrong nesting count, …) surfaces as a panic.
#[derive(Default)]
struct ParanoidVisitor {
    resolved_count: usize,
}

impl<'a> ComponentVisitor<'a> for ParanoidVisitor {
    fn visit_inst_type_decl(
        &mut self,
        cx: &ScopedVisitCtx<'a>,
        _decl_idx: usize,
        _id: u32,
        _parent: &ComponentType<'a>,
        decl: &InstanceTypeDeclaration<'a>,
    ) {
        for r in decl.referenced_indices() {
            let _ = cx.resolve(&r.ref_);
            self.resolved_count += 1;
        }
    }

    fn visit_comp_type_decl(
        &mut self,
        cx: &ScopedVisitCtx<'a>,
        _decl_idx: usize,
        _id: u32,
        _parent: &ComponentType<'a>,
        decl: &ComponentTypeDeclaration<'a>,
    ) {
        for r in decl.referenced_indices() {
            let _ = cx.resolve(&r.ref_);
            self.resolved_count += 1;
        }
    }

    fn visit_module_type_decl(
        &mut self,
        cx: &ScopedVisitCtx<'a>,
        _decl_idx: usize,
        _id: u32,
        _parent: &CoreType<'a>,
        decl: &ModuleTypeDeclaration<'a>,
    ) {
        for r in decl.referenced_indices() {
            let _ = cx.resolve(&r.ref_);
            self.resolved_count += 1;
        }
    }
}

/// Run the paranoid visitor under both walkers and assert they resolve the
/// same number of refs (a consistency check on top of the no-panic check).
/// Returns the resolved ref count.
fn run_paranoid(wat: &str) -> usize {
    let bytes = wat::parse_str(wat).expect("WAT parse failed");
    let comp = Component::parse(&bytes, false, false).expect("component parse failed");

    let mut structural = ParanoidVisitor::default();
    walk_structural(&comp, &mut structural);

    let mut topological = ParanoidVisitor::default();
    walk_topological(&comp, &mut topological);

    assert_eq!(
        structural.resolved_count, topological.resolved_count,
        "structural and topological walks resolved a different number of refs"
    );
    structural.resolved_count
}

fn run_paranoid_file(path: &str) {
    let bytes = wat::parse_file(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let comp = Component::parse(&bytes, false, false).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut v1 = ParanoidVisitor::default();
    walk_structural(&comp, &mut v1);
    let mut v2 = ParanoidVisitor::default();
    walk_topological(&comp, &mut v2);
    assert_eq!(
        v1.resolved_count, v2.resolved_count,
        "{path}: structural/topological ref count mismatch"
    );
}

// ============================================================
// Layer 2 helpers
// ============================================================

/// Parse `wat` and run `walk_structural` with `visitor`.
fn walk_wat<V: for<'a> ComponentVisitor<'a>>(wat: &str, visitor: &mut V) {
    let bytes = wat::parse_str(wat).expect("WAT parse failed");
    let comp = Component::parse(&bytes, false, false).expect("component parse failed");
    walk_structural(&comp, visitor);
}

/// Resolve every ref in `refs` using `cx`, assert each satisfies `pred`, return the count.
///
/// Called from within a `ComponentVisitor<'a>` impl where `'a` is already concrete,
/// so no higher-ranked lifetime bound is needed here.
fn check_refs<'a>(
    cx: &ScopedVisitCtx<'a>,
    refs: Vec<RefKind>,
    pred: impl Fn(&ResolvedItem<'a, 'a>) -> bool,
    msg: &'static str,
) -> usize {
    let mut n = 0;
    for r in refs {
        let resolved = cx.resolve(&r.ref_);
        assert!(pred(&resolved), "{msg}: got {resolved:?}");
        n += 1;
    }
    n
}

// ============================================================
// Layer 1 — regression net over existing fixture files
// ============================================================

/// Runs the paranoid visitor over all handwritten fixture components to catch
/// regressions.  These files exercise a variety of component model features
/// that were previously only tested implicitly through encoding round-trips.
#[test]
fn test_scoped_resolution_no_panic_fixture_files() {
    let fixtures = [
        "./tests/test_inputs/handwritten/components/add.wat",
        "./tests/test_inputs/handwritten/components/mul_mod.wat",
        "./tests/test_inputs/dfinity/components/exports.wat",
        "./tests/test_inputs/dfinity/components/func.wat",
        "./tests/test_inputs/dfinity/components/func_locals.wat",
        "./tests/test_inputs/spin/hello_world.wat",
    ];
    for path in fixtures {
        run_paranoid_file(path);
    }
}

// ============================================================
// Layer 1 — targeted WATs for specific bug scenarios
// ============================================================

/// Instance type body with a type and an export that references it.
///
/// Previously panicked with "No index for assumed ID" because `cx.resolve()`
/// was called without entering the type scope first.
#[test]
fn test_scoped_resolution_instance_type_body() {
    let count = run_paranoid(
        r#"(component
          (type (instance
            (type $t u32)
            (export "n" (type (eq $t)))
          ))
        )"#,
    );
    // The export has one type ref — at least 1 resolution must happen.
    assert!(count > 0);
}

/// Instance type body with a compound type (list) that references a sibling
/// type in the same scope.
#[test]
fn test_scoped_resolution_instance_type_compound_ref() {
    let count = run_paranoid(
        r#"(component
          (type (instance
            (type $item u8)
            (type $list (list $item))
            (export "data" (type (eq $list)))
          ))
        )"#,
    );
    assert!(count > 0);
}

/// Component type body (not instance type) with a type and an export ref.
/// Exercises `enter_comp_ty_scope` for `ComponentType::Component`.
#[test]
fn test_scoped_resolution_component_type_body() {
    let count = run_paranoid(
        r#"(component
          (type (component
            (type $t u32)
            (export "value" (type (eq $t)))
          ))
        )"#,
    );
    assert!(count > 0);
}

/// `CoreType::Module` body with a func type declaration and an import that
/// references it.  Exercises `enter_core_ty_scope` and
/// `resolve_maybe_from_subvec` for `ModuleTypeDeclaration`.
#[test]
fn test_scoped_resolution_core_module_type_body() {
    let count = run_paranoid(
        r#"(component
          (core type (module
            (type (func (param i32) (result i64)))
            (import "env" "compute" (func (type 0)))
            (export "result" (func (type 0)))
          ))
        )"#,
    );
    assert!(count > 0);
}

/// Outer alias (depth > 0) inside an instance type body.
///
/// This is the exact pattern that triggered the `comp_at` subtraction
/// overflow before `type_scope_nesting` was introduced.  With one type-scope
/// level on the scope stack but zero on the component stack, resolving a
/// depth=1 ref into `component_stack[len - 1 - 1]` used to underflow.
#[test]
fn test_scoped_resolution_outer_alias_in_instance_type() {
    let count = run_paranoid(
        r#"(component
          (type $outer u32)
          (type (instance
            (alias outer 1 0 (type))
            (export "n" (type (eq 0)))
          ))
        )"#,
    );
    assert!(count > 0);
}

/// Two consecutive instance type scopes.
///
/// Verifies that `type_scope_nesting` resets to zero after the first scope
/// exits, so the second scope's resolution is not affected by the first.
#[test]
fn test_scoped_resolution_nesting_resets_between_scopes() {
    run_paranoid(
        r#"(component
          (type (instance
            (type $a u32)
            (export "a" (type (eq $a)))
          ))
          (type (instance
            (type $b string)
            (export "b" (type (eq $b)))
          ))
        )"#,
    );
}

/// Instance type body that itself contains an inner instance type.
///
/// The outer export references the inner instance type, which is itself a
/// complete type body.  Verifies that nested scoped contexts are handled
/// correctly.
#[test]
fn test_scoped_resolution_nested_instance_types() {
    let count = run_paranoid(
        r#"(component
          (type (instance
            (type $inner (instance
              (type $t u8)
              (export "v" (type (eq $t)))
            ))
            (export "inner" (type (eq $inner)))
          ))
        )"#,
    );
    assert!(count > 0);
}

// ============================================================
// Layer 2 — specific resolved-item variant assertions
// ============================================================

/// An export's type ref inside an instance type body (pointing at a sibling
/// type declaration in the same scope) must resolve to `ResolvedItem::CompType`.
#[test]
fn test_resolve_result_instance_type_export_ref_is_comp_type() {
    struct V {
        checked: usize,
    }
    impl<'a> ComponentVisitor<'a> for V {
        fn visit_inst_type_decl(
            &mut self,
            cx: &ScopedVisitCtx<'a>,
            _: usize,
            _: u32,
            _: &ComponentType<'a>,
            decl: &InstanceTypeDeclaration<'a>,
        ) {
            if !matches!(decl, InstanceTypeDeclaration::Export { .. }) {
                return;
            }
            self.checked += check_refs(
                cx,
                decl.referenced_indices(),
                |r| matches!(r, ResolvedItem::CompType(..)),
                "export type ref in instance body should resolve to CompType",
            );
        }
    }
    let mut v = V { checked: 0 };
    walk_wat(
        r#"(component (type (instance (type $t u32) (export "n" (type (eq $t))))))"#,
        &mut v,
    );
    assert_eq!(
        v.checked, 1,
        "expected exactly 1 export type ref to be resolved"
    );
}

/// An export's type ref inside a `ComponentType::Component` body must also
/// resolve to `ResolvedItem::CompType`.
#[test]
fn test_resolve_result_component_type_export_ref_is_comp_type() {
    struct V {
        checked: usize,
    }
    impl<'a> ComponentVisitor<'a> for V {
        fn visit_comp_type_decl(
            &mut self,
            cx: &ScopedVisitCtx<'a>,
            _: usize,
            _: u32,
            _: &ComponentType<'a>,
            decl: &ComponentTypeDeclaration<'a>,
        ) {
            if !matches!(decl, ComponentTypeDeclaration::Export { .. }) {
                return;
            }
            self.checked += check_refs(
                cx,
                decl.referenced_indices(),
                |r| matches!(r, ResolvedItem::CompType(..)),
                "export type ref in component body should resolve to CompType",
            );
        }
    }
    let mut v = V { checked: 0 };
    walk_wat(
        r#"(component (type (component (type $t u32) (export "value" (type (eq $t))))))"#,
        &mut v,
    );
    assert_eq!(
        v.checked, 1,
        "expected exactly 1 export type ref to be resolved"
    );
}

/// A func import's type ref inside a `CoreType::Module` body (pointing at a
/// sibling `Type` declaration in the same scope) must resolve to
/// `ResolvedItem::ModuleTyDecl`.
#[test]
fn test_resolve_result_core_module_import_ref_is_module_ty_decl() {
    struct V {
        checked: usize,
    }
    impl<'a> ComponentVisitor<'a> for V {
        fn visit_module_type_decl(
            &mut self,
            cx: &ScopedVisitCtx<'a>,
            _: usize,
            _: u32,
            _: &CoreType<'a>,
            decl: &ModuleTypeDeclaration<'a>,
        ) {
            if !matches!(decl, ModuleTypeDeclaration::Import(..)) {
                return;
            }
            self.checked += check_refs(
                cx,
                decl.referenced_indices(),
                |r| matches!(r, ResolvedItem::ModuleTyDecl(..)),
                "import type ref in module type body should resolve to ModuleTyDecl",
            );
        }
    }
    let mut v = V { checked: 0 };
    walk_wat(
        r#"(component (core type (module
          (type (func (param i32) (result i64)))
          (import "env" "compute" (func (type 0)))
        )))"#,
        &mut v,
    );
    assert_eq!(
        v.checked, 1,
        "expected exactly 1 import type ref to be resolved"
    );
}

// ============================================================
// Layer 3 — exact resolved index assertions
// ============================================================

/// The only type in an instance type scope is at index 0; the export's ref
/// must resolve to `CompType(0, _)`.
#[test]
fn test_resolve_index_single_type_in_instance_scope() {
    struct V {
        checked: usize,
    }
    impl<'a> ComponentVisitor<'a> for V {
        fn visit_inst_type_decl(
            &mut self,
            cx: &ScopedVisitCtx<'a>,
            _: usize,
            _: u32,
            _: &ComponentType<'a>,
            decl: &InstanceTypeDeclaration<'a>,
        ) {
            if !matches!(decl, InstanceTypeDeclaration::Export { .. }) {
                return;
            }
            self.checked += check_refs(
                cx,
                decl.referenced_indices(),
                |r| matches!(r, ResolvedItem::CompType(0, _)),
                "sole type in instance scope must resolve to index 0",
            );
        }
    }
    let mut v = V { checked: 0 };
    walk_wat(
        r#"(component (type (instance (type $t u32) (export "n" (type (eq $t))))))"#,
        &mut v,
    );
    assert_eq!(v.checked, 1);
}

/// When two types exist in an instance scope, an export referencing the
/// second must resolve to `CompType(1, _)`.
#[test]
fn test_resolve_index_second_type_in_instance_scope() {
    struct V {
        checked: usize,
    }
    impl<'a> ComponentVisitor<'a> for V {
        fn visit_inst_type_decl(
            &mut self,
            cx: &ScopedVisitCtx<'a>,
            _: usize,
            _: u32,
            _: &ComponentType<'a>,
            decl: &InstanceTypeDeclaration<'a>,
        ) {
            if !matches!(decl, InstanceTypeDeclaration::Export { .. }) {
                return;
            }
            self.checked += check_refs(
                cx,
                decl.referenced_indices(),
                |r| matches!(r, ResolvedItem::CompType(1, _)),
                "export referencing second type must resolve to index 1",
            );
        }
    }
    let mut v = V { checked: 0 };
    walk_wat(
        r#"(component (type (instance
          (type $a u8)        (;; index 0 ;)
          (type $b string)    (;; index 1 ;)
          (export "x" (type (eq $b)))
        )))"#,
        &mut v,
    );
    assert_eq!(v.checked, 1);
}

/// An outer alias pointing at the *second* type in the enclosing component
/// scope must resolve to `CompType(1, _)` — verifying that `comp_at` walks
/// to the right outer scope AND picks the right index within it.
#[test]
fn test_resolve_index_outer_alias_to_second_outer_type() {
    struct V {
        checked: usize,
    }
    impl<'a> ComponentVisitor<'a> for V {
        fn visit_inst_type_decl(
            &mut self,
            cx: &ScopedVisitCtx<'a>,
            _: usize,
            _: u32,
            _: &ComponentType<'a>,
            decl: &InstanceTypeDeclaration<'a>,
        ) {
            if !matches!(decl, InstanceTypeDeclaration::Alias(..)) {
                return;
            }
            self.checked += check_refs(
                cx,
                decl.referenced_indices(),
                |r| matches!(r, ResolvedItem::CompType(1, _)),
                "alias to second outer type must resolve to index 1",
            );
        }
    }
    let mut v = V { checked: 0 };
    walk_wat(
        r#"(component
          (type $first u8)   (;; outer index 0 ;)
          (type $second u32) (;; outer index 1 ;)
          (type (instance
            (alias outer 1 1 (type)) (;; depth=1, index=1 → $second ;)
            (export "n" (type (eq 0)))
          ))
        )"#,
        &mut v,
    );
    assert_eq!(v.checked, 1);
}

/// In a `CoreType::Module` body with two func types, an import referencing
/// the second func type must resolve to `ModuleTyDecl(1, _)`.
#[test]
fn test_resolve_index_module_type_import_refs_second_func_type() {
    struct V {
        checked: usize,
    }
    impl<'a> ComponentVisitor<'a> for V {
        fn visit_module_type_decl(
            &mut self,
            cx: &ScopedVisitCtx<'a>,
            _: usize,
            _: u32,
            _: &CoreType<'a>,
            decl: &ModuleTypeDeclaration<'a>,
        ) {
            if !matches!(decl, ModuleTypeDeclaration::Import(..)) {
                return;
            }
            self.checked += check_refs(
                cx,
                decl.referenced_indices(),
                |r| matches!(r, ResolvedItem::ModuleTyDecl(1, _)),
                "import referencing second func type must resolve to index 1",
            );
        }
    }
    let mut v = V { checked: 0 };
    walk_wat(
        r#"(component (core type (module
          (type (func (param i32)))            (;; index 0 ;)
          (type (func (param i32) (result i64))) (;; index 1 ;)
          (import "env" "f" (func (type 1)))
        )))"#,
        &mut v,
    );
    assert_eq!(v.checked, 1);
}

/// An outer alias inside an instance type body resolves via the outer
/// component scope, not the type scope.  The resolved item must match the
/// type declared in the enclosing component.
#[test]
fn test_resolve_result_outer_alias_in_instance_type_resolves_to_comp_type() {
    struct V {
        checked: usize,
    }
    impl<'a> ComponentVisitor<'a> for V {
        fn visit_inst_type_decl(
            &mut self,
            cx: &ScopedVisitCtx<'a>,
            _: usize,
            _: u32,
            _: &ComponentType<'a>,
            decl: &InstanceTypeDeclaration<'a>,
        ) {
            if !matches!(decl, InstanceTypeDeclaration::Alias(..)) {
                return;
            }
            self.checked += check_refs(cx, decl.referenced_indices(),
                |r| matches!(r, ResolvedItem::CompType(..)),
                "outer alias in instance type should resolve to CompType in the outer component scope");
        }
    }
    let mut v = V { checked: 0 };
    walk_wat(
        r#"(component
          (type $outer u32)
          (type (instance (alias outer 1 0 (type)) (export "n" (type (eq 0)))))
        )"#,
        &mut v,
    );
    assert_eq!(
        v.checked, 1,
        "expected exactly 1 outer alias ref to be resolved"
    );
}
