ssh root@10.0.0.110 "killall arty 2>/dev/null"; ssh root@10.0.0.126 'kill $(ps | grep arty | grep -v grep | awk "{print $1}")'
