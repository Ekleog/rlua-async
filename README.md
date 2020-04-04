# rlua-async [![Crates.io](https://img.shields.io/crates/v/rlua-async.svg)](https://crates.io/crates/rlua-async) [![Documentation](https://docs.rs/rlua-async/badge.svg)](https://docs.rs/rlua-async)

`rlua-async` provides a way for asynchronous Rust code using `rlua` to
seamlessly interface with Lua code, the only condition being that the Lua code
must not make use of coroutines, as it is the way the `async` handling is
proxied through Lua.

## Make `async` builtins available to Lua code

The first thing you need to do is to make `async` builtins available to Lua
code. You can do it via eg.
[`ContextExt::create_async_function`](https://docs.rs/rlua-async/latest/rlua_async/trait.ContextExt.html#tymethod.create_async_function).

Once these builtins are available, they can be called by Lua code. This Lua code
must not use coroutines, as the coroutines are an essential part of how
`rlua-async` works internally.

## Call Lua code asynchronously

In addition to this, to actually make the code `async`, it is also required to
retrieve a `Future` when trying to call functions -- otherwise, evaluation is
not going to be asynchronous.

To do that, you can use methods like
[`FunctionExt::call_async`](https://docs.rs/rlua-async/latest/rlua_async/trait.FunctionExt.html#tymethod.call_async).
