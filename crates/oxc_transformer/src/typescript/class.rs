use oxc_allocator::TakeIn;
use oxc_allocator::Vec as ArenaVec;
use oxc_ast::ast::*;
use oxc_semantic::ScopeFlags;
use oxc_span::SPAN;
use oxc_traverse::{BoundIdentifier, TraverseCtx};

use crate::utils::ast_builder::{
    create_class_constructor, create_this_property_access, create_this_property_assignment,
};

use super::TypeScript;

impl<'a> TypeScript<'a, '_> {
    /// Transform class fields, and constructor parameters that includes modifiers into `this` assignments.
    ///
    /// This transformation is doing 2 things:
    ///
    /// 1. Convert constructor parameters that include modifier to `this` assignments and insert them
    ///    after the super call in the constructor body.
    /// Input:
    /// ```ts
    /// class C {
    ///   constructor(public x, private y) {}
    /// }
    /// ```
    ///
    /// Output:
    /// ```js
    /// class C {
    ///  constructor(x, y) {
    ///   this.x = x;
    ///   this.y = y;
    /// }
    /// ```
    ///
    /// 2. Convert class fields to `this` assignments in the constructor body.
    ///
    /// > This transformation only works when set_public_class_fields is true and the fields have initializers,
    ///   which is to align with the behavior of TypeScript's useDefineForClassFields: false option.
    ///
    /// Input:
    /// ```ts
    /// class C {
    ///   x = 1;
    ///   [y] = 2;
    /// }
    /// ```
    ///
    /// Output:
    /// ```js
    /// let _y;
    /// class C {
    ///   static {
    ///     _y = y;
    ///   }
    ///   constructor() {
    ///     this.x = 1;
    ///     this[_y] = 2;
    ///   }
    /// }
    /// ```
    ///
    // The computed key transformation behavior is the same as `TypeScript`, computed key assignments are
    // inserted into a static block rather than Babel that inserts them before class. We follow `TypeScript` just for
    // simplicity because Babel handles class expression and class declaration differently, which quite troublesome to
    // implement. Anyway, `TypeScript` is the source of truth for the typescript transformation.
    //
    // TODO: Not handling static property yet. Might no need to handle it because static property wouldn't affect the
    //       constructor body, which means never breaks https://github.com/oxc-project/oxc/issues/9192. And the
    //       `class-properties` plugin has covered it.
    pub(super) fn transform_class(&self, class: &mut Class<'a>, ctx: &mut TraverseCtx<'a>) {
        let mut constructor = None;
        let mut property_assignments = Vec::new();
        let mut computed_key_assignments = ctx.ast.vec();
        for element in &mut class.body.body {
            match element {
                ClassElement::PropertyDefinition(prop)
                    if self.ctx.assumptions.set_public_class_fields
                        && !prop.r#static
                        && prop.value.is_some() =>
                {
                    property_assignments.push(self.convert_property_definition(
                        prop,
                        &mut computed_key_assignments,
                        ctx,
                    ));
                }
                ClassElement::MethodDefinition(method) => {
                    if method.kind == MethodDefinitionKind::Constructor {
                        constructor = Some(&mut method.value);
                    }
                }
                _ => (),
            }
        }

        let computed_key_assignment_static_block =
            (!computed_key_assignments.is_empty()).then(|| {
                let scope_id = ctx.create_child_scope_of_current(ScopeFlags::ClassStaticBlock);
                ctx.ast.class_element_static_block_with_scope_id(
                    SPAN,
                    computed_key_assignments,
                    scope_id,
                )
            });

        if let Some(constructor) = constructor {
            let params = &constructor.params.items;

            // Transform constructor parameters that include modifier to `this` assignments.
            let param_assignments = params
                .iter()
                .filter_map(|param| {
                    if param.has_modifier() {
                        param.pattern.get_binding_identifier().map(|id| {
                            let target = create_this_property_assignment(id.span, &id.name, ctx);
                            Self::create_assignment(
                                target,
                                BoundIdentifier::from_binding_ident(id).create_read_expression(ctx),
                                ctx,
                            )
                        })
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();

            // Exits if there are no property or parameter assignments
            if property_assignments.is_empty() && param_assignments.is_empty() {
                return;
            }

            let constructor_body = constructor.body.as_mut().unwrap();

            // Find the position of the super call in the constructor body,
            // don't need to care about nested super call because `TypeScript`
            // doesn't allow it.
            let super_call_position = constructor_body
                .statements
                .iter()
                .position(|stmt| {
                    matches!(stmt, Statement::ExpressionStatement(stmt)
                        if stmt.expression.is_super_call_expression())
                })
                .map_or(0, |pos| pos + 1);

            // Insert the assignments after the super call
            constructor_body.statements.splice(
                super_call_position..super_call_position,
                param_assignments.into_iter().chain(property_assignments),
            );

            // Insert the static block after the constructor if there is a constructor.
            if let Some(element) = computed_key_assignment_static_block {
                class.body.body.insert(0, element);
            }
        } else if !property_assignments.is_empty() {
            // If there is no constructor, we need to create a default constructor
            // that initializes the public fields.
            let scope_id =
                ctx.create_child_scope_of_current(ScopeFlags::Function | ScopeFlags::Constructor);
            let ctor = create_class_constructor(
                property_assignments,
                class.super_class.is_some(),
                scope_id,
                ctx,
            );

            // Insert the static block at the beginning of the class body if there is no constructor.
            if let Some(element) = computed_key_assignment_static_block {
                class.body.body.splice(0..0, [ctor, element]);
            } else {
                // TODO(improve-on-babel): Could push constructor onto end of elements, instead of inserting as first
                class.body.body.insert(0, ctor);
            }
        }
    }

    pub(super) fn transform_class_on_exit(
        &self,
        class: &mut Class<'a>,
        _ctx: &mut TraverseCtx<'a>,
    ) {
        if !self.remove_class_fields_without_initializer {
            return;
        }

        class.body.body.retain(|element| {
            if let ClassElement::PropertyDefinition(prop) = element {
                if prop.value.is_none() {
                    return false;
                }
            }
            true
        });
    }

    /// Convert property definition to `this` assignment in constructor.
    ///
    /// * Computed key:
    ///   `class C { [x()] = 1; }` -> `let _x; class C { static { _x = x(); } constructor() { this[_x] = 1; } }`
    /// * Static key:
    ///  `class C { x = 1; }` -> `class C { constructor() { this.x = 1; } }`
    ///
    /// Returns an assignment statement which would be inserted in the constructor body.
    fn convert_property_definition(
        &self,
        prop: &mut PropertyDefinition<'a>,
        computed_key_assignments: &mut ArenaVec<Statement<'a>>,
        ctx: &mut TraverseCtx<'a>,
    ) -> Statement<'a> {
        let member = match &mut prop.key {
            PropertyKey::StaticIdentifier(ident) => {
                create_this_property_access(SPAN, &ident.name, ctx)
            }
            PropertyKey::PrivateIdentifier(_) => {
                // Handled in `convert_instance_property` and `convert_static_property`
                unreachable!();
            }
            key @ match_expression!(PropertyKey) => {
                let key = key.to_expression_mut();
                // Note: Key can also be static `StringLiteral` or `NumericLiteral`.
                // `class C { 'x' = true; 123 = false; }`
                // No temp var is created for these.
                // TODO: Any other possible static key types?

                let new_key = if self.ctx.key_needs_temp_var(key, ctx) {
                    let (assignment, ident) =
                        self.ctx.create_computed_key_temp_var(key.take_in(ctx.ast.allocator), ctx);
                    let assignment = ctx.ast.statement_expression(SPAN, assignment);
                    computed_key_assignments.push(assignment);
                    ident
                } else {
                    key.take_in(ctx.ast.allocator)
                };

                ctx.ast.member_expression_computed(
                    SPAN,
                    ctx.ast.expression_this(SPAN),
                    new_key,
                    false,
                )
            }
        };
        let target = AssignmentTarget::from(member);

        // Has checked that `prop.value` is `Some` in the call site.
        debug_assert!(prop.value.is_some());
        Self::create_assignment(target, prop.value.take().unwrap(), ctx)
    }

    // Creates `a.b = value`
    fn create_assignment(
        target: AssignmentTarget<'a>,
        value: Expression<'a>,
        ctx: &TraverseCtx<'a>,
    ) -> Statement<'a> {
        ctx.ast.statement_expression(
            SPAN,
            ctx.ast.expression_assignment(SPAN, AssignmentOperator::Assign, target, value),
        )
    }
}
