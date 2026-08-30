# v0.4: the machine is a place — find the web server without ever typing its name.
title ono — the machine is a place
size 96 24
wait 1.2
run look
wait 2.0
run find place --where local.port == 8080
wait 2.0
run enter 0.0.0.0:8080
wait 1.0
run near --type process
wait 2.4
run follow owner
wait 2.8
run look
wait 2.4
run trail
wait 2.8
