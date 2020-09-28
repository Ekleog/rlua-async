use rlua::Lua;
use rlua_async::{ChunkExt, ContextExt};
use futures::executor::block_on;

async fn say(n: u32) {
    println!("number: {}", n);
}

fn main() -> rlua::Result<()> {
    let lua = Lua::new();
    lua.context(|ctx| {
        let globals = ctx.globals();
        let map_table = ctx.create_table()?;
        map_table.set(
            "say",
            ctx.create_async_function(|_ctx, param: u32| async move {
                say(param).await;
                Ok(())
            })?,
        )?;
        globals.set("foo", map_table)
    })?;
    lua.context(|ctx| {
        let chunk = ctx.load("foo.say(42)");
        block_on(chunk.exec_async(ctx))
    })?;
    Ok(())
}
