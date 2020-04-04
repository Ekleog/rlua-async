//! See the [README](https://github.com/Ekleog/rlua-async) for more general information
// TODO: uncomment when stable #![doc(include = "../README.md")]

// # Implementation details note
//
// Having a `Poll::Pending` returned from a Rust `async` function will trigger a
// `coroutine.yield()`, with no arguments and that doesn't use its return value.
// This `coroutine.yield()` will bubble up to the closest `create_thread` Rust call (assuming the
// Lua code doesn't use coroutines in-between, which would break all hell loose).

use std::{
    future::Future,
    marker::PhantomData,
    pin::Pin,
    task::{self, Poll},
};

use futures::future;
use rlua::*;
use scoped_tls::scoped_thread_local;

/// A "prelude" that provides all the extension traits that need to be in scope for the
/// `async`-related functions to be usable.
pub mod prelude {
    pub use super::{ContextExt, FunctionExt};
}

// Safety invariant: This always points to a valid `task::Context`.
// This is guaranteed by the fact that it is *always* set with
// `FUTURE_CTX.set(&(ctx as *mut _ as *mut ()), ...`, meaning that the `ctx` lifetime cannot be
// less than the time this pointer is in here.
// TODO: Is there a way to not be that awful? I don't think so, because:
//  * we can't just store an `&mut` in a thread-local for lack of lifetime, and
//  * we can't clone the `Context`, as it's not `Clone`
scoped_thread_local!(static FUTURE_CTX: *mut ());

/// Extension trait for [`rlua::Context`]
pub trait ContextExt<'lua> {
    /// Create an asynchronous function.
    ///
    /// This works exactly like [`Context::create_function`], except that the function returns a
    /// [`Future`] instead of just the result. Note that when this function is being called from
    /// Lua, it will generate a coroutine, that will prevent any use of coroutines in the said Lua
    /// code and is designed to be called from an `async`-compliant caller such as
    /// [`FunctionExt::call_async`]
    fn create_async_function<Arg, Ret, RetFut, F>(self, func: F) -> Result<Function<'lua>>
    where
        Arg: FromLuaMulti<'lua>,
        Ret: ToLua<'lua>,
        RetFut: 'static + Send + Future<Output = Result<Ret>>,
        F: 'static + Send + Fn(Context<'lua>, Arg) -> RetFut;
}

impl<'lua> ContextExt<'lua> for Context<'lua> {
    fn create_async_function<Arg, Ret, RetFut, F>(self, func: F) -> Result<Function<'lua>>
    where
        Arg: FromLuaMulti<'lua>,
        Ret: ToLuaMulti<'lua>,
        // TODO: 'static below should probably be 'lua instead -- need to figure out a way to work
        // around the 'static bound on create_function
        RetFut: 'static + Send + Future<Output = Result<Ret>>,
        F: 'static + Send + Fn(Context<'lua>, Arg) -> RetFut,
    {
        let wrapped_fun = self.create_function(move |ctx, arg| {
            let mut fut = Box::pin(func(ctx, arg)); // TODO: maybe we can avoid this pin?
            ctx.create_function_mut(move |ctx, _: MultiValue<'lua>| {
                FUTURE_CTX.with(|fut_ctx| {
                    let fut_ctx_ref = unsafe { &mut *(*fut_ctx as *mut task::Context) };
                    match Future::poll(fut.as_mut(), fut_ctx_ref) {
                        Poll::Pending => ToLuaMulti::to_lua_multi((false, 1), ctx),
                        Poll::Ready(v) => {
                            // Note: .into_iter() is basically .into_vec().into_iter(), so there is
                            // no gain to be had by using it
                            let mut v = ToLuaMulti::to_lua_multi(v?, ctx)?.into_vec();
                            v.push(rlua::Value::Boolean(true));
                            let len = v.len();
                            ToLuaMulti::to_lua_multi((v, len), ctx)
                        }
                    }
                })
            })
        })?;

        self.load(
            r#"
                function(f)
                    return function(...)
                        local poll = f(...)
                        while true do
                            local t, n = poll()
                            if n == 1 then
                                if t then
                                    return
                                else
                                    coroutine.yield()
                                end
                            else
                                if t[n] then
                                    return table.unpack(t, 1, n - 1)
                                else
                                    coroutine.yield()
                                end
                            end
                        end
                    end
                end
            "#,
        )
        .set_name(b"coroutine yield helper")?
        .eval::<Function<'lua>>()? // TODO: find some way to cache this eval, maybe?
        .call(wrapped_fun)
    }
}

/// Extension trait for [`rlua::Function`]
pub trait FunctionExt<'lua> {
    /// Calls the function in an async-compliant way.
    ///
    /// By using this on the Rust side, you can recover as a [`Future`] the potentiall
    /// [`Poll::Pending`] that might have been sent by eg. a downstream
    /// [`ContextExt::create_async_function`]
    // TODO: make the return type `impl trait`... when GAT + existential types will be stable?
    fn call_async<'ret, Arg, Ret>(
        &self,
        ctx: Context<'lua>,
        args: Arg,
    ) -> Pin<Box<dyn Future<Output = Result<Ret>> + 'ret>>
    where
        'lua: 'ret,
        Arg: ToLuaMulti<'lua>,
        Ret: 'ret + FromLuaMulti<'lua>;
}

impl<'lua> FunctionExt<'lua> for Function<'lua> {
    fn call_async<'ret, Arg, Ret>(
        &self,
        ctx: Context<'lua>,
        args: Arg,
    ) -> Pin<Box<dyn Future<Output = Result<Ret>> + 'ret>>
    where
        'lua: 'ret,
        Arg: ToLuaMulti<'lua>,
        Ret: 'ret + FromLuaMulti<'lua>,
    {
        struct RetFut<'lua, Ret> {
            ctx: Context<'lua>,
            thread: Thread<'lua>,
            _phantom: PhantomData<Ret>,
        }

        impl<'lua, Ret> Future for RetFut<'lua, Ret>
        where
            Ret: FromLuaMulti<'lua>,
        {
            type Output = Result<Ret>;

            fn poll(self: Pin<&mut Self>, fut_ctx: &mut task::Context) -> Poll<Result<Ret>> {
                FUTURE_CTX.set(&(fut_ctx as *mut _ as *mut ()), || {
                    match self.thread.resume::<_, rlua::MultiValue>(()) {
                        Err(e) => Poll::Ready(Err(e)),
                        Ok(v) => {
                            match self.thread.status() {
                                ThreadStatus::Resumable => Poll::Pending,

                                ThreadStatus::Unresumable => {
                                    Poll::Ready(FromLuaMulti::from_lua_multi(v, self.ctx))
                                }

                                // The `Error` case should be caught by the `Err(e)` match above
                                ThreadStatus::Error => unreachable!(),
                            }
                        }
                    }
                })
            }
        }

        let bound_fun = match self.bind(args) {
            Ok(bound_fun) => bound_fun,
            Err(e) => return Box::pin(future::err(e)),
        };

        let thread = match ctx.create_thread(bound_fun) {
            Ok(thread) => thread,
            Err(e) => return Box::pin(future::err(e)),
        };

        Box::pin(RetFut {
            ctx,
            thread,
            _phantom: PhantomData,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use futures::executor;

    #[test]
    fn it_works() {
        let lua = Lua::new();

        lua.context(|lua_ctx| {
            let globals = lua_ctx.globals();

            let f = lua_ctx
                .create_async_function(|_, a: usize| future::ok(a + 1))
                .unwrap();
            globals.set("f", f).unwrap();

            assert_eq!(
                executor::block_on(
                    lua_ctx
                        .load(r#"function(a) return f(a) - 1 end"#)
                        .eval::<Function>()
                        .unwrap()
                        .call_async::<_, usize>(lua_ctx, 2)
                )
                .unwrap(),
                2
            );
        });
    }
}
