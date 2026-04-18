//! TODO: docs.

use std::{any::Any, cell::RefCell, collections::HashMap, marker::PhantomData, pin::Pin, task};

pub use disguise_macros::{disguise_original as original, disguise_with as with};
use pin_project::pin_project;

/// TODO: docs.
pub trait Disguise: Sized + 'static {
    /// Attempts to [`call`] a disguised version of `target` if one exists in
    /// the current thread-local registry, passing `args` to it.
    ///
    /// # Errors
    ///
    /// Otherwise, if no disguise is found, returns <code>[Err]\(args)</code> so
    /// the caller can decide fallback logic.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use disguise::{coerce_fn, Disguise};
    /// fn mul(x: u32, y: u32) -> u32 {
    ///     match Disguise::disguise(coerce_fn!(mul), (x, y)) {
    ///         Ok(v) => v,
    ///         Err((x, y)) => x * y,
    ///     }
    /// }
    /// ```
    ///
    /// # Note
    ///
    /// The `target` must be a [function pointer][0]. To avoid manual casting,
    /// use [`coerce_fn!`] to coerce a function item to a function pointer.
    ///
    /// [`call`]: Function::call
    /// [0]: https://doc.rust-lang.org/reference/types/function-pointer.html
    #[inline]
    fn disguise<Args>(target: impl FnPtr<Args, Output = Self>, args: Args) -> Result<Self, Args>
    where
        Args: 'static,
    {
        DISGUISE.with_borrow(|registry| {
            if let Some(func) = registry
                .get(&target.addr())
                .and_then(|func| func.downcast_ref::<BoxedFunction<Args, Self>>())
            {
                Ok(func.call(args))
            } else {
                Err(args)
            }
        })
    }

    /// Calls a disguised version of `target` if one exists, passing `args` to
    /// it, otherwise returns `default`.
    ///
    /// This is a convenience wrapper around [`disguise`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use disguise::{coerce_fn, Disguise};
    /// fn sub(x: u32, y: u32) -> u32 {
    ///     Disguise::disguise_or(coerce_fn!(sub), (x, y), x - y)
    /// }
    /// ```
    ///
    /// [`disguise`]: Self::disguise
    #[inline]
    fn disguise_or<Args>(target: impl FnPtr<Args, Output = Self>, args: Args, default: Self) -> Self
    where
        Args: 'static,
    {
        Self::disguise(target, args).unwrap_or(default)
    }

    /// Calls a disguised version of `target` if one exists, passing `args` to
    /// it, otherwise computes a fallback using the provided `func`tion.
    ///
    /// The fallback receives the original arguments if no disguise is found.
    ///
    /// This is a convenience wrapper around [`disguise`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use disguise::{coerce_fn, Disguise};
    /// fn div(x: u32, y: u32) -> u32 {
    ///     Disguise::disguise_or_else(coerce_fn!(div), (x, y), |x, y| {
    ///         if y == 0 { 0 } else { x / y }
    ///     })
    /// }
    /// ```
    ///
    /// [`disguise`]: Self::disguise
    #[inline]
    fn disguise_or_else<Args>(
        target: impl FnPtr<Args, Output = Self>,
        args: Args,
        func: impl Function<Args, Output = Self>,
    ) -> Self
    where
        Args: 'static,
    {
        Self::disguise(target, args).unwrap_or_else(|args| func.call(args))
    }

    /// Calls a disguised version of `target` if one exists, passing `args` to
    /// it, otherwise uses [`Default`] to compute a fallback.
    ///
    /// This is a convenience wrapper around [`disguise`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use disguise::{coerce_fn, Disguise};
    /// fn zeroed(x: u32) -> u32 {
    ///     Disguise::disguise_or_default(coerce_fn!(zeroed), (x,))
    /// }
    /// ```
    ///
    /// [`disguise`]: Self::disguise
    #[inline]
    fn disguise_or_default<Args>(target: impl FnPtr<Args, Output = Self>, args: Args) -> Self
    where
        Args: 'static,
        Self: Default,
    {
        Self::disguise(target, args).unwrap_or_default()
    }
}

impl<T: 'static> Disguise for T {}

/// Unique identifier of a [`FnPtr`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ptr(*const ());

/// Shortcut for a `dyn`amically dispatched `'static` [`Function`] of arguments
/// `Args` and an output `Output`.
pub type BoxedFunction<Args, Output> = Box<dyn Function<Args, Output = Output> + 'static>;

thread_local! {
    /// Registry of [`FnPtr`]s' [`Disguise`]d [`BoxedFunction`]s for the current
    /// [`thread`].
    ///
    /// [`thread`]: std::thread
    static DISGUISE: RefCell<HashMap<Ptr, Box<dyn Any>>> =
        RefCell::default();
}

/// A guard managing the lifetime of a single active function [`Disguise`].
///
/// When created, it inserts a new [`BoxedFunction`] into the thread-local
/// registry for a specific [`FnPtr`] `target` and upon [`Drop`] restores the
/// `prev`ious state (if any) or removes the entry entirely.
///
/// If you don't need to bind a guard to a variable to control its lifetime,
/// use [`with_fn!`] instead, as it is more ergonomic.
///
/// # Examples
///
/// ```rust
/// # use disguise::{coerce_fn, Disguise, ScopeGuard};
/// fn add(x: u32, y: u32) -> u32 {
/// #   Disguise::disguise_or(coerce_fn!(add), (x, y), x + y)
///     // ...
/// }
///
/// assert_eq!(add(3, 4), 7);
/// {
///     // use `coerce_fn!` to coerce `add` to an `fn(u32, u32) -> u32` without
///     // manually spelling the signature
///     let _guard = ScopeGuard::new(coerce_fn!(add), |x, y| x * y);
///     // use instead:
///     // disguise::with_fn!(add = |x, y| x * y);
///     assert_eq!(add(3, 4), 12);
/// }
///
/// // guard drops here, original behavior restored
/// assert_eq!(add(3, 4), 7);
/// ```
#[must_use = "not binding [`ScopeGuard`] to a variable will [`Drop`] it \
              immediately and do nothing to the scope."]
#[derive(Debug)]
pub struct ScopeGuard {
    /// Previously stored boxed function (if any).
    ///
    /// This is restored on [`Drop`] to preserve prior state.
    prev: Option<Box<dyn Any>>,

    /// Function pointer identifying the patched target.
    target: Ptr,

    /// Marker to prevent accidental cross-thread usage.
    _not_send: PhantomData<Ptr>,
}

impl ScopeGuard {
    /// Places a new `disguise` [`Function`] for `target` [`FnPtr`] into
    /// registry until this guard is [`Drop`]ped.
    ///
    /// The previous disguise (if any) is stored and will be restored upon
    /// [`Drop`]ping.
    ///
    /// If you don't need to bind a guard to a variable to control its lifetime,
    /// use [`with_fn!`] instead, as it is more ergonomic.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use disguise::{coerce_fn, Disguise, ScopeGuard};
    /// fn add(x: u32, y: u32) -> u32 {
    /// #   Disguise::disguise_or(coerce_fn!(add), (x, y), x + y)
    ///     // ...
    /// }
    ///
    /// assert_eq!(add(3, 4), 7);
    /// {
    ///     // use `coerce_fn!` to coerce `add` to an `fn(u32, u32) -> u32`
    ///     // without manually spelling the signature
    ///     let _guard = ScopeGuard::new(coerce_fn!(add), |x, y| x * y);
    ///     // use instead:
    ///     // disguise::with_fn!(add = |x, y| x * y);
    ///     assert_eq!(add(3, 4), 12);
    /// }
    ///
    /// // guard drops here, original behavior restored
    /// assert_eq!(add(3, 4), 7);
    /// ```
    #[inline]
    pub fn new<Args, Output, F, D>(target: F, disguise: D) -> Self
    where
        Args: 'static,
        Output: 'static,
        F: FnPtr<Args, Output = Output>,
        D: Function<Args, Output = Output> + 'static,
    {
        let target = target.addr();
        let disguise: BoxedFunction<Args, Output> = Box::new(disguise);
        Self {
            target,
            prev: DISGUISE.with_borrow_mut(|registry| registry.insert(target, Box::new(disguise))),
            _not_send: PhantomData,
        }
    }
}

impl Drop for ScopeGuard {
    #[inline]
    fn drop(&mut self) {
        drop(DISGUISE.with_borrow_mut(|registry| {
            if let Some(prev) = self.prev.take() {
                registry.insert(self.target, prev)
            } else {
                registry.remove(&self.target)
            }
        }));
    }
}

/// A [`Future`] that carries a [`Disguise`] scope.
///
/// This struct is created by the [`DisguiseScopeExt::disguise_with`] or
/// [`DisguiseScopeExt::disguise_with_value`] methods. It wraps an inner future
/// and installs a disguise for the duration of that [`Future`]'s execution.
///
/// [`DisguiseScope`] is <code>\![Send]</code>; if you need to `spawn` a
/// disguised future, consider disguising on the target thread using
/// [`with_fn!`] instead:
///
/// ```rust,compile_fail
/// # tokio::runtime::Runtime::new().unwrap().block_on(async {
/// # use disguise::{coerce_fn, Disguise, DisguiseScopeExt as _};
/// fn greet(name: &'static str) -> String {
/// #   Disguise::disguise_or_else(coerce_fn!(greet), (name,), |name| {
/// #       format!("Hello, {name}!")
/// #   })
///     // ...
/// }
///
/// assert_eq!(greet("Alice"), "Hello, Alice!");
///
/// tokio::spawn(
///     async {
///         assert_eq!(greet("Alice"), "Goodbye, Alice!");
///     }
///     .disguise_with(coerce_fn!(greet), |name| format!("Goodbye, {name}!")),
/// //   ^^^^^^^^^^^^^
/// //   `(dyn Any + 'static)` cannot be sent between threads safely
/// //   `*const ()` cannot be sent between threads safely
/// )
/// .await;
///
/// assert_eq!(greet("Alice"), "Hello, Alice!");
/// # });
/// ```
///
/// Do this instead:
///
/// ```rust
/// # tokio::runtime::Runtime::new().unwrap().block_on(async {
/// # use disguise::{coerce_fn, Disguise, DisguiseScopeExt as _};
/// # fn greet(name: &'static str) -> String {
/// #   Disguise::disguise_or_else(coerce_fn!(greet), (name,), |name| {
/// #       format!("Hello, {name}!")
/// #   })
/// # }
/// #
/// # assert_eq!(greet("Alice"), "Hello, Alice!");
/// #
/// tokio::spawn(async {
///     disguise::with_fn!(greet = |name| format!("Goodbye, {name}!"));
///     assert_eq!(greet("Alice"), "Goodbye, Alice!");
/// })
/// .await;
/// #
/// # assert_eq!(greet("Alice"), "Hello, Alice!");
/// # });
/// ```
#[pin_project]
#[derive(Debug)]
pub struct DisguiseScope<Fut: ?Sized> {
    /// [`ScopeGuard`] to restore the previous disguise.
    guard: ScopeGuard,

    /// Original [`Future`] to have a function [`Disguise`]d inside of it.
    #[pin]
    fut: Fut,
}

impl<Fut> Future for DisguiseScope<Fut>
where
    Fut: Future + ?Sized,
{
    type Output = Fut::Output;

    #[inline]
    fn poll(self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> task::Poll<Self::Output> {
        self.project().fut.poll(cx)
    }
}

/// [`Future`] extension allowing to [`Disguise`] the target function inside
/// itself.
///
/// This trait is automatically implemented for all [`Future`]s.
///
/// # Example
///
/// ```rust
/// # use disguise::{Disguise, DisguiseScopeExt as _, coerce_fn};
/// fn greet(name: &'static str) -> String {
/// #   Disguise::disguise_or_else(
/// #       coerce_fn!(greet),
/// #       (name,),
/// #       |name| format!("Hello, {name}!"),
/// #   )
///     // ...
/// }
///
/// # tokio::runtime::Runtime::new().unwrap().block_on(async {
/// assert_eq!(greet("Alice"), "Hello, Alice!");
/// async {
///     greet("Alice");
///     assert_eq!(greet("Alice"), "Goodbye, Alice!");
/// }
/// // use `coerce_fn!` to coerce `greet` to an `fn(&'static str) -> String`
/// // without manually spelling the signature
/// .disguise_with(coerce_fn!(greet), |name| format!("Goodbye, {name}!"))
/// .await;
/// # });
/// ```
pub trait DisguiseScopeExt: Future + Sized {
    /// [`Disguise`]s the `target` [`FnPtr`] with the provided
    /// `disguise` [`Function`] for the [`DisguiseScope`] of this [`Future`].
    ///
    /// If you only have one value to disguise the `target` with, consider using
    /// [`DisguiseScopeExt::disguise_with_value`] instead.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use disguise::{Disguise, DisguiseScopeExt as _, coerce_fn};
    /// fn greet(name: &'static str) -> String {
    /// #   Disguise::disguise_or_else(
    /// #       coerce_fn!(greet),
    /// #       (name,),
    /// #       |name| format!("Hello, {name}!"),
    /// #   )
    ///     // ...
    /// }
    ///
    /// # tokio::runtime::Runtime::new().unwrap().block_on(async {
    /// assert_eq!(greet("Alice"), "Hello, Alice!");
    /// async {
    ///     greet("Alice");
    ///     assert_eq!(greet("Alice"), "Goodbye, Alice!");
    /// }
    /// // use `coerce_fn!` to coerce `greet` to an `fn(&'static str) -> String`
    /// // without manually spelling the signature
    /// .disguise_with(coerce_fn!(greet), |name| format!("Goodbye, {name}!"))
    /// .await;
    /// # });
    /// ```
    #[inline]
    fn disguise_with<Args, Output>(
        self,
        target: impl FnPtr<Args, Output = Output>,
        disguise: impl Function<Args, Output = Output> + 'static,
    ) -> DisguiseScope<Self>
    where
        Args: 'static,
        Output: 'static,
    {
        DisguiseScope {
            guard: ScopeGuard::new(target, disguise),
            fut: self,
        }
    }

    /// [`Disguise`]s the `target` [`FnPtr`] with the provided constant
    /// `disguise` value for the [`DisguiseScope`] of this [`Future`].
    ///
    /// It will ignore any `Args` passed to the `target` function and always
    /// return a [`Clone`]e of the `disguise` on each [`call`].
    ///
    /// If the value is expensive to compute the first time, consider using
    /// [`DisguiseScopeExt::disguise_with`] instead.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use disguise::{Disguise, DisguiseScopeExt as _, coerce_fn};
    /// fn compute_value() -> u32 {
    /// #   Disguise::disguise_or(coerce_fn!(compute_value), (), 42)
    ///     // ...
    /// }
    ///
    /// # tokio::runtime::Runtime::new().unwrap().block_on(async {
    /// assert_eq!(compute_value(), 42);
    ///
    /// async {
    ///     compute_value();
    ///     assert_eq!(compute_value(), 999);
    /// }
    /// // use `coerce_fn!` to coerce `compute_value` to an `fn() -> u32`
    /// // without manually spelling the signature
    /// .disguise_with_value(coerce_fn!(compute_value), 999)
    /// .await;
    /// # });
    /// ```
    ///
    /// [`call`]: Function::call
    #[inline]
    fn disguise_with_value<Args, Output>(
        self,
        target: impl FnPtr<Args, Output = Output>,
        disguise: Output,
    ) -> DisguiseScope<Self>
    where
        Args: 'static,
        Output: Clone + 'static,
    {
        self.disguise_with(target, ValueDisguise(disguise))
    }

    // TODO: in_current_disguise_scope
}

impl<Fut> DisguiseScopeExt for Fut where Fut: Future {}

/// A callable object abstraction.
///
/// Represents an [`Fn`] closure or a [function pointer][0] that takes
/// a tuple of `Args` and returns an `Output`.
///
/// It is implemented for [`Fn`]s of up to 13 `Args`.
///
/// [0]: https://doc.rust-lang.org/reference/types/function-pointer.html
pub trait Function<Args> {
    /// The type returned by the [`call`].
    ///
    /// [`call`]: Self::call
    type Output;

    /// Invokes the [`Function`] with the provided `args` tuple.
    fn call(&self, args: Args) -> Self::Output;
}

/// A trait for getting the [`addr`]ess of a [function pointer][0].
///
/// It is implemented for [function pointer][0]s of up to 13 `Args`.
///
/// [`addr`]: Self::addr
/// [0]: https://doc.rust-lang.org/reference/types/function-pointer.html
pub trait FnPtr<Args>: Copy {
    /// The return type of the [function pointer][0].
    ///
    /// [0]: https://doc.rust-lang.org/reference/types/function-pointer.html
    type Output;

    /// Returns the [function pointer][0] address as a [`Ptr`].
    ///
    /// [0]: https://doc.rust-lang.org/reference/types/function-pointer.html
    fn addr(self) -> Ptr;
}

/// A [`Function`] that ignores its arguments and returns a [`Clone`]d value on
/// each [`call`].
///
/// Used internally by [`DisguiseScopeExt::disguise_with_value`] and the value
/// form of [`with_fn!`].
///
/// [`call`]: Function::call
struct ValueDisguise<T>(T);

impl<T, Args> Function<Args> for ValueDisguise<T>
where
    T: Clone,
{
    type Output = T;

    #[inline]
    fn call(&self, _args: Args) -> T {
        self.0.clone()
    }
}

/// Bootstrap macro for [`Function`]-related traits.
///
/// The macro generates [`Function`] and [`CoerceFnItem`] implementations for
/// functions of up to 13 arguments and [`FnPtr`], [`FnPtrOutput`]
/// implementations for [function pointer][0]s of up to 13 arguments.
///
/// [`FnPtrOutput`]: __internal::FnPtrOutput
/// [`CoerceFnItem`]: __internal::CoerceFnItem
/// [0]: https://doc.rust-lang.org/reference/types/function-pointer.html
macro_rules! impl_functions {
    () => { /* empty */ };
    ($head:ident $($tail:ident)*) => {
        impl<F, Output, $($tail,)*>
            $crate::Function<($($tail,)*)> for F
        where
            F: Fn($($tail,)*) -> Output,
        {
            type Output = Output;
            #[inline]
            fn call(
                &self,
                #[allow(
                    unfulfilled_lint_expectations,
                    clippy::allow_attributes,
                    reason = "in the base case we don't have any identifiers"
                )]
                #[expect(
                    nonstandard_style,
                    clippy::min_ident_chars,
                    reason = "common and harmless abuse of the fact that \
                              idents and types are the same on token level"
                )]
                ($($tail,)*): ($($tail,)*),
            ) -> Self::Output {
                self($($tail,)*)
            }
        }

        impl<Output, $($tail,)*>
            $crate::FnPtr<($($tail,)*)> for fn($($tail,)*) -> Output
        {
            type Output = Output;
            #[inline]
            fn addr(self) -> Ptr {
                #[expect(
                    clippy::fn_to_numeric_cast_any,
                    clippy::as_conversions,
                    reason = "storing function's address to fetch it later"
                )]
                {
                    Ptr(self as *const ())
                }
            }
        }

        impl<Output, $($tail,)*>
            __internal::FnPtrOutput<($($tail,)*)> for Output
        {
            type FnPtr = fn($($tail,)*) -> Output;
        }

        impl<F, Output, $($tail,)*>
            __internal::CoerceFnItem<($($tail,)*)> for F
        where
            F: FnOnce($($tail,)*) -> Output + Copy,
        {
            type Output = Output;
        }

        impl_functions!($($tail)*);
    };
}

impl_functions!(A B C D E G H I J K L M N);

#[doc(hidden)]
pub mod __internal {
    //! Internal machinery used by [`disguise::original`], [`disguise::with`],
    //! [`disguise::with_fn`], and [`disguise::coerce_fn`] macros. Not part of
    //! the public API.
    //!
    //! [`disguise::original`]: macro@crate::original
    //! [`disguise::with`]: macro@crate::with
    //! [`disguise::with_fn`]: macro@crate::with_fn
    //! [`disguise::coerce_fn`]: macro@crate::coerce_fn

    use super::{Function, PhantomData, ValueDisguise};

    /// Maps `Args` to the output type `Self`. Required so that we can refer to
    /// a function pointer knowing only its type signature.
    pub trait FnPtrOutput<Args> {
        /// The type of the function pointer of signature `fn(...Args) -> Self`.
        type FnPtr;
    }

    /// Shortcut for the type of a function pointer of signature `fn(...Args) ->
    /// Output`.
    pub type FnPtr<Args, Output> = <Output as FnPtrOutput<Args>>::FnPtr;

    /// Generically coerces a [function item][0] into a [function pointer][1].
    ///
    /// This trait is implemented for all [`FnOnce`] functions of up to 13
    /// arguments. Function items receive this implementation, and we abuse this
    /// fact to [`coerce`] them to [function pointer][1]s by extracting their
    /// `Args` and [`Output`] signature and requiring to pass the same
    /// function as a `target` argument. This way the [function item][0] is
    /// coerced to a [function pointer][1] in the argument position without
    /// explicitly specifying the [`primitive@fn`] signature.
    ///
    /// # Note
    ///
    /// Technically, the function passed can be any other function of the same
    /// signature and it will be returned, but this trait is `doc(hidden)`, so
    /// we can control what is passed to [`coerce`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// use disguise::__internal::CoerceFnItem;
    ///
    /// fn compute() -> u32 {
    ///     42
    /// }
    ///
    /// // coerces to `fn() -> u32`
    /// let ptr = CoerceFnItem::coerce(compute, compute);
    /// ```
    ///
    /// [`coerce`]: Self::coerce
    /// [`Output`]: Self::Output
    /// [0]: https://doc.rust-lang.org/reference/types/function-item.html
    /// [1]: https://doc.rust-lang.org/reference/types/function-pointer.html
    pub trait CoerceFnItem<Args>: Copy {
        /// The return type of the function.
        type Output: FnPtrOutput<Args>;

        /// Uses inferred `Args` and [`Output`] to coerce the provided function
        /// to a [function pointer][1] and returns it. The `target` argument
        /// must be the same function this method was called on.
        ///
        /// [`Output`]: Self::Output
        /// [1]: https://doc.rust-lang.org/reference/types/function-pointer.html
        #[inline]
        fn coerce(self, target: FnPtr<Args, Self::Output>) -> FnPtr<Args, Self::Output> {
            target
        }
    }

    /// A wrapper for [autoref-based specialization][0] [`DisguiseSpec`]
    /// implementations.
    ///
    /// [0]: https://tinyurl.com/autoref-spec
    #[derive(Debug)]
    pub struct Wrapper<T>(pub T);

    /// [Autoref-based specialization][0] to abstract over a
    /// <code>[Function]<Args, Output></code> or an
    /// <code>[Into]\<Output></code>.
    ///
    /// [0]: https://tinyurl.com/autoref-spec
    pub trait DisguiseSpec<Args, Output> {
        /// Either a [`FunctionTag`] or [`IntoTag`].
        type Tag;

        /// Returns either a [`FunctionTag`] or [`IntoTag`].
        fn __resolve(&self) -> Self::Tag;
    }

    /// A tag for [autoref-based specialization][0] of a
    /// <code>[Into]\<Output></code> expression.
    ///
    /// [0]: https://tinyurl.com/autoref-spec
    #[derive(Debug)]
    pub struct IntoTag<Args, T, I>(PhantomData<(Args, T, I)>);

    impl<T, Args, Output> DisguiseSpec<Args, Output> for Wrapper<T>
    where
        T: Into<Output>,
        Output: Clone,
    {
        type Tag = IntoTag<Args, T, Output>;

        #[inline]
        fn __resolve(&self) -> Self::Tag {
            IntoTag(PhantomData)
        }
    }

    impl<Args, T, Output> IntoTag<Args, T, Output>
    where
        T: Into<Output>,
        Output: Clone,
    {
        /// Returns a <code>impl [Function]<Args, [Output] = Output></code> that
        /// ignores its `Args` and returns a [`Clone`] of `Output`
        /// `value` on each [`call`].
        ///
        /// [`Output`]: Function::Output
        /// [`call`]: Function::call
        #[inline]
        pub fn __into_disguise(self, value: T) -> impl Function<Args, Output = Output> {
            ValueDisguise(value.into())
        }
    }

    /// A tag for [autoref-based specialization][0] of a
    /// <code>[Function]<Args, Output></code> expression.
    ///
    /// [0]: https://tinyurl.com/autoref-spec
    #[derive(Debug)]
    pub struct FunctionTag<Args, Output, F>(PhantomData<(Args, Output, F)>);

    impl<F, Args, Output> DisguiseSpec<Args, Output> for &Wrapper<F>
    where
        F: Function<Args, Output = Output>,
    {
        type Tag = FunctionTag<Args, Output, F>;

        #[inline]
        fn __resolve(&self) -> Self::Tag {
            FunctionTag(PhantomData)
        }
    }

    impl<Args, Output, F> FunctionTag<Args, Output, F>
    where
        F: Function<Args, Output = Output>,
    {
        /// Returns an <code>impl [Function]<Args, [Output] = Output></code> as
        /// is.
        ///
        /// [Output]: Function::Output
        #[inline]
        pub fn __into_disguise(self, func: F) -> impl Function<Args, Output = Output> {
            func
        }
    }
}

/// Coerces a [function item][0] to a [function pointer][1] suitable for use
/// with [`Disguise`] as a target.
///
/// The [`disguise`] system uses [function pointer][1] addresses as identifiers.
/// In Rust, referring to a function item directly by name (e.g., `my_function`)
/// does not automatically coerce to a [function pointer][1] in all contexts. As
/// methods of [`Disguise`] and [`DisguiseScopeExt`] take generic types bound by
/// traits, simply passing a function item to them would not work and will
/// require users to manually coerce it to a [function pointer][1] (e.g.,
/// `my_function as fn(_) -> _`). To reduce the friction, this macro performs
/// that coercion for you, regardless of the passed function's signature.
///
/// # Syntax
///
/// - <code>[coerce_fn!]\(path::to::function);</code>
///
/// # Examples
///
/// ```rust
/// use disguise::coerce_fn;
///
/// fn compute() -> u32 {
///     42
/// }
///
/// fn generic<T>(x: T) -> T {
///     x
/// }
///
/// struct Foo;
///
/// impl Foo {
///     fn bar(&self) -> u32 {
///         42
///     }
/// }
///
/// let ptr = coerce_fn!(compute); // coerces to `fn() -> u32`
/// let ptr = coerce_fn!(generic::<u32>); // coerces to `fn(u32) -> u32`
/// let ptr = coerce_fn!(Foo::bar); // coerces to `fn(&Foo) -> u32`
/// ```
///
/// [0]: https://doc.rust-lang.org/reference/types/function-item.html
/// [1]: https://doc.rust-lang.org/reference/types/function-pointer.html
/// [`disguise`]: crate
#[macro_export]
macro_rules! coerce_fn {
    ($original:path $(,)?) => {
        $crate::__internal::CoerceFnItem::coerce($original, $original)
    };
}

/// Installs a function [`Disguise`] for the duration of the current scope.
///
/// This macro creates a [`ScopeGuard`] that overrides the behavior of the
/// specified function. The disguise is active until the guard is [`Drop`]ped.
/// If you need control of where the guard gets dropped, use the [`ScopeGuard`]
/// directly (see its documentation).
///
/// If you are in a [`Future`] context, use [`DisguiseScopeExt::disguise_with`]
/// or [`DisguiseScopeExt::disguise_with_value`] instead.
///
/// # Syntax
///
/// - <code>[with_fn!](path::to::function = closure_or_value);</code>
///
/// # Examples
///
/// Override with a closure that receives the original arguments:
///
/// ```rust
/// # use disguise::{coerce_fn, Disguise};
/// fn add(x: u32, y: u32) -> u32 {
/// #   Disguise::disguise_or(coerce_fn!(add), (x, y), x + y)
///     // ...
/// }
///
/// assert_eq!(add(3, 4), 7);
/// disguise::with_fn!(add = |x, y| x * y);
/// assert_eq!(add(3, 4), 12);
/// ```
///
/// Override with a constant value (ignores arguments):
///
/// ```rust
/// # use disguise::{coerce_fn, Disguise};
/// fn greet(name: &'static str) -> String {
/// #   Disguise::disguise_or_else(
/// #       coerce_fn!(greet),
/// #       (name,),
/// #       |name| format!("Hello, {name}!"),
/// #   )
///     // ...
/// }
///
/// assert_eq!(greet("Alice"), "Hello, Alice!");
/// disguise::with_fn!(greet = "Hello, disguised!".to_owned());
/// assert_eq!(greet("Alice"), "Hello, disguised!");
/// ```
#[macro_export]
macro_rules! with_fn {
    ($original:path = $disguise:expr $(,)?) => {
        let _guard = {
            use $crate::__internal::DisguiseSpec as _;
            let wrap = $crate::__internal::Wrapper($disguise);
            $crate::ScopeGuard::new(
                $crate::coerce_fn!($original),
                (&&&wrap).__resolve().__into_disguise(wrap.0),
            )
        };
    };
}

#[cfg(test)]
mod spec {
    #![expect(clippy::panic, clippy::arithmetic_side_effects, reason = "test code")]

    use std::future;

    use pretty_assertions::assert_eq;

    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct FooId(u128);

    impl FooId {
        fn new() -> Self {
            Self::disguise_or(coerce_fn!(Self::new), (), Self(0))
        }
    }

    #[derive(Debug, Clone, Default, PartialEq, Eq)]
    struct BarId(u128);

    impl BarId {
        fn new() -> Self {
            Self::disguise_or_default(coerce_fn!(Self::new), ())
        }

        fn from(id: u128) -> Self {
            Self::disguise_or_else(coerce_fn!(Self::from), (id,), Self)
        }

        fn from_sum(id1: u128, id2: u128) -> Self {
            Self::disguise_or_else(coerce_fn!(Self::from_sum), (id1, id2), |id1, id2| {
                Self(id1 + id2)
            })
        }

        fn generic<T>(id: T) -> Self
        where
            T: Into<u128> + 'static,
        {
            Self::disguise_or_else(coerce_fn!(Self::generic), (id,), |id: T| Self(id.into()))
        }

        fn non_self_return() -> u32 {
            Disguise::disguise_or_default(coerce_fn!(Self::non_self_return), ())
        }

        fn self_receiver(self) -> u128 {
            Disguise::disguise_or_else(coerce_fn!(Self::self_receiver), (self,), |this: Self| {
                this.0
            })
        }
    }

    fn standalone() -> u32 {
        Disguise::disguise_or(coerce_fn!(standalone), (), 0)
    }

    fn standalone_same_return_type() -> u32 {
        Disguise::disguise_or(coerce_fn!(standalone_same_return_type), (), 0)
    }

    fn assert(foo: u128, bar: u128) {
        assert_eq!(FooId::new(), FooId(foo));
        assert_eq!(BarId::new(), BarId(bar));
    }

    mod blocking {
        use std::{panic, thread};

        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn basic_scope() {
            assert(0, 0);

            {
                with_fn!(FooId::new = || FooId(42));
                assert(42, 0);
            }

            assert(0, 0);

            {
                with_fn!(FooId::new = FooId(42));
                assert(42, 0);
            }

            assert(0, 0);
        }

        #[test]
        fn nested_scope() {
            assert(0, 0);

            {
                with_fn!(FooId::new = FooId(42));
                assert(42, 0);

                {
                    with_fn!(FooId::new = FooId(69));
                    assert(69, 0);
                }

                assert(42, 0);
            }

            assert(0, 0);
        }

        #[test]
        fn multiple_functions() {
            assert(0, 0);

            {
                with_fn!(FooId::new = FooId(42));
                with_fn!(BarId::new = BarId(69));
                assert(42, 69);
            }

            assert(0, 0);
        }

        #[test]
        fn multiple_disguises_uses_outer() {
            assert(0, 0);

            {
                with_fn!(FooId::new = FooId(42));
                with_fn!(FooId::new = FooId(69));
                assert(69, 0);
            }

            assert(0, 0);
        }

        #[test]
        fn arguments() {
            assert_eq!(BarId::from(0), BarId(0));

            {
                with_fn!(BarId::from = |x: u128| BarId(x + 1));
                assert_eq!(BarId::from(0), BarId(1));
            }

            {
                with_fn!(BarId::from = BarId(42));
                assert_eq!(BarId::from(0), BarId(42));
            }

            {
                with_fn!(BarId::from_sum = |id1, id2| BarId(id1 * id2));
                assert_eq!(BarId::from_sum(21, 21), BarId(21 * 21));
            }

            assert_eq!(BarId::from(0), BarId(0));
        }

        #[test]
        #[expect(clippy::assertions_on_result_states, reason = "for symmetry")]
        fn panic_inside_disguise_call() {
            assert(0, 0);

            let status1 = panic::catch_unwind(|| {
                with_fn!(FooId::new = || panic!("oops"));
                assert(42, 0);
            });
            assert!(status1.is_err());

            assert(0, 0);

            let status2 = panic::catch_unwind(|| assert(0, 0));
            assert!(status2.is_ok());

            assert(0, 0);
        }

        #[test]
        fn panic_inside_disguise_scope() {
            assert(0, 0);

            let status = panic::catch_unwind(|| {
                with_fn!(FooId::new = FooId(42));
                assert(42, 0);
                panic!("oops");
            });

            assert!(status.is_err());

            assert(0, 0);
        }

        #[test]
        fn monomorphization() {
            assert_eq!(BarId::generic(21_u64), BarId(21));
            assert_eq!(BarId::generic(21_u32), BarId(21));
            assert_eq!(BarId::generic(21_u16), BarId(21));

            {
                with_fn!(BarId::generic::<u64> = |id| BarId(u128::from(id) * 2),);
                with_fn!(BarId::generic::<u32> = |id| BarId(u128::from(id) * 2),);
                assert_eq!(BarId::generic(21_u64), BarId(42));
                assert_eq!(BarId::generic(21_u32), BarId(42));
                assert_eq!(BarId::generic(21_u16), BarId(21));
            }

            assert_eq!(BarId::generic(21_u64), BarId(21));
            assert_eq!(BarId::generic(21_u32), BarId(21));
            assert_eq!(BarId::generic(21_u16), BarId(21));
        }

        #[test]
        fn thread_local() {
            assert(0, 0);

            with_fn!(FooId::new = FooId(42));
            assert(42, 0);

            let res1 = thread::spawn(|| {
                assert(0, 0);
            });

            assert!(res1.join().is_ok(), "thread spawned and ran successfully");

            let res2 = thread::spawn(|| {
                with_fn!(FooId::new = FooId(69));

                assert(69, 0);
            });

            assert!(res2.join().is_ok(), "thread spawned and ran successfully");

            assert(42, 0);
        }

        #[test]
        fn non_self_return() {
            assert_eq!(BarId::non_self_return(), 0);
            assert_eq!(standalone(), 0);
            assert_eq!(standalone_same_return_type(), 0);
            assert_eq!(BarId::new().self_receiver(), 0);

            {
                with_fn!(BarId::non_self_return = 42_u32);
                with_fn!(standalone = 69_u32);
                with_fn!(standalone_same_return_type = 666_u32);
                with_fn!(BarId::self_receiver = 123_u128);
                assert_eq!(BarId::non_self_return(), 42);
                assert_eq!(standalone(), 69);
                assert_eq!(standalone_same_return_type(), 666);
                assert_eq!(BarId::new().self_receiver(), 123);
            }

            assert_eq!(BarId::non_self_return(), 0);
            assert_eq!(standalone(), 0);
            assert_eq!(standalone_same_return_type(), 0);
            assert_eq!(BarId::new().self_receiver(), 0);
        }
    }

    mod r#async {
        use pretty_assertions::assert_eq;

        use super::*;

        async fn yield_now() {
            let mut yielded = false;
            future::poll_fn(|cx| {
                if yielded {
                    task::Poll::Ready(())
                } else {
                    yielded = true;
                    cx.waker().wake_by_ref();
                    task::Poll::Pending
                }
            })
            .await;
        }

        #[tokio::test]
        async fn basic_scope() {
            assert(0, 0);

            async {
                assert(42, 0);
            }
            .disguise_with(coerce_fn!(FooId::new), || FooId(42))
            .await;

            assert(0, 0);

            async {
                assert(42, 0);
            }
            .disguise_with_value(coerce_fn!(FooId::new), FooId(42))
            .await;

            assert(0, 0);
        }

        #[tokio::test]
        async fn nested_scope() {
            assert(0, 0);

            async {
                assert(42, 0);

                async {
                    assert(69, 0);
                }
                .disguise_with_value(coerce_fn!(FooId::new), FooId(69))
                .await;

                assert(42, 0);
            }
            .disguise_with_value(coerce_fn!(FooId::new), FooId(42))
            .await;

            assert(0, 0);
        }

        #[tokio::test]
        async fn multiple_functions() {
            assert(0, 0);

            async {
                assert(42, 69);
            }
            .disguise_with_value(coerce_fn!(FooId::new), FooId(42))
            .disguise_with_value(coerce_fn!(BarId::new), BarId(69))
            .await;

            assert(0, 0);
        }

        #[tokio::test]
        async fn multiple_disguises_uses_outer() {
            assert(0, 0);

            async {
                assert(69, 0);
            }
            .disguise_with_value(coerce_fn!(FooId::new), FooId(42))
            .disguise_with_value(coerce_fn!(FooId::new), FooId(69))
            .await;

            assert(0, 0);
        }

        #[tokio::test]
        async fn arguments() {
            assert_eq!(BarId::from(0), BarId(0));

            async {
                assert_eq!(BarId::from(0), BarId(1));
            }
            .disguise_with(coerce_fn!(BarId::from), |x: u128| BarId(x + 1))
            .await;

            async {
                assert_eq!(BarId::from(0), BarId(42));
            }
            .disguise_with_value(coerce_fn!(BarId::from), BarId(42))
            .await;

            async {
                assert_eq!(BarId::from_sum(21, 21), BarId(21 * 21));
            }
            .disguise_with(coerce_fn!(BarId::from_sum), |id1, id2| BarId(id1 * id2))
            .await;

            assert_eq!(BarId::from(0), BarId(0));
        }

        // TODO: FnMut
        #[tokio::test]
        async fn r#yield() {
            assert(0, 0);

            async {
                assert(42, 0);

                yield_now().await;
                assert(42, 0);

                yield_now().await;
                yield_now().await;
                yield_now().await;

                assert(42, 0);
            }
            .disguise_with_value(coerce_fn!(FooId::new), FooId(42))
            .await;

            assert(0, 0);
        }

        // TODO: FnMut
        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn yield_multithread_behaves_same() {
            assert(0, 0);

            async {
                assert(42, 0);

                yield_now().await;
                assert(42, 0);

                yield_now().await;
                yield_now().await;
                yield_now().await;

                assert(42, 0);
            }
            .disguise_with_value(coerce_fn!(FooId::new), FooId(42))
            .await;

            assert(0, 0);
        }

        #[tokio::test]
        #[ignore = "TODO: requires Send + unwind support"]
        async fn panic_inside_disguise_call() {
            assert(0, 0);

            async {
                assert(42, 0);
            }
            .disguise_with(coerce_fn!(FooId::new), || {
                panic!("oops");
            })
            // .catch_unwind()
            .await;

            assert(0, 0);
        }

        #[tokio::test]
        #[ignore = "TODO: requires Send + unwind support"]
        async fn panic_inside_disguise_scope() {
            assert(0, 0);

            async {
                assert(42, 0);
                panic!("oops");
            }
            .disguise_with_value(coerce_fn!(FooId::new), FooId(42))
            .await;

            assert(0, 0);
        }

        #[tokio::test]
        async fn monomorphization() {
            assert_eq!(BarId::generic(21_u64), BarId(21));
            assert_eq!(BarId::generic(21_u32), BarId(21));
            assert_eq!(BarId::generic(21_u16), BarId(21));

            async {
                assert_eq!(BarId::generic(21_u64), BarId(42));
                assert_eq!(BarId::generic(21_u32), BarId(42));
                assert_eq!(BarId::generic(21_u16), BarId(21));
            }
            .disguise_with(coerce_fn!(BarId::generic::<u64>), |id| {
                BarId(u128::from(id) * 2)
            })
            .disguise_with(coerce_fn!(BarId::generic::<u32>), |id| {
                BarId(u128::from(id) * 2)
            })
            .await;

            assert_eq!(BarId::generic(21_u64), BarId(21));
            assert_eq!(BarId::generic(21_u32), BarId(21));
            assert_eq!(BarId::generic(21_u16), BarId(21));
        }

        // TODO: replace `with_fn!` with `disguise_with_value`, but that
        // requires `Send`
        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn thread_local() {
            assert(0, 0);

            with_fn!(FooId::new = FooId(42));
            assert(42, 0);

            let res1 = tokio::spawn(async {
                assert(0, 0);
            })
            .await;

            assert!(res1.is_ok(), "task spawned and ran successfully");

            let res2 = tokio::spawn(async {
                with_fn!(FooId::new = FooId(69));

                assert(69, 0);
            })
            .await;

            assert!(res2.is_ok(), "task spawned and ran successfully");

            assert(42, 0);
        }

        #[tokio::test]
        async fn non_self_return() {
            assert_eq!(BarId::non_self_return(), 0);
            assert_eq!(standalone(), 0);
            assert_eq!(standalone_same_return_type(), 0);
            assert_eq!(BarId::new().self_receiver(), 0);

            async {
                assert_eq!(BarId::non_self_return(), 42);
                assert_eq!(standalone(), 69);
                assert_eq!(standalone_same_return_type(), 666);
                assert_eq!(BarId::new().self_receiver(), 123);
            }
            .disguise_with_value(coerce_fn!(BarId::non_self_return), 42_u32)
            .disguise_with_value(coerce_fn!(standalone), 69_u32)
            .disguise_with_value(coerce_fn!(standalone_same_return_type), 666_u32)
            .disguise_with_value(coerce_fn!(BarId::self_receiver), 123_u128)
            .await;

            assert_eq!(BarId::non_self_return(), 0);
            assert_eq!(standalone(), 0);
            assert_eq!(standalone_same_return_type(), 0);
            assert_eq!(BarId::new().self_receiver(), 0);
        }

        #[tokio::test]
        async fn join() {
            assert(0, 0);

            let fut1 = async {
                yield_now().await;
                FooId::new()
            }
            .disguise_with_value(coerce_fn!(FooId::new), FooId(42));

            let fut2 =
                async { FooId::new() }.disguise_with_value(coerce_fn!(FooId::new), FooId(69));

            assert_eq!(tokio::join!(fut1, fut2), (FooId(42), FooId(69)));
        }
    }

    mod assert {
        use pretty_assertions::assert_eq;

        use super::*;

        #[expect(clippy::inline_always, reason = "test example")]
        #[test]
        fn cannot_disguise_indirect_inline_always() {
            fn foo() -> u32 {
                0
            }

            #[inline(always)]
            fn bar() -> u32 {
                0
            }

            fn baz() -> u32 {
                Disguise::disguise_or_default(coerce_fn!(baz), ())
            }

            #[inline(always)]
            fn qux() -> u32 {
                Disguise::disguise_or_default(coerce_fn!(qux), ())
            }

            #[inline(always)]
            fn quux() -> u32 {
                Disguise::disguise_or_default(coerce_fn!(baz), ())
            }

            with_fn!(foo = || 42_u32);
            with_fn!(bar = || 69_u32);
            with_fn!(baz = || 666_u32);
            with_fn!(qux = || 123_u32);
            with_fn!(quux = || 1984_u32);

            let foofn = || Disguise::disguise_or_default(coerce_fn!(foo), ());
            let barfn = || Disguise::disguise_or_default(coerce_fn!(bar), ());

            assert_eq!(foo(), 0_u32);
            assert_eq!(bar(), 0_u32);
            assert_eq!(foofn(), 42_u32);
            assert_eq!(barfn(), 0_u32);
            assert_eq!(baz(), 666_u32);
            assert_eq!(qux(), 123_u32);
            assert_eq!(quux(), 666_u32);
        }

        #[test]
        fn recursive_disguise_reentry() {
            fn foo(n: u32) -> u32 {
                Disguise::disguise_or(coerce_fn!(foo), (n,), {
                    if n == 0 { 1 } else { bar(n - 1) }
                })
            }

            fn bar(n: u32) -> u32 {
                Disguise::disguise_or(coerce_fn!(bar), (n,), {
                    if n == 0 { 2 } else { foo(n - 1) }
                })
            }

            with_fn!(foo = |n: u32| if n == 0 { 100 } else { bar(n - 1) });
            with_fn!(bar = |n: u32| if n == 0 { 200 } else { foo(n - 1) });

            assert_eq!(foo(3), 200);
        }

        // TODO: fix?
        #[test]
        fn drop_reorder_breaks_stack() {
            assert(0, 0);
            {
                let guard1 = ScopeGuard::new(coerce_fn!(FooId::new), ValueDisguise(FooId(42)));
                let guard2 = ScopeGuard::new(coerce_fn!(FooId::new), ValueDisguise(FooId(69)));

                assert(69, 0);

                drop(guard1);

                assert(0, 0);

                drop(guard2);

                assert(42, 0);
            }

            assert(42, 0);
        }

        #[test]
        fn composed_disguises_call_each_other() {
            fn foo() -> u32 {
                Disguise::disguise_or(coerce_fn!(foo), (), bar())
            }

            fn bar() -> u32 {
                Disguise::disguise_or(coerce_fn!(bar), (), 1)
            }

            assert_eq!(foo(), 1);

            with_fn!(foo = || bar() + 10);
            with_fn!(bar = 5_u32);

            assert_eq!(foo(), 15);
        }

        #[test]
        fn only_static_references() {
            fn greet(name: &'static str) -> String {
                Disguise::disguise_or_else(coerce_fn!(greet), (name,), |name| {
                    format!("Hello, {name}!")
                })
            }

            assert_eq!(greet("Alice"), "Hello, Alice!");
            with_fn!(greet = "Hello, disguised!".to_owned());
            assert_eq!(greet("Alice"), "Hello, disguised!");
        }
    }
}
