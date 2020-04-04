function(f)
    return function(...)
        local poll = f(...)
        while true do
            local t, ready = poll()
            if ready then
                return table.unpack(t)
            else
                coroutine.yield()
            end
        end
    end
end
