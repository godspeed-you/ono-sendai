# v0.3: the Unix tools you already know become typed — and `explain` shows the whole trick.
title ono — the Unix tools you know, typed on demand
size 100 30
wait 1.2
run ps aux | head -3
wait 2.4
run ps aux | where user == "case" | select pid user name state
wait 2.4
run explain ps aux | where state == "running"
wait 4.0
