function(f)
    return function(...)
        local fut = f(...)
        while true do
            local t, ready = fut:poll()
            if ready then
                return table.unpack(t)
            else
                coroutine.yield()
            end
        end
    end
end
