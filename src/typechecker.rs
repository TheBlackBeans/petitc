use beans::span::Span;
use colored::Colorize;

use core::fmt;
use std::collections::HashMap;

use crate::{
    ast::*,
    error::{Error, ErrorKind, Result},
    parsing::{SpanAnnotation, WithSpan},
    typing::{BasisTypable, BasisType, Type},
};

static mut ERRORS: Vec<Error> = Vec::new();

fn report_error(error: Error) {
    // SAFETY: This program is single-threaded.
    unsafe { ERRORS.push(error) }
}

fn get_errors() -> Vec<Error> {
    // SAFETY: This program is single-threaded.
    unsafe { std::mem::take(&mut ERRORS) }
}

pub(crate) type PartialType = Type<PartialBasisType>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PartialBasisType {
    Void,
    Int,
    Bool,
    Error,
}

impl BasisTypable for PartialBasisType {
    const VOID: Self = Self::Void;
    const INT: Self = Self::Int;
    const BOOL: Self = Self::Bool;

    fn is_eq(left: &Type<Self>, right: &Type<Self>) -> bool {
        left == right
            || if left.is_ptr() {
                (right.is_ptr()
                    && ((left.indirection_count == 1
                        && left.basis == Self::Void)
                        || (right.indirection_count == 1
                            && right.basis == Self::Void)))
                    || right.basis == Self::Error
            } else if right.is_ptr() {
                left.basis == Self::Error
            } else {
                matches!(
                    (left.basis, right.basis),
                    (Self::Error, _)
                        | (_, Self::Error)
                        | (Self::Int, Self::Int)
                        | (Self::Bool, Self::Int)
                        | (Self::Int, Self::Bool)
                        | (Self::Bool, Self::Bool)
                        | (Self::Void, Self::Void)
                )
            }
    }
    fn to_basic(self) -> Option<BasisType> {
        match self {
            Self::Bool => Some(BasisType::Bool),
            Self::Int => Some(BasisType::Int),
            Self::Void => Some(BasisType::Void),
            _ => None,
        }
    }
}

impl fmt::Display for PartialBasisType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Error => write!(f, "{{unknown}}"),
            Self::Int => write!(f, "int"),
            Self::Bool => write!(f, "bool"),
            Self::Void => write!(f, "void"),
        }
    }
}

#[derive(Debug)]
pub struct PartialTypeAnnotation;

impl Annotation for PartialTypeAnnotation {
    type Ident = WithSpan<Ident>;
    type Type = WithSpan<PartialType>;
    type WrapExpr<T> = WithType<Option<T>, PartialType>;
    type WrapInstr<T> = TypedInstr<T, PartialType>;
    type WrapFunDecl<T> = WithSpan<T>;
    type WrapVarDecl<T> = WithSpan<T>;
    type WrapElseBranch<T> = Option<TypedInstr<T, PartialType>>;
}

impl File<PartialTypeAnnotation> {
    fn to_full(self) -> Option<File<TypeAnnotation>> {
        Some(File {
            fun_decls: self
                .fun_decls
                .into_iter()
                .map(|ws| ws.map_opt(|inner| inner.to_full()))
                .collect::<Option<_>>()?,
        })
    }
}

impl FunDecl<PartialTypeAnnotation> {
    fn to_full(self) -> Option<FunDecl<TypeAnnotation>> {
        let TypedInstr {
            instr,
            span,
            loop_level,
            expected_return_type,
        } = self.code;
        Some(FunDecl {
            ty: self.ty.to_full()?,
            name: self.name,
            params: self
                .params
                .into_iter()
                .map(|(arg_ty, arg)| Some((arg_ty.to_full()?, arg)))
                .collect::<Option<_>>()?,
            code: TypedInstr {
                instr: instr
                    .into_iter()
                    .map(|decl_instr| decl_instr.to_full())
                    .collect::<Option<_>>()?,
                span,
                loop_level,
                expected_return_type: expected_return_type.to_basic()?,
            },
            toplevel: self.toplevel,
        })
    }
}

impl DeclOrInstr<PartialTypeAnnotation> {
    fn to_full(self) -> Option<DeclOrInstr<TypeAnnotation>> {
        Some(match self {
            Self::Fun(fun) => {
                DeclOrInstr::Fun(fun.map_opt(|fun_decl| fun_decl.to_full())?)
            }
            Self::Var(var) => {
                DeclOrInstr::Var(var.map_opt(|var_decl| var_decl.to_full())?)
            }
            Self::Instr(instr) => DeclOrInstr::Instr(TypedInstr {
                instr: instr.instr.to_full()?,
                span: instr.span,
                loop_level: instr.loop_level,
                expected_return_type: instr.expected_return_type.to_basic()?,
            }),
        })
    }
}

impl VarDecl<PartialTypeAnnotation> {
    fn to_full(self) -> Option<VarDecl<TypeAnnotation>> {
        Some(VarDecl {
            ty: self.ty.to_full()?,
            name: self.name,
            value: if let Some(val) = self.value {
                let new_val = val.to_full()?;
                new_val.map_opt(|inner| inner.to_full())
            } else {
                None
            },
        })
    }
}

impl Expr<PartialTypeAnnotation> {
    fn to_full(self) -> Option<Expr<TypeAnnotation>> {
        Some(match self {
            Expr::Int(i) => Expr::Int(i),
            Expr::True => Expr::True,
            Expr::False => Expr::False,
            Expr::Null => Expr::Null,
            Expr::Ident(ident) => Expr::Ident(ident),
            Expr::Deref(expr) => {
                Expr::Deref(Box::new(expr.to_full()?.map_opt(|e| e.to_full())?))
            }
            Expr::Assign { lhs, rhs } => Expr::Assign {
                lhs: Box::new(lhs.to_full()?.map_opt(|e| e.to_full())?),
                rhs: Box::new(rhs.to_full()?.map_opt(|e| e.to_full())?),
            },
            Expr::Call { name, args } => Expr::Call {
                name,
                args: args
                    .into_iter()
                    .map(|arg| arg.to_full()?.map_opt(|e| e.to_full()))
                    .collect::<Option<_>>()?,
            },
            Expr::PrefixIncr(e) => Expr::PrefixIncr(Box::new(
                e.to_full()?.map_opt(|e| e.to_full())?,
            )),
            Expr::PrefixDecr(e) => Expr::PrefixDecr(Box::new(
                e.to_full()?.map_opt(|e| e.to_full())?,
            )),
            Expr::PostfixIncr(e) => Expr::PostfixIncr(Box::new(
                e.to_full()?.map_opt(|e| e.to_full())?,
            )),
            Expr::PostfixDecr(e) => Expr::PostfixDecr(Box::new(
                e.to_full()?.map_opt(|e| e.to_full())?,
            )),
            Expr::Addr(e) => {
                Expr::Addr(Box::new(e.to_full()?.map_opt(|e| e.to_full())?))
            }
            Expr::Not(e) => {
                Expr::Not(Box::new(e.to_full()?.map_opt(|e| e.to_full())?))
            }
            Expr::Neg(e) => {
                Expr::Neg(Box::new(e.to_full()?.map_opt(|e| e.to_full())?))
            }
            Expr::Pos(e) => {
                Expr::Pos(Box::new(e.to_full()?.map_opt(|e| e.to_full())?))
            }
            Expr::Op { op, lhs, rhs } => Expr::Op {
                op,
                lhs: Box::new(lhs.to_full()?.map_opt(|e| e.to_full())?),
                rhs: Box::new(rhs.to_full()?.map_opt(|e| e.to_full())?),
            },
            Expr::SizeOf(ty) => Expr::SizeOf(ty.to_full()?),
        })
    }
}

impl Instr<PartialTypeAnnotation> {
    fn to_full(self) -> Option<Instr<TypeAnnotation>> {
        Some(match self {
            Instr::EmptyInstr => Instr::EmptyInstr,
            Instr::ExprInstr(e) => {
                Instr::ExprInstr(e.to_full()?.map_opt(|e| e.to_full())?)
            }
            Instr::If {
                cond,
                then_branch,
                else_branch,
            } => Instr::If {
                cond: cond.to_full()?.map_opt(|e| e.to_full())?,
                then_branch: Box::new(
                    then_branch.to_full()?.map_opt(|e| e.to_full())?,
                ),
                else_branch: Box::new(if let Some(branch) = *else_branch {
                    Some(branch.to_full()?.map_opt(|e| e.to_full())?)
                } else {
                    None
                }),
            },
            Instr::While { cond, body } => Instr::While {
                cond: cond.to_full()?.map_opt(|e| e.to_full())?,
                body: Box::new(body.to_full()?.map_opt(|e| e.to_full())?),
            },
            Instr::For {
                loop_var,
                cond,
                incr,
                body,
            } => Instr::For {
                loop_var: if let Some(var_decl) = loop_var {
                    Some(var_decl.map_opt(|v| v.to_full())?)
                } else {
                    None
                },
                cond: if let Some(condition) = cond {
                    Some(condition.to_full()?.map_opt(|e| e.to_full())?)
                } else {
                    None
                },
                incr: incr
                    .into_iter()
                    .map(|expr| expr.to_full()?.map_opt(|e| e.to_full()))
                    .collect::<Option<_>>()?,
                body: Box::new(body.to_full()?.map_opt(|e| e.to_full())?),
            },
            Instr::Block(b) => Instr::Block(
                b.into_iter()
                    .map(|decl_instr| decl_instr.to_full())
                    .collect::<Option<_>>()?,
            ),
            Instr::Return(None) => Instr::Return(None),
            Instr::Return(Some(value)) => {
                Instr::Return(Some(value.to_full()?.map_opt(|e| e.to_full())?))
            }
            Instr::Break => Instr::Break,
            Instr::Continue => Instr::Continue,
        })
    }
}

pub struct TypeAnnotation;

impl Annotation for TypeAnnotation {
    type Ident = WithSpan<Ident>;
    type Type = WithSpan<Type>;
    type WrapExpr<T> = WithType<T, Type>;
    type WrapInstr<T> = TypedInstr<T, Type>;
    type WrapFunDecl<T> = WithSpan<T>;
    type WrapVarDecl<T> = WithSpan<T>;
    type WrapElseBranch<T> = Option<TypedInstr<T, Type>>;
}

impl<T> WithType<Option<T>, PartialType> {
    fn to_full(self) -> Option<WithType<T, Type>> {
        Some(WithType {
            inner: self.inner?,
            ty: self.ty.to_basic()?,
            span: self.span,
        })
    }
}

impl WithSpan<PartialType> {
    fn to_full(self) -> Option<WithSpan<Type>> {
        Some(WithSpan {
            inner: self.inner.to_basic()?,
            span: self.span,
        })
    }
}

impl PartialType {
    const ERROR: Self = Self {
        basis: PartialBasisType::Error,
        indirection_count: 0,
    };

    fn is_void(&self) -> bool {
        *self == Self::VOID
    }
}

#[derive(Debug)]
pub struct TypedInstr<U, T = Type> {
    pub instr: U,
    pub span: Span,
    pub loop_level: usize,
    pub expected_return_type: T,
}

impl<T> TypedInstr<T, PartialType> {
    fn to_full(self) -> Option<TypedInstr<T, Type>> {
        Some(TypedInstr {
            instr: self.instr,
            span: self.span,
            loop_level: self.loop_level,
            expected_return_type: self.expected_return_type.to_basic()?,
        })
    }
}

impl<U, T> TypedInstr<U, T> {
    fn map_opt<V>(
        self,
        f: impl FnOnce(U) -> Option<V>,
    ) -> Option<TypedInstr<V, T>> {
        Some(TypedInstr {
            instr: f(self.instr)?,
            span: self.span,
            loop_level: self.loop_level,
            expected_return_type: self.expected_return_type,
        })
    }
}

#[derive(Debug)]
pub struct WithType<U, T> {
    pub inner: U,
    pub ty: T,
    pub span: Span,
}

impl<U, T> WithType<U, T> {
    pub fn new(inner: U, ty: T, span: Span) -> Self {
        Self { inner, ty, span }
    }

    fn map_opt<V>(
        self,
        f: impl FnOnce(U) -> Option<V>,
    ) -> Option<WithType<V, T>> {
        Some(WithType {
            inner: f(self.inner)?,
            ty: self.ty,
            span: self.span,
        })
    }
}

pub type PartiallyTypedExpr = <PartialTypeAnnotation as Annotation>::WrapExpr<
    Expr<PartialTypeAnnotation>,
>;
pub type TypedExpr =
    <TypeAnnotation as Annotation>::WrapExpr<Expr<TypeAnnotation>>;

enum Binding {
    Var(PartialType),
    Fun((PartialType, Vec<PartialType>)),
}

type Environment = HashMap<Ident, (Binding, Option<Span>)>;

fn get_fun<'env>(
    env: &'env Environment,
    ident: WithSpan<Ident>,
    name_of: &'_ [String],
) -> Result<(&'env (PartialType, Vec<PartialType>), &'env Option<Span>)> {
    if let Some((Binding::Fun(res), span)) = env.get(&ident.inner) {
        Ok((res, span))
    } else {
        Err(Error::new(ErrorKind::NameError {
            name: name_of[ident.inner].clone(),
            span: ident.span,
        }))
    }
}

fn get_var(
    env: &Environment,
    ident: WithSpan<Ident>,
    name_of: &[String],
) -> Result<PartialType> {
    if let Some((Binding::Var(res), _)) = env.get(&ident.inner) {
        Ok(*res)
    } else {
        Err(Error::new(ErrorKind::NameError {
            name: name_of[ident.inner].clone(),
            span: ident.span,
        }))
    }
}

fn type_expr(
    e: WithSpan<Expr<SpanAnnotation>>,
    env: &Environment,
    name_of: &[String],
) -> PartiallyTypedExpr {
    match e.inner {
        Expr::True => {
            WithType::new(Some(Expr::True), PartialType::BOOL, e.span)
        }
        Expr::False => {
            WithType::new(Some(Expr::False), PartialType::BOOL, e.span)
        }
        Expr::Null => {
            WithType::new(Some(Expr::Null), PartialType::VOID.ptr(), e.span)
        }
        Expr::Int(n) => {
            WithType::new(Some(Expr::Int(n)), PartialType::INT, e.span)
        }
        Expr::Ident(name) => WithType::new(
            Some(Expr::Ident(name)),
            get_var(
                env,
                WithSpan {
                    inner: name,
                    span: e.span.clone(),
                },
                name_of,
            )
            .unwrap_or_else(|error| {
                report_error(error);
                PartialType::ERROR
            }),
            e.span,
        ),
        Expr::SizeOf(ty) => {
            let value = if !ty.inner.is_eq(&Type::VOID) {
                Some(Expr::SizeOf(WithSpan {
                    inner: ty.inner.from_basic(),
                    span: ty.span.clone(),
                }))
            } else {
                report_error(Error::new(ErrorKind::SizeofVoid {
                    span: e.span,
                }));
                None
            };
            WithType::new(value, PartialType::INT, ty.span)
        }
        Expr::Addr(inner_e) => {
            if !inner_e.inner.is_lvalue() {
                report_error(
		    Error::new(ErrorKind::AddressOfRvalue {
			span: e.span.clone(),
			expression_span: inner_e.span.clone(),
		    })
			.add_help(String::from(
			    "you could allocate this expression, by binding it to a variable"
			))
		)
            }
            let inner_e = type_expr(*inner_e, env, name_of);
            let ty = inner_e.ty.ptr();
            WithType::new(Some(Expr::Addr(Box::new(inner_e))), ty, e.span)
        }
        Expr::Deref(inner_e) => {
            let inner_e = type_expr(*inner_e, env, name_of);

            let ty: PartialType = if let Some(ty) = inner_e.ty.deref_ptr() {
                if ty.is_void() {
                    report_error(Error::new(ErrorKind::DerefVoidPointer {
                        span: inner_e.span.clone(),
                    }));
                    PartialType::ERROR
                } else {
                    ty
                }
            } else {
                if let Some(ty) = inner_e.ty.to_basic() {
                    report_error(Error::new(ErrorKind::DerefNonPointer {
                        ty,
                        span: inner_e.span.clone(),
                    }))
                }
                PartialType::ERROR
            };
            WithType::new(Some(Expr::Deref(Box::new(inner_e))), ty, e.span)
        }
        Expr::Assign { lhs, rhs } => {
            if !lhs.inner.is_lvalue() {
                report_error(Error::new(ErrorKind::RvalueAssignment {
                    span: lhs.span.clone(),
                }));
            }
            let lhs = type_expr(*lhs, env, name_of);
            let rhs = type_expr(*rhs, env, name_of);
            let ty1 = lhs.ty;
            let ty2 = rhs.ty;

            if !ty1.is_eq(&ty2) {
                report_error(Error::new(ErrorKind::TypeMismatch {
                    span: e.span.clone(),
                    expected_type: ty1,
                    found_type: ty2,
                }));
            }
            let found_type = if ty1 == PartialType::ERROR { ty2 } else { ty1 };
            WithType::new(
                Some(Expr::Assign {
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                }),
                found_type,
                e.span,
            )
        }
        Expr::PrefixIncr(inner_e) => {
            if !inner_e.inner.is_lvalue() {
                report_error(Error::new(ErrorKind::IncrOrDecrRvalue {
                    span: e.span.clone(),
                    expression_span: inner_e.span.clone(),
                }))
            }
            let inner_e = type_expr(*inner_e, env, name_of);
            let ty = inner_e.ty;
            WithType::new(Some(Expr::PrefixIncr(Box::new(inner_e))), ty, e.span)
        }
        Expr::PrefixDecr(inner_e) => {
            if !inner_e.inner.is_lvalue() {
                report_error(Error::new(ErrorKind::IncrOrDecrRvalue {
                    span: e.span.clone(),
                    expression_span: inner_e.span.clone(),
                }))
            }
            let inner_e = type_expr(*inner_e, env, name_of);
            let ty = inner_e.ty;
            WithType::new(Some(Expr::PrefixDecr(Box::new(inner_e))), ty, e.span)
        }
        Expr::PostfixIncr(inner_e) => {
            if !inner_e.inner.is_lvalue() {
                report_error(Error::new(ErrorKind::IncrOrDecrRvalue {
                    span: e.span.clone(),
                    expression_span: inner_e.span.clone(),
                }))
            }
            let inner_e = type_expr(*inner_e, env, name_of);
            let ty = inner_e.ty;
            WithType::new(
                Some(Expr::PostfixIncr(Box::new(inner_e))),
                ty,
                e.span,
            )
        }
        Expr::PostfixDecr(inner_e) => {
            if !inner_e.inner.is_lvalue() {
                report_error(Error::new(ErrorKind::IncrOrDecrRvalue {
                    span: e.span.clone(),
                    expression_span: inner_e.span.clone(),
                }))
            }
            let inner_e = type_expr(*inner_e, env, name_of);
            let ty = inner_e.ty;
            WithType::new(
                Some(Expr::PostfixDecr(Box::new(inner_e))),
                ty,
                e.span,
            )
        }
        Expr::Pos(inner_e) => {
            let inner_e = type_expr(*inner_e, env, name_of);
            let ty = inner_e.ty;

            if !ty.is_eq(&PartialType::INT) {
                report_error(Error::new(ErrorKind::TypeMismatch {
                    expected_type: PartialType::INT,
                    found_type: ty,
                    span: inner_e.span.clone(),
                }));
            }
            WithType::new(
                Some(Expr::Pos(Box::new(inner_e))),
                PartialType::INT,
                e.span,
            )
        }
        // The code here should be the same of the one at the previous branch
        Expr::Neg(inner_e) => {
            let inner_e = type_expr(*inner_e, env, name_of);
            let ty = inner_e.ty;

            if !ty.is_eq(&PartialType::INT) {
                report_error(Error::new(ErrorKind::TypeMismatch {
                    expected_type: PartialType::INT,
                    found_type: ty,
                    span: inner_e.span.clone(),
                }));
            }
            WithType::new(
                Some(Expr::Neg(Box::new(inner_e))),
                PartialType::INT,
                e.span,
            )
        }
        Expr::Not(inner_e) => {
            let inner_e = type_expr(*inner_e, env, name_of);
            if inner_e.ty.is_void() {
                report_error(Error::new(ErrorKind::VoidExpression {
                    span: inner_e.span.clone(),
                }))
            }
            WithType::new(
                Some(Expr::Not(Box::new(inner_e))),
                PartialType::INT,
                e.span,
            )
        }
        Expr::Op {
            op:
                op @ (BinOp::Eq
                | BinOp::NEq
                | BinOp::Lt
                | BinOp::Le
                | BinOp::Gt
                | BinOp::Ge),
            lhs,
            rhs,
        } => {
            let lhs = type_expr(*lhs, env, name_of);
            let rhs = type_expr(*rhs, env, name_of);
            let ty1 = lhs.ty;
            let ty2 = rhs.ty;

            if ty1.is_void() {
                report_error(Error::new(ErrorKind::VoidExpression {
                    span: lhs.span.clone(),
                }));
            }
            if ty2.is_void() {
                report_error(Error::new(ErrorKind::VoidExpression {
                    span: rhs.span.clone(),
                }));
            }
            if !ty1.is_eq(&ty2) {
                report_error(
                    Error::new(ErrorKind::TypeMismatch {
                        span: rhs.span.clone(),
                        expected_type: ty1,
                        found_type: ty2,
                    })
                    .add_help(format!(
                        "Type `{}` was expected because the expression ",
                        format!("{}", ty1).bold()
                    )),
                );
            }
            WithType::new(
                Some(Expr::Op {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                }),
                PartialType::INT,
                e.span,
            )
        }
        Expr::Op {
            op:
                op @ (BinOp::Mul
                | BinOp::Div
                | BinOp::Mod
                | BinOp::BOr
                | BinOp::BAnd),
            lhs,
            rhs,
        } => {
            let lhs = type_expr(*lhs, env, name_of);
            let rhs = type_expr(*rhs, env, name_of);
            let ty1 = lhs.ty;
            let ty2 = rhs.ty;
            if !ty1.is_eq(&PartialType::INT) {
                report_error(Error::new(ErrorKind::TypeMismatch {
                    span: lhs.span.clone(),
                    expected_type: PartialType::INT,
                    found_type: ty1,
                }));
            }
            if !ty2.is_eq(&PartialType::INT) {
                report_error(Error::new(ErrorKind::TypeMismatch {
                    span: rhs.span.clone(),
                    expected_type: PartialType::INT,
                    found_type: ty2,
                }));
            }
            WithType::new(
                Some(Expr::Op {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                }),
                PartialType::INT,
                e.span,
            )
        }
        Expr::Op {
            op: BinOp::Add,
            lhs,
            rhs,
        } => {
            let lhs = type_expr(*lhs, env, name_of);
            let rhs = type_expr(*rhs, env, name_of);
            let mut ty1 = lhs.ty;
            let mut ty2 = rhs.ty;
            let new_e = Expr::Op {
                op: BinOp::Add,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
            let ret_type = if ty1.is_ptr() && ty2.is_ptr() {
                report_error(
                    Error::new(ErrorKind::BuiltinBinopTypeMismatch {
                        left_type: ty1,
                        right_type: ty2,
                        span: e.span.clone(),
                        op: "+",
                    })
                    .reason(String::from("pointers cannot be added."))
                    .add_help(String::from(
                        "maybe you meant to subtract the pointers?",
                    )),
                );
                PartialType::ERROR
            } else {
                if ty2.is_ptr() {
                    std::mem::swap(&mut ty1, &mut ty2);
                }

                if ty1.is_ptr() {
                    if !ty2.is_eq(&PartialType::INT) {
                        report_error(Error::new(
                            ErrorKind::BuiltinBinopTypeMismatch {
                                left_type: ty1,
                                right_type: ty2,
                                span: e.span.clone(),
                                op: "+",
                            },
                        ))
                    }
                    ty1
                } else if !ty1.is_eq(&ty2) {
                    report_error(
                        Error::new(ErrorKind::BuiltinBinopTypeMismatch {
                            left_type: ty1,
                            right_type: ty2,
                            span: e.span.clone(),
                            op: "+",
                        })
                        .reason(format!(
                            "casting between {ty1} and {ty2} is undefined"
                        )),
                    );
                    PartialType::ERROR
                } else if !ty1.is_eq(&PartialType::INT) {
                    report_error(
                        Error::new(ErrorKind::BuiltinBinopTypeMismatch {
                            left_type: ty1,
                            right_type: ty2,
                            span: e.span.clone(),
                            op: "+",
                        })
                        .reason(format!("addition over `{ty1}` is undefined")),
                    );
                    PartialType::ERROR
                } else {
                    PartialType::INT
                }
            };

            WithType::new(Some(new_e), ret_type, e.span)
        }
        Expr::Op {
            op: BinOp::Sub,
            lhs,
            rhs,
        } => {
            let lhs = type_expr(*lhs, env, name_of);
            let rhs = type_expr(*rhs, env, name_of);
            let ty1 = lhs.ty;
            let ty2 = rhs.ty;
            let new_e = Expr::Op {
                op: BinOp::Sub,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };

            let ret_type = if ty1.is_ptr() {
                if ty2.is_ptr() {
                    if ty1 != ty2 {
                        report_error(
                            Error::new(ErrorKind::BuiltinBinopTypeMismatch {
                                left_type: ty1,
                                right_type: ty2,
                                span: e.span.clone(),
                                op: "-",
                            })
                            .reason(String::from(
                                "heterogeneous pointers cannot be subtracted",
                            )),
                        );
                        PartialType::ERROR
                    } else {
                        PartialType::INT
                    }
                } else if !ty2.is_eq(&PartialType::INT) {
                    report_error(Error::new(
                        ErrorKind::BuiltinBinopTypeMismatch {
                            left_type: ty1,
                            right_type: ty2,
                            span: e.span.clone(),
                            op: "-",
                        },
                    ));
                    PartialType::ERROR
                } else {
                    ty1
                }
            } else if !ty1.is_eq(&PartialType::INT) || !ty1.is_eq(&ty2) {
                let mut error =
                    Error::new(ErrorKind::BuiltinBinopTypeMismatch {
                        left_type: ty1,
                        right_type: ty2,
                        span: e.span.clone(),
                        op: "-",
                    });
                if ty1.is_eq(&PartialType::INT) && ty2.is_ptr() {
                    error = error
                        .add_help(String::from(
			    "maybe you meant to have the operands the other way around"
			));
                }
                report_error(error);
                PartialType::ERROR
            } else {
                PartialType::INT
            };
            println!("{} - {} : {}", ty1, ty2, ret_type);
            WithType::new(Some(new_e), ret_type, e.span)
        }
        Expr::Call { name, args } => {
            let ((ret_ty, args_ty), fun_span) =
                match get_fun(env, name.clone(), name_of) {
                    Ok(stuff) => stuff,
                    Err(error) => {
                        report_error(error);
                        return WithType::new(None, PartialType::ERROR, e.span);
                    }
                };
            if args.len() != args_ty.len() {
                report_error(Error::new(ErrorKind::ArityMismatch {
                    found_arity: args.len(),
                    expected_arity: args_ty.len(),
                    span: e.span.clone(),
                    definition_span: fun_span.clone(),
                    function_name: name_of[name.inner].clone(),
                }));
                return WithType::new(None, ret_ty.clone(), e.span);
            }

            let mut typed_args = Vec::new();

            for (arg, ty) in args.into_iter().zip(args_ty.iter()) {
                let arg = type_expr(arg, env, name_of);
                let arg_ty = arg.ty;

                if !arg_ty.is_eq(ty) {
                    report_error(Error::new(ErrorKind::TypeMismatch {
                        expected_type: *ty,
                        found_type: arg_ty,
                        span: arg.span.clone(),
                    }));
                }

                typed_args.push(arg);
            }

            WithType::new(
                Some(Expr::Call {
                    name,
                    args: typed_args,
                }),
                *ret_ty,
                e.span,
            )
        }
    }
}

fn typecheck_instr(
    instr: WithSpan<Instr<SpanAnnotation>>,
    loop_level: usize,
    expected_return_type: PartialType,
    env: &mut Environment,
    name_of: &[String],
) -> TypedInstr<Instr<PartialTypeAnnotation>, PartialType> {
    match instr.inner {
        Instr::EmptyInstr => TypedInstr {
            instr: Instr::EmptyInstr,
            span: instr.span,
            loop_level,
            expected_return_type,
        },
        Instr::ExprInstr(e) => TypedInstr {
            instr: Instr::ExprInstr(type_expr(e, env, name_of)),
            span: instr.span,
            loop_level,
            expected_return_type,
        },
        Instr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            let cond = type_expr(cond, env, name_of);
            let then_branch = typecheck_instr(
                *then_branch,
                loop_level,
                expected_return_type.clone(),
                env,
                name_of,
            );
            let else_branch = if let Some(else_branch) = *else_branch {
                Some(typecheck_instr(
                    else_branch,
                    loop_level,
                    expected_return_type.clone(),
                    env,
                    name_of,
                ))
            } else {
                None
            };
            if cond.ty.is_void() {
                report_error(Error::new(ErrorKind::VoidExpression {
                    span: cond.span.clone(),
                }));
            }
            if then_branch.expected_return_type != expected_return_type {
                report_error(
                    Error::new(ErrorKind::TypeMismatch {
                        span: then_branch.span.clone(),
                        expected_type: expected_return_type.clone(),
                        found_type: then_branch.expected_return_type.clone(),
                    })
                    .add_help(String::from("this should not happen")),
                );
            }
            if let Some(ref else_branch) = else_branch {
                if else_branch.expected_return_type != expected_return_type {
                    report_error(
                        Error::new(ErrorKind::TypeMismatch {
                            span: else_branch.span.clone(),
                            expected_type: expected_return_type.clone(),
                            found_type: else_branch
                                .expected_return_type
                                .clone(),
                        })
                        .add_help(String::from("this should not happen")),
                    );
                }
            }

            TypedInstr {
                instr: Instr::If {
                    cond,
                    then_branch: Box::new(then_branch),
                    else_branch: Box::new(else_branch),
                },
                span: instr.span,
                loop_level,
                expected_return_type,
            }
        }
        Instr::While { cond, body } => {
            let cond = type_expr(cond, env, name_of);
            let body = typecheck_instr(
                *body,
                loop_level + 1,
                expected_return_type.clone(),
                env,
                name_of,
            );
            if cond.ty.is_void() {
                report_error(Error::new(ErrorKind::VoidExpression {
                    span: cond.span.clone(),
                }));
            }
            if body.expected_return_type != expected_return_type {
                report_error(
                    Error::new(ErrorKind::TypeMismatch {
                        expected_type: expected_return_type.clone(),
                        found_type: body.expected_return_type.clone(),
                        span: body.span.clone(),
                    })
                    .add_help(String::from("this should not happen")),
                );
            }
            TypedInstr {
                instr: Instr::While {
                    cond,
                    body: Box::new(body),
                },
                span: instr.span,
                loop_level,
                expected_return_type,
            }
        }
        Instr::For {
            loop_var: None,
            cond,
            incr,
            body,
        } => {
            let cond = cond.map(|cond| type_expr(cond, env, name_of));
            let incr = incr
                .into_iter()
                .map(|incr| type_expr(incr, env, name_of))
                .collect::<Vec<_>>();
            let body = Box::new(typecheck_instr(
                *body,
                loop_level + 1,
                expected_return_type.clone(),
                env,
                name_of,
            ));

            if let Some(ref cond) = cond {
                if cond.ty.is_void() {
                    report_error(Error::new(ErrorKind::VoidExpression {
                        span: cond.span.clone(),
                    }))
                }
            }

            if body.expected_return_type != expected_return_type {
                report_error(
                    Error::new(ErrorKind::TypeMismatch {
                        expected_type: expected_return_type.clone(),
                        found_type: body.expected_return_type.clone(),
                        span: body.span.clone(),
                    })
                    .add_help(String::from("this should not happen")),
                );
            }
            TypedInstr {
                instr: Instr::For {
                    loop_var: None,
                    cond,
                    incr,
                    body,
                },
                span: instr.span,
                loop_level,
                expected_return_type,
            }
        }
        Instr::For {
            loop_var: Some(decl),
            cond,
            incr,
            body,
        } => typecheck_block(
            WithSpan::new(
                vec![
                    DeclOrInstr::Var(decl),
                    DeclOrInstr::Instr(WithSpan::new(
                        Instr::For {
                            loop_var: None,
                            cond,
                            incr,
                            body,
                        },
                        instr.span.clone(),
                    )),
                ],
                instr.span,
            ),
            loop_level,
            expected_return_type,
            env,
            name_of,
        ),
        Instr::Block(block) => typecheck_block(
            WithSpan::new(block, instr.span),
            loop_level,
            expected_return_type,
            env,
            name_of,
        ),
        Instr::Return(None) => {
            if !expected_return_type.is_void() {
                report_error(Error::new(ErrorKind::TypeMismatch {
                    span: instr.span.clone(),
                    expected_type: expected_return_type.clone(),
                    found_type: PartialType::VOID,
                })
                    .reason(String::from(
			"a `return` statement without arguments requires the current function to have a return type `void`"
		    ))
                    .add_help(format!(
			"try adding an argument `{}`",
			format!("return /* {expected_return_type} */;").bold())
		    ));
            }
            TypedInstr {
                instr: Instr::Return(None),
                span: instr.span,
                loop_level,
                expected_return_type,
            }
        }
        Instr::Return(Some(e)) => {
            let e = type_expr(e, env, name_of);
            if !e.ty.is_eq(&expected_return_type) {
                report_error(Error::new(ErrorKind::TypeMismatch {
                    span: instr.span.clone(),
                    expected_type: expected_return_type.clone(),
                    found_type: e.ty.clone(),
                }))
            }
            TypedInstr {
                instr: Instr::Return(Some(e)),
                span: instr.span,
                loop_level,
                expected_return_type,
            }
        }
        Instr::Break | Instr::Continue => {
            if loop_level == 0 {
                report_error(Error::new(ErrorKind::BreakContinueOutsideLoop {
                    span: instr.span.clone(),
                }))
            }
            TypedInstr {
                instr: Instr::Break,
                span: instr.span,
                loop_level,
                expected_return_type,
            }
        }
    }
}

/// On returning an instr, always returns a block
fn typecheck_block(
    block: WithSpan<Vec<DeclOrInstr<SpanAnnotation>>>,
    loop_level: usize,
    expected_return_type: PartialType,
    env: &mut Environment,
    name_of: &[String],
) -> TypedInstr<Instr<PartialTypeAnnotation>, PartialType> {
    let mut new_bindings: Vec<(WithSpan<Ident>, Option<Binding>)> = Vec::new();
    let mut ret = Vec::new();

    fn assert_var_is_not_reused(
        var_name: WithSpan<Ident>,
        new_bindings: &[(WithSpan<Ident>, Option<Binding>)],
        name_of: &[String],
    ) -> Result<()> {
        if let Some((_, first_definition_span)) = new_bindings
            .iter()
            .map(|(WithSpan { inner, span }, _)| (*inner, span))
            .find(|(name, _)| *name == var_name.inner)
        {
            Err(Error::new(ErrorKind::SymbolDefinedTwice {
                first_definition: first_definition_span.clone(),
                second_definition: var_name.span,
                name: name_of[var_name.inner].clone(),
            }))
        } else {
            Ok(())
        }
    }

    for decl_or_instr in block.inner {
        match decl_or_instr {
            DeclOrInstr::Fun(fun_decl) => {
                if let Err(error) = assert_var_is_not_reused(
                    fun_decl
                        .inner
                        .name
                        .clone()
                        .with_span(fun_decl.span.clone()),
                    &new_bindings,
                    name_of,
                ) {
                    report_error(error);
                };
                let fun_decl = typecheck_fun(fun_decl, env, name_of);
                new_bindings.push((
                    fun_decl
                        .inner
                        .name
                        .clone()
                        .with_span(fun_decl.span.clone()),
                    env.remove(&fun_decl.inner.name.inner).map(|x| x.0),
                ));
                env.insert(
                    fun_decl.inner.name.inner,
                    (
                        Binding::Fun((
                            fun_decl.inner.ty.inner.clone(),
                            fun_decl
                                .inner
                                .params
                                .iter()
                                .map(|(ty, _)| ty.inner.clone())
                                .collect(),
                        )),
                        Some(fun_decl.span.clone()),
                    ),
                );
                ret.push(DeclOrInstr::Fun(fun_decl));
            }
            DeclOrInstr::Var(var_decl) => {
                if var_decl.inner.ty.inner.is_eq(&Type::VOID) {
                    report_error(Error::new(ErrorKind::VoidVariable {
                        span: var_decl.span.clone(),
                        name: name_of[var_decl.inner.name.inner].clone(),
                    }));
                }
                if let Err(error) = assert_var_is_not_reused(
                    var_decl
                        .inner
                        .name
                        .clone()
                        .with_span(var_decl.span.clone()),
                    &new_bindings,
                    name_of,
                ) {
                    report_error(error)
                };
                new_bindings.push((
                    var_decl
                        .inner
                        .name
                        .clone()
                        .with_span(var_decl.span.clone()),
                    env.remove(&var_decl.inner.name.inner).map(|x| x.0),
                ));
                env.insert(
                    var_decl.inner.name.inner,
                    (
                        Binding::Var(var_decl.inner.ty.inner.from_basic()),
                        Some(var_decl.span.clone()),
                    ),
                );
                let value = var_decl
                    .inner
                    .value
                    .map(|value| type_expr(value, env, name_of));

                if let Some(ref val) = value {
                    let var_decl_type = var_decl.inner.ty.inner.from_basic();
                    if !val.ty.is_eq(&var_decl_type) {
                        report_error(Error::new(
                            ErrorKind::VariableTypeMismatch {
                                expected_type: var_decl_type,
                                found_type: val.ty.clone(),
                                span: val.span.clone(),
                                definition_span: var_decl
                                    .inner
                                    .ty
                                    .span
                                    .sup(&var_decl.inner.name.span),
                                variable_name: name_of
                                    [var_decl.inner.name.inner]
                                    .clone(),
                            },
                        ));
                    }
                }

                ret.push(DeclOrInstr::Var(WithSpan::new(
                    VarDecl {
                        ty: var_decl.inner.ty.into(),
                        name: var_decl.inner.name,
                        value,
                    },
                    var_decl.span,
                )));
            }
            DeclOrInstr::Instr(instr) => {
                ret.push(DeclOrInstr::Instr(typecheck_instr(
                    instr,
                    loop_level,
                    expected_return_type,
                    env,
                    name_of,
                )))
            }
        }
    }
    for (name, old_binding) in new_bindings {
        if let Some(binding) = old_binding {
            env.insert(name.inner, (binding, Some(name.span)));
        } else {
            env.remove(&name.inner);
        }
    }

    TypedInstr {
        instr: Instr::Block(ret),
        span: block.span,
        loop_level,
        expected_return_type,
    }
}

/// Insert the function in fun_env
/// Caller should remove it later if needed,
/// and saved previous value
fn typecheck_fun(
    decl: WithSpan<FunDecl<SpanAnnotation>>,
    env: &mut Environment,
    name_of: &[String],
) -> WithSpan<FunDecl<PartialTypeAnnotation>> {
    let code_span = decl.inner.code.span.clone();
    let code = decl
        .inner
        .params
        .iter()
        .map(|(ty, name)| {
            DeclOrInstr::Var(WithSpan::new(
                VarDecl {
                    ty: ty.clone(),
                    name: name.clone(),
                    value: None,
                },
                ty.span.sup(&name.span),
            ))
        })
        .chain(decl.inner.code.inner.into_iter())
        .collect::<Vec<_>>();
    env.insert(
        decl.inner.name.inner,
        (
            Binding::Fun((
                decl.inner.ty.inner.from_basic(),
                decl.inner
                    .params
                    .iter()
                    .map(|(ty, _)| ty.inner.from_basic())
                    .collect(),
            )),
            Some(decl.span.clone()),
        ),
    );

    let typed_instr = typecheck_block(
        WithSpan::new(code, decl.inner.code.span),
        0,
        decl.inner.ty.inner.from_basic(),
        env,
        name_of,
    );

    let Instr::Block(mut code) =
        typed_instr.instr
    else { unreachable!("Internal error") };

    code = code.into_iter().skip(decl.inner.params.len()).collect();

    let typed_code = TypedInstr {
        instr: code,
        span: code_span,
        loop_level: typed_instr.loop_level,
        expected_return_type: typed_instr.expected_return_type,
    };

    WithSpan::new(
        FunDecl {
            ty: decl.inner.ty.into(),
            name: decl.inner.name,
            params: decl
                .inner
                .params
                .into_iter()
                .map(|(left, right)| (left.into(), right))
                .collect(),
            code: typed_code,
            toplevel: decl.inner.toplevel,
        },
        decl.span,
    )
}

pub fn typecheck(
    file: File<SpanAnnotation>,
    name_of: &[String],
) -> std::result::Result<File<TypeAnnotation>, Vec<Error>> {
    if let Some(WithSpan {
        inner: main_decl,
        span: main_span,
    }) = &file
        .fun_decls
        .iter()
        .find(|decl| name_of[decl.inner.name.inner] == "main")
    {
        if main_decl.ty.inner != Type::INT || !main_decl.params.is_empty() {
            report_error(Error::new(ErrorKind::IncorrectMainFunctionType {
                ty: main_decl.ty.inner,
                params: main_decl
                    .params
                    .iter()
                    .map(|(ty, _)| ty.inner.from_basic())
                    .collect(),
                span: main_span.clone(),
            }));
        }
    } else {
        report_error(Error::new(ErrorKind::NoMainFunction));
    };

    let mut env = HashMap::new();
    env.insert(
        0,
        (
            Binding::Fun((PartialType::VOID.ptr(), vec![PartialType::INT])),
            None,
        ),
    );
    env.insert(
        1,
        (
            Binding::Fun((PartialType::INT, vec![PartialType::INT])),
            None,
        ),
    );
    let mut fun_decls = Vec::new();

    for decl in file.fun_decls {
        if let Ok((_, first_definition)) = get_fun(
            &env,
            decl.inner.name.clone().with_span(decl.span.clone()),
            name_of,
        ) {
            report_error(Error::new(ErrorKind::FunctionDefinedTwice {
                first_definition: first_definition.clone(),
                second_definition: decl.span.clone(),
                name: name_of[decl.inner.name.inner].clone(),
            }));
        }
        fun_decls.push(typecheck_fun(decl, &mut env, name_of));
    }

    let errors = get_errors();
    if errors.is_empty() {
        Ok(File { fun_decls }.to_full().unwrap())
    } else {
        Err(errors)
    }
}
