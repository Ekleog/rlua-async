function(ud)
    return function(...)
        ud:set_arg(...)
        while true do
            local t, ready = ud:poll()
            if ready then
                return table.unpack(t)
            else
                coroutine.yield()
            end
        end
    end
end
