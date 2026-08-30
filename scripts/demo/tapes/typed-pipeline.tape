# Objects survive the pipe: every stage addresses fields, never columns.
title ono — structure survives the pipe
size 96 22
wait 1.2
run get process | sort memory desc | select pid ppid name memory
wait 2.2
run get process | where name == "nginx" | select pid ppid name state
wait 2.2
run get socket | where state == "listen" | select protocol local.port state
wait 2.2
run type get process
wait 2.8
