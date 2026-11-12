//! Check expression structure without recursive validation or width-sized work
//! lists. Parentheses and repeated postfix chains can otherwise compose several
//! individually bounded parses into one deeply nested expression tree.
use super::*;

enum Node<'a> {
    Expression(&'a Expr),
    Statement(&'a Stmt),
    Pattern(&'a Pattern),
    Expressions(&'a [Expr]),
    Statements(&'a [Stmt]),
    IfClauses(&'a [(Expr, Stmt)]),
    Patterns(&'a [Option<Pattern>]),
    ObjectExpressions(&'a [(String, Expr)]),
    ObjectPatterns(&'a [(String, Pattern)]),
    Variables(&'a [(String, Option<Expr>)]),
    PatternVariables(&'a [(Pattern, Option<Expr>)]),
    Parameters(&'a [Parameter]),
    Class(&'a ClassDefinition),
    ClassMembers(&'a [ClassMember]),
    SwitchClauses(&'a [(Option<Expr>, Vec<Stmt>)]),
    Binding(&'a ForBinding),
}

// Invoked when an expression/constructor/declaration or changed group tail
// finishes, not after every chain link. Unchanged groups reuse the checked
// tree. Each changed tail wraps that tree and is checked before another tail
// can compose it. Native parser depth and retained expression depth stay
// bounded at 64, so redundant groups cannot multiply walks of a wide AST.
// This counts structural depth, not all AST allocations or parser visits.
// Checking children before composing their parent also bounds rejected trees:
// at most one local chain (63 links) can be added to an already checked child.
pub(super) fn validate_expression_depth(expression: &Expr) -> Result<(), JsError> {
    walk(vec![(Node::Expression(expression), 0)]).map(|_| ())
}

pub(super) fn validate_statement_depth(statement: &Stmt) -> Result<(), JsError> {
    walk(vec![(Node::Statement(statement), 0)]).map(|_| ())
}

pub(super) fn validate_class_depth(class: &ClassDefinition) -> Result<(), JsError> {
    walk(vec![(Node::Class(class), 1)]).map(|_| ())
}

pub(super) fn validate_function_depth(
    body: &[Stmt],
    parameters: &[Parameter],
) -> Result<(), JsError> {
    walk(vec![
        (Node::Statements(body), 1),
        (Node::Parameters(parameters), 1),
    ])
    .map(|_| ())
}

// Immutable sequences cache this exact physical height. In particular an
// empty class declaration adds a level even when it contains no expressions.
pub(super) fn expression_height(expression: &Expr) -> Result<usize, JsError> {
    walk(vec![(Node::Expression(expression), 0)])
}
fn walk(mut pending: Vec<(Node<'_>, usize)>) -> Result<usize, JsError> {
    let mut max_depth = 0;
    while let Some((node, depth)) = pending.pop() {
        max_depth = max_depth.max(depth);
        // A slice continuation holds the unvisited siblings without allocating
        // one entry per child. Only the current ancestor path occupies the list.
        macro_rules! push {
            ($kind:ident, $value:expr) => {
                pending.push((Node::$kind($value), depth))
            };
        }
        match node {
            Node::Expression(expression) => {
                let depth = depth + 1;
                max_depth = max_depth.max(depth);
                if depth > 64 {
                    return Err(JsError::resource("Expression tree nesting budget exceeded"));
                }
                macro_rules! child {
                    ($kind:ident, $value:expr) => {
                        pending.push((Node::$kind($value), depth))
                    };
                }
                match expression {
                    Expr::Sequence(sequence) => {
                        // No consumer can mutate these children. Reuse the
                        // checked height instead of rescanning a growing prefix.
                        let deepest = depth - 1 + sequence.height();
                        if deepest > 64 {
                            return Err(JsError::resource(
                                "Expression tree nesting budget exceeded",
                            ));
                        }
                        max_depth = max_depth.max(deepest);
                    }
                    Expr::ConditionalChain(chain) => {
                        let deepest = depth - 1 + chain.height();
                        if deepest > 64 {
                            return Err(JsError::resource(
                                "Expression tree nesting budget exceeded",
                            ));
                        }
                        max_depth = max_depth.max(deepest);
                    }
                    Expr::Array(items) | Expr::Template(items) => child!(Expressions, items),
                    Expr::Object(items) => child!(ObjectExpressions, items),
                    Expr::Member(left, right)
                    | Expr::Binary(_, left, right)
                    | Expr::Assign(_, left, right) => {
                        child!(Expression, right);
                        child!(Expression, left);
                    }
                    Expr::Call(target, args) | Expr::Construct(target, args) => {
                        child!(Expressions, args);
                        child!(Expression, target);
                    }
                    Expr::ImportCall { specifier, options } => {
                        if let Some(options) = options {
                            child!(Expression, options);
                        }
                        child!(Expression, specifier);
                    }
                    Expr::Await(value)
                    | Expr::YieldDelegate(value)
                    | Expr::Unary(_, value)
                    | Expr::Update(value, ..)
                    | Expr::Spread(value)
                    | Expr::PrivateMember(value, _) => child!(Expression, value),
                    Expr::Yield(value) => {
                        if let Some(value) = value {
                            child!(Expression, value);
                        }
                    }
                    Expr::Conditional(condition, yes, no) => {
                        child!(Expression, no);
                        child!(Expression, yes);
                        child!(Expression, condition);
                    }
                    Expr::Class(class) => child!(Class, class),
                    Expr::Function {
                        body, parameters, ..
                    } => {
                        child!(Statements, body);
                        if let Some(parameters) = parameters {
                            child!(Parameters, parameters);
                        }
                    }
                    Expr::Literal(_)
                    | Expr::RegExp(..)
                    | Expr::Name(_)
                    | Expr::ImportMeta
                    | Expr::Hole
                    | Expr::Super => {}
                }
            }
            Node::Statement(statement) => match statement {
                Stmt::Expression(expression)
                | Stmt::FunctionDefinition(_, expression)
                | Stmt::Throw(expression) => push!(Expression, expression),
                Stmt::Variables(_, items) => push!(Variables, items),
                Stmt::PatternVariables(_, items) => push!(PatternVariables, items),
                Stmt::ClassDefinition(_, class) => {
                    if depth == 64 {
                        return Err(JsError::resource("Expression tree nesting budget exceeded"));
                    }
                    pending.push((Node::Class(class), depth + 1));
                }
                Stmt::Block(body) => push!(Statements, body),
                Stmt::If(condition, yes, no) => {
                    if let Some(no) = no {
                        push!(Statement, no);
                    }
                    push!(Statement, yes);
                    push!(Expression, condition);
                }
                Stmt::IfChain(chain) => {
                    if let Some(no) = chain.fallback() {
                        push!(Statement, no);
                    }
                    push!(IfClauses, chain.clauses());
                }
                Stmt::While(condition, body) | Stmt::DoWhile(body, condition) => {
                    push!(Statement, body);
                    push!(Expression, condition);
                }
                Stmt::For(initial, condition, update, body) => {
                    push!(Statement, body);
                    if let Some(initial) = initial {
                        push!(Statement, initial);
                    }
                    if let Some(condition) = condition {
                        push!(Expression, condition);
                    }
                    if let Some(update) = update {
                        push!(Expression, update);
                    }
                }
                Stmt::ForIn(binding, source, body)
                | Stmt::ForOf(binding, source, body)
                | Stmt::ForAwaitOf(binding, source, body) => {
                    push!(Statement, body);
                    push!(Binding, binding);
                    push!(Expression, source);
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    push!(Statements, body);
                    if let Some((_, body)) = catch {
                        push!(Statements, body);
                    }
                    if let Some(body) = finally {
                        push!(Statements, body);
                    }
                }
                Stmt::Switch(value, clauses) => {
                    push!(SwitchClauses, clauses);
                    push!(Expression, value);
                }
                Stmt::Label(_, body) => push!(Statement, body),
                Stmt::Return(value) => {
                    if let Some(value) = value {
                        push!(Expression, value);
                    }
                }
                Stmt::Empty | Stmt::Break(_) | Stmt::Continue(_) => {}
            },
            Node::Pattern(pattern) => match pattern {
                Pattern::Name(_) => {}
                Pattern::Array(values, rest) => {
                    push!(Patterns, values);
                    if let Some(rest) = rest {
                        push!(Pattern, rest);
                    }
                }
                Pattern::Object(values, rest) => {
                    push!(ObjectPatterns, values);
                    if let Some(rest) = rest {
                        push!(Pattern, rest);
                    }
                }
                Pattern::Default(pattern, value) => {
                    push!(Expression, value);
                    push!(Pattern, pattern);
                }
            },
            Node::Expressions(items) => {
                if let Some((first, rest)) = items.split_first() {
                    push!(Expressions, rest);
                    push!(Expression, first);
                }
            }
            Node::Statements(items) => {
                if let Some((first, rest)) = items.split_first() {
                    push!(Statements, rest);
                    push!(Statement, first);
                }
            }
            Node::Patterns(items) => {
                if let Some((first, rest)) = items.split_first() {
                    push!(Patterns, rest);
                    if let Some(first) = first {
                        push!(Pattern, first);
                    }
                }
            }
            Node::ObjectExpressions(items) => {
                if let Some(((_, first), rest)) = items.split_first() {
                    push!(ObjectExpressions, rest);
                    push!(Expression, first);
                }
            }
            Node::ObjectPatterns(items) => {
                if let Some(((_, first), rest)) = items.split_first() {
                    push!(ObjectPatterns, rest);
                    push!(Pattern, first);
                }
            }
            Node::Variables(items) => {
                if let Some(((_, first), rest)) = items.split_first() {
                    push!(Variables, rest);
                    if let Some(first) = first {
                        push!(Expression, first);
                    }
                }
            }
            Node::PatternVariables(items) => {
                if let Some(((pattern, value), rest)) = items.split_first() {
                    push!(PatternVariables, rest);
                    push!(Pattern, pattern);
                    if let Some(value) = value {
                        push!(Expression, value);
                    }
                }
            }
            Node::Parameters(items) => {
                if let Some((first, rest)) = items.split_first() {
                    push!(Parameters, rest);
                    push!(Pattern, &first.pattern);
                }
            }
            Node::Class(class) => {
                push!(ClassMembers, &class.members);
                if let Some(base) = &class.base {
                    push!(Expression, base);
                }
            }
            Node::ClassMembers(items) => {
                if let Some((first, rest)) = items.split_first() {
                    push!(ClassMembers, rest);
                    if let Some(value) = &first.value {
                        push!(Expression, value);
                    }
                }
            }
            Node::IfClauses(items) => {
                if let Some(((condition, body), rest)) = items.split_first() {
                    push!(IfClauses, rest);
                    push!(Statement, body);
                    push!(Expression, condition);
                }
            }
            Node::SwitchClauses(items) => {
                if let Some(((test, body), rest)) = items.split_first() {
                    push!(SwitchClauses, rest);
                    push!(Statements, body);
                    if let Some(test) = test {
                        push!(Expression, test);
                    }
                }
            }
            Node::Binding(binding) => match binding {
                ForBinding::Declaration(..) => {}
                ForBinding::Reference(value) => push!(Expression, value),
                ForBinding::Pattern(_, pattern) => push!(Pattern, pattern),
            },
        }
    }
    Ok(max_depth)
}
