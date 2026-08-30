# The relationship tree comes from the kernel, not from a parser — and it composes.
title ono — relationships the kernel asserts
size 96 22
wait 1.2
run get process | where name == "nginx" | select pid ppid name state
wait 2.2
run get process | where name == "nginx" | take 1 | trace process --depth 1
wait 3.4
run get process | where name == "redis-server" | trace process --depth 1
wait 3.2
